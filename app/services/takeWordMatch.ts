const words = new Intl.Segmenter('und', { granularity: 'word' });

export function sungWords(text: string): string[] {
  const normal = text.normalize('NFKC').toLowerCase().replace(/\[[^\]\n]*\]/gu, ' ').replace(/[’‘]/gu, "'");
  return Array.from(words.segment(normal)).filter(part => part.isWordLike).map(part => part.segment);
}

/** Word error rate accounts for inserted, missing and substituted lyrics in order. */
export function wordMatchPercent(reference: string[], transcript: string[]): number | null {
  if (!reference.length || !transcript.length) return null;
  // Imported lyrics and recogniser output can be arbitrarily long. Keep the UI responsive.
  if (reference.length * transcript.length > 4_000_000) return null;
  let previous = Uint32Array.from({ length: transcript.length + 1 }, (_, i) => i);
  let current = new Uint32Array(previous.length);
  for (let i = 1; i <= reference.length; i++) {
    current[0] = i;
    for (let j = 1; j <= transcript.length; j++) {
      current[j] = Math.min(previous[j] + 1, current[j - 1] + 1,
        previous[j - 1] + Number(reference[i - 1] !== transcript[j - 1]));
    }
    [previous, current] = [current, previous];
  }
  return Math.max(0, Math.round(100 * (1 - previous[transcript.length] / reference.length)));
}
