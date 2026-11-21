const sectionTag = /^\[[^\]]+\]$/;
const nonemptyLines = (text: string) => text.split(/\r\n?|\n/).map(line => line.trim()).filter(Boolean);

export function hasOnlySectionTags(lyrics: string): boolean {
  const lines = nonemptyLines(lyrics);
  return lines.length > 0 && lines.every(line => sectionTag.test(line));
}

/** Match the Instrumental LoRA's training input even when the form has no lyrics. */
export function sectionTagsOnly(lyrics: string): string {
  const sections = nonemptyLines(lyrics).filter(line => sectionTag.test(line));
  return sections.join('\n\n') || '[instrumental]';
}
