/** Match the Instrumental LoRA's training input even when the form has no lyrics. */
export function sectionTagsOnly(lyrics: string): string {
  const sections = lyrics.replace(/\r\n?/g, '\n').split('\n').map(line => line.trim()).filter(line => /^\[[^\]]+\]$/.test(line));
  return sections.join('\n\n') || '[instrumental]';
}
