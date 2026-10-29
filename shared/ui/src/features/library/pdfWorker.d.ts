declare module "pdfjs-dist/build/pdf.worker.min.mjs?url" {
  const url: string;
  export default url;
}

// The host app's bundler (Vite) resolves these at build time.
interface ImportMeta {
  glob<T>(pattern: string, options: { query: string; import: "default"; eager: true }): Record<string, T>;
}
