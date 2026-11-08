import type { PDFDocumentLoadingTask, PDFDocumentProxy } from "pdfjs-dist";
import { basename } from "../../basename";
import type { MediaDelivery } from "./types";

// The media protocol answers at most 1 MiB per range request.
const MAX_RANGE_BYTES = 1024 * 1024;
const RANGE_CHUNK_BYTES = 256 * 1024;
const THUMBNAIL_WIDTH_LIMIT = 200;
const CACHED_PAGES = 6;
const CACHED_THUMBNAILS = 30;

// Text PDFs that lean on Adobe's predefined CMaps (Japanese text often does)
// or on the standard fonts read them from these files, bundled with the app.
const BUNDLED_DATA: Record<string, ReadonlyMap<string, string>> = {
  cMapUrl: byFileName(import.meta.glob<string>("../../../node_modules/pdfjs-dist/cmaps/*.bcmap", {
    query: "?url&no-inline",
    import: "default",
    eager: true,
  })),
  standardFontDataUrl: byFileName(import.meta.glob<string>("../../../node_modules/pdfjs-dist/standard_fonts/*.{pfb,ttf}", {
    query: "?url&no-inline",
    import: "default",
    eager: true,
  })),
};

export interface PdfPageSize {
  width: number;
  height: number;
}

export interface PdfDocument {
  readonly pageCount: number;
  pageSize(index: number): Promise<PdfPageSize>;
  /**
   * The page drawn `width` CSS pixels wide on a canvas of its own. The same
   * canvas comes back for the same page and width while it stays cached.
   */
  render(index: number, width: number): Promise<HTMLCanvasElement>;
  destroy(): void;
}

type Pdfjs = typeof import("pdfjs-dist");
let pdfjs: Promise<Pdfjs> | null = null;

function loadPdfjs(): Promise<Pdfjs> {
  pdfjs ??= Promise.all([
    import("pdfjs-dist"),
    import("pdfjs-dist/build/pdf.worker.min.mjs?url"),
  ]).then(([module, worker]) => {
    module.GlobalWorkerOptions.workerSrc = worker.default;
    return module;
  });
  return pdfjs;
}

/**
 * Opens a PDF served by the media protocol. Streaming reads only the byte
 * ranges the shown pages need, so a long book opens at any page without
 * loading it whole; "blob" fetches the file first, for a webview whose media
 * protocol cannot answer ranges (CEF).
 */
export async function openPdfDocument(url: string, delivery: MediaDelivery = "stream"): Promise<PdfDocument> {
  const module = await loadPdfjs();
  const common = {
    disableAutoFetch: true,
    disableStream: true,
    useWasm: false,
    cMapPacked: true,
    useWorkerFetch: false,
    BinaryDataFactory: BundledData,
    verbosity: module.VerbosityLevel.ERRORS,
  };
  if (delivery === "blob") {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`the PDF could not be read (status ${response.status})`);
    const task = module.getDocument({ ...common, data: new Uint8Array(await response.arrayBuffer()) });
    return new LoadedPdf(task, await task.promise);
  }
  const head = await fetch(url, { method: "HEAD" });
  const length = Number(head.headers.get("content-length"));
  if (!head.ok || !Number.isSafeInteger(length) || length <= 0) {
    throw new Error(`the PDF could not be read (status ${head.status})`);
  }
  // A failed range ends the load with its own error: pdf.js keeps waiting
  // for the bytes otherwise.
  let fail: (error: unknown) => void = () => undefined;
  const failed = new Promise<never>((_, reject) => {
    fail = reject;
  });
  let task: PDFDocumentLoadingTask | null = null;
  class RangeTransport extends module.PDFDataRangeTransport {
    override requestDataRange(begin: number, end: number) {
      readRange(url, begin, end).then(
        (bytes) => this.onDataRange(begin, bytes),
        (error: unknown) => {
          fail(error);
          void task?.destroy();
        },
      );
    }
  }
  task = module.getDocument({
    ...common,
    range: new RangeTransport(length, null),
    rangeChunkSize: RANGE_CHUNK_BYTES,
  });
  return new LoadedPdf(task, await Promise.race([task.promise, failed]));
}

class BundledData {
  async fetch({ kind, filename }: { kind: string; filename: string }): Promise<Uint8Array> {
    const url = BUNDLED_DATA[kind]?.get(filename);
    if (!url) throw new Error(`the PDF needs ${filename}, which is not bundled`);
    const response = await fetch(url);
    if (!response.ok) throw new Error(`${filename} could not be read (status ${response.status})`);
    return new Uint8Array(await response.arrayBuffer());
  }
}

function byFileName(urls: Record<string, string>): ReadonlyMap<string, string> {
  return new Map(Object.entries(urls).map(([path, url]) => [basename(path), url]));
}

async function readRange(url: string, begin: number, end: number): Promise<Uint8Array> {
  const bytes = new Uint8Array(end - begin);
  for (let start = begin; start < end; start += MAX_RANGE_BYTES) {
    const stop = Math.min(end, start + MAX_RANGE_BYTES);
    const response = await fetch(url, { headers: { Range: `bytes=${start}-${stop - 1}` } });
    if (response.status !== 206) throw new Error(`the PDF could not be read (status ${response.status})`);
    const chunk = new Uint8Array(await response.arrayBuffer());
    if (chunk.byteLength !== stop - start) throw new Error("the PDF could not be read (short range)");
    bytes.set(chunk, start - begin);
  }
  return bytes;
}

class LoadedPdf implements PdfDocument {
  readonly pageCount: number;
  readonly #task: PDFDocumentLoadingTask;
  readonly #pdf: PDFDocumentProxy;
  readonly #pages = new Map<string, Promise<HTMLCanvasElement>>();
  readonly #thumbnails = new Map<string, Promise<HTMLCanvasElement>>();

  constructor(task: PDFDocumentLoadingTask, pdf: PDFDocumentProxy) {
    this.#task = task;
    this.#pdf = pdf;
    this.pageCount = pdf.numPages;
  }

  async pageSize(index: number): Promise<PdfPageSize> {
    const page = await this.#pdf.getPage(index + 1);
    const { width, height } = page.getViewport({ scale: 1 });
    return { width, height };
  }

  render(index: number, width: number): Promise<HTMLCanvasElement> {
    const cssWidth = Math.max(1, Math.round(width));
    const thumbnail = cssWidth < THUMBNAIL_WIDTH_LIMIT;
    const cache = thumbnail ? this.#thumbnails : this.#pages;
    const key = `${index}:${cssWidth}`;
    const cached = cache.get(key);
    if (cached) {
      cache.delete(key);
      cache.set(key, cached);
      return cached;
    }
    const drawn = this.#draw(index, cssWidth);
    cache.set(key, drawn);
    drawn.catch(() => cache.delete(key));
    const limit = thumbnail ? CACHED_THUMBNAILS : CACHED_PAGES;
    for (const oldest of cache.keys()) {
      if (cache.size <= limit) break;
      cache.delete(oldest);
    }
    return drawn;
  }

  async #draw(index: number, cssWidth: number): Promise<HTMLCanvasElement> {
    const page = await this.#pdf.getPage(index + 1);
    const base = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: (cssWidth / base.width) * (window.devicePixelRatio || 1) });
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.floor(viewport.width));
    canvas.height = Math.max(1, Math.floor(viewport.height));
    await page.render({ canvas, viewport }).promise;
    return canvas;
  }

  destroy(): void {
    this.#pages.clear();
    this.#thumbnails.clear();
    void this.#task.destroy();
  }
}
