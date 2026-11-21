const words = new Intl.Segmenter('und', { granularity: 'word' });

export function sungWords(text: string): string[] {
  const normal = text.normalize('NFKC').toLowerCase().replace(/\[[^\]\n]*\]/gu, ' ').replace(/[’‘]/gu, "'");
  return Array.from(words.segment(normal)).filter(part => part.isWordLike).map(part => part.segment);
}

/** Word error rate accounts for inserted, missing and substituted lyrics in order. */
export async function wordMatchPercent(
  reference: string[], transcript: string[], obsolete?: () => boolean,
): Promise<number | null> {
  if (!reference.length || !transcript.length) return null;
  const [rows, columns] = reference.length >= transcript.length ? [reference, transcript] : [transcript, reference];
  let first = 0;
  while (first < columns.length && rows[first] === columns[first]) first++;
  let lastRow = rows.length;
  let lastColumn = columns.length;
  while (lastColumn > first && rows[lastRow - 1] === columns[lastColumn - 1]) {
    lastRow--;
    lastColumn--;
  }
  const width = lastColumn - first;
  let distance = lastRow - first;
  if (width) {
    let previous = Uint32Array.from({ length: width + 1 }, (_, i) => i);
    let current = new Uint32Array(width + 1);
    let cells = 0;
    for (let i = first; i < lastRow; i++) {
      current[0] = i - first + 1;
      for (let j = 1; j <= width; j++) {
        current[j] = Math.min(previous[j] + 1, current[j - 1] + 1,
          previous[j - 1] + Number(rows[i] !== columns[first + j - 1]));
      }
      [previous, current] = [current, previous];
      cells += width;
      if (cells >= 500_000) {
        await new Promise<void>(resolve => setTimeout(resolve, 0));
        if (obsolete?.()) return null;
        cells = 0;
      }
    }
    distance = previous[width];
  }
  return Math.max(0, Math.round(100 * (1 - distance / reference.length)));
}
