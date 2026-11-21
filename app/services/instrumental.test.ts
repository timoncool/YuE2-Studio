import { describe, expect, it } from 'vitest';
import { sectionTagsOnly } from './instrumental';

describe('an instrumental keeps the song structure without words', () => {
  it('keeps the section tags and drops every sung line', () => {
    expect(sectionTagsOnly('[Verse 1]\nneon on the glass\r\nwe drive\n\n[Chorus]\nhold on\n[Instrumental Break]\n')).toBe('[Verse 1]\n\n[Chorus]\n\n[Instrumental Break]');
    expect(sectionTagsOnly('just words')).toBe('[instrumental]');
    expect(sectionTagsOnly('')).toBe('[instrumental]');
  });
});
