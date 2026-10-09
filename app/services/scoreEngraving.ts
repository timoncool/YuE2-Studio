/**
 * The score as it is engraved: abcjs widens a line far past the page when one
 * voice holds a multi-measure rest (`Z4|`) beside bars of notes in another, so
 * each becomes that many whole-bar rests. Only the drawing changes; the score
 * the model reads keeps its `Z`.
 */

const fraction = (text: string): number | null => {
  const found = /^\s*(\d+)\s*\/\s*(\d+)/.exec(text);
  if (!found) return null;
  const value = Number(found[1]) / Number(found[2]);
  return value > 0 ? value : null;
};

/** The bar in units of the default note length, or null when the header does not say. */
const barUnits = (abc: string): number | null => {
  const meterLine = /^M:(.*)$/m.exec(abc)?.[1].trim();
  const lengthLine = /^L:(.*)$/m.exec(abc)?.[1];
  const meter = meterLine === 'C' ? 1 : meterLine === 'C|' ? 1 : meterLine ? fraction(meterLine) : null;
  const length = lengthLine ? fraction(lengthLine) : null;
  if (!meter || !length) return null;
  const units = meter / length;
  return Number.isInteger(units) ? units : null;
};

export function engravedScore(abc: string): string {
  const units = barUnits(abc);
  if (!units) return abc;
  return abc
    .split('\n')
    .map(line => (/^[A-Za-z]:/.test(line) || line.startsWith('%')
      ? line
      : line.replace(/(^|[|\s\]])Z(\d*)(?=\s*(\||$))/g, (_, before: string, count: string) => {
        const bars = count ? Number(count) : 1;
        return `${before}${Array.from({ length: bars }, () => `z${units}`).join('|')}`;
      })))
    .join('\n');
}
