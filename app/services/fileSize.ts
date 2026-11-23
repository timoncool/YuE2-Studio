/** A download or file size in the interface language: «5,1 ГБ», «52 МБ», "5.1 GB". Binary units, as the downloads
 * and the disks count them; never zero for a real file. */
export function formatSize(bytes: number, language: string, digits = 1): string {
  const unit = bytes >= 1024 ** 3 ? 'gigabyte' : bytes >= 1024 ** 2 ? 'megabyte' : 'kilobyte';
  const scale = unit === 'gigabyte' ? 1024 ** 3 : unit === 'megabyte' ? 1024 ** 2 : 1024;
  const value = unit === 'gigabyte' ? bytes / scale : Math.max(bytes > 0 ? 1 : 0, Math.round(bytes / scale));
  return new Intl.NumberFormat(language, {
    style: 'unit', unit, unitDisplay: 'short', maximumFractionDigits: unit === 'gigabyte' ? digits : 0,
  }).format(value);
}

/** A size in gigabytes only, for a sentence that already says «ГБ» after the number. */
export function formatGigabytes(bytes: number, language: string, digits = 1): string {
  return new Intl.NumberFormat(language, { maximumFractionDigits: digits }).format(bytes / 1024 ** 3);
}
