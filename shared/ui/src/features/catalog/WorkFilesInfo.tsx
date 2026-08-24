import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway, CatalogRom } from "./types";
import { RomFileTree } from "./RomFileTree";

export function WorkFilesInfo({
  workCode,
  roms,
  releaseDate,
  gateway,
}: {
  workCode: string;
  roms: CatalogRom[];
  releaseDate: string;
  gateway: CatalogDetailGateway;
}) {
  const { locale, t } = usePresentation();
  if (!roms.length) return <p className="work-detail-empty">{t("detail.files.noMetadata")}</p>;

  const groups = groupRoms(roms, releaseDate, t("detail.files.unknownDate"));
  return (
    <div className="rom-thread">
      {groups.map(([label, items]) => (
        <section className="rom-group" key={label}>
          <div className="rom-row rom-row--header">
            <div className="rom-gutter" aria-hidden="true">
              <div className="rom-threadline"><div className="rom-continuation" /></div>
              <div className="rom-group-chip">{label}</div>
            </div>
            <div className="rom-header">
              <span className="rom-header-count">
                {t(items.length === 1 ? "detail.files.countOne" : "detail.files.countMany", {
                  count: items.length.toLocaleString(locale),
                })}
              </span>
            </div>
          </div>
          <div className="rom-children">
            {items.map((rom, index) => {
              const position = roms.indexOf(rom);
              return (
                <div className={`rom-row rom-row--child ${index === items.length - 1 ? "is-last" : ""}`} key={`${rom.name}:${rom.size}:${position}`}>
                  <div className="rom-gutter" aria-hidden="true">
                    <div className="rom-threadline">
                      <div className="rom-continuation" />
                      <div className="rom-connection" />
                    </div>
                  </div>
                  <article className="rom-file-card">
                    <div className="rom-file-top"><div className="rom-file-name">{rom.name || t("detail.files.unnamedArchive")}</div></div>
                    <dl className="rom-file-hashes">
                      <FileValue label={t("detail.files.size")} value={formatSize(rom.size, locale, t("detail.files.bytes"))} />
                      <FileValue label="CRC" value={rom.crc} />
                      <FileValue label="MD5" value={rom.md5} />
                      <FileValue label="SHA1" value={rom.sha1} />
                      <FileValue label="SHA256" value={rom.sha256} />
                    </dl>
                    {index === 0 && (
                      <RomFileTree
                        workCode={workCode}
                        romPosition={position}
                        fileCount={rom.fileCount}
                        gateway={gateway}
                      />
                    )}
                  </article>
                </div>
              );
            })}
          </div>
        </section>
      ))}
    </div>
  );
}

export function groupRoms(
  roms: CatalogRom[],
  releaseDate: string,
  unknownDate: string,
): Array<[string, CatalogRom[]]> {
  const groups = new Map<string, CatalogRom[]>();
  for (const rom of roms) {
    const date = rom.updateDate || releaseDate || unknownDate;
    const label = rom.version ? `${date} (${rom.version})` : date;
    const items = groups.get(label) ?? [];
    items.push(rom);
    groups.set(label, items);
  }
  return [...groups.entries()].sort(([left], [right]) => right.slice(0, 10).localeCompare(left.slice(0, 10)));
}

export function formatSize(raw: string, locale: string, bytesLabel: string): string {
  if (!raw) return "—";
  const bytes = Number(raw);
  if (!Number.isFinite(bytes)) return raw;
  const units = [bytesLabel, "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const exact = bytes.toLocaleString(locale);
  if (unit === 0) return `${exact} ${bytesLabel}`;
  return `${value.toLocaleString(locale, { maximumFractionDigits: 1 })} ${units[unit]} (${exact} ${bytesLabel})`;
}

function FileValue({ label, value }: { label: string; value: string }) {
  return <div><dt>{label}:</dt><dd>{value || "—"}</dd></div>;
}
