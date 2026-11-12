import { describe, expect, it } from 'vitest';
import { bedLine } from './scoreApi';

describe('bedLine', () => {
  it('names each section the bed has, with the chords it plays there', () => {
    expect(bedLine([{ label: 'verse', chords: ['C', 'F', 'G', 'F'] }, { label: 'chorus', chords: ['C', 'G', 'Am', 'Em'] }]))
      .toBe('verse: C F G F · chorus: C G Am Em');
    expect(bedLine([])).toBe('');
  });
});
