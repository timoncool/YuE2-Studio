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

/** Use the PDF's embedded font only for accidentals, leaving the rest of each chord in its usual font. */
export function fontScoreAccidentals(host: ParentNode, fontFamily: string): boolean {
  let found = false;
  for (const chord of host.querySelectorAll('text[data-name="chord"]')) {
    const walker = document.createTreeWalker(chord, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    while (walker.nextNode()) nodes.push(walker.currentNode as Text);
    for (const node of nodes) {
      const parts = node.data.split(/([♭♮♯]+)/);
      if (parts.length === 1) continue;
      found = true;
      node.replaceWith(...parts.map((part, index) => {
        if (index % 2 === 0) return part;
        const span = document.createElementNS(chord.namespaceURI, 'tspan');
        span.setAttribute('font-family', fontFamily);
        span.setAttribute('font-style', 'normal');
        span.setAttribute('font-weight', 'normal');
        span.textContent = part;
        return span;
      }));
    }
  }
  return found;
}
