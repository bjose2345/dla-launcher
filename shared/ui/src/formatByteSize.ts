export function formatByteSize(bytes: number, locale = "en-US"): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / (1024 ** unit);
  const maximumFractionDigits = value >= 100 || unit === 0 ? 0 : 1;
  return `${value.toLocaleString(locale, { minimumFractionDigits: 0, maximumFractionDigits })} ${units[unit]}`;
}
