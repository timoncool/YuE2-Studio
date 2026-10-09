import { describe, expect, it } from 'vitest';
import { engravedScore } from './scoreEngraving';

const head = 'X:1\nM:2/4\nL:1/32\nK:Em\n';

describe('engravedScore', () => {
  it('draws a multi-measure rest as whole-bar rests', () => {
    expect(engravedScore(`${head}V: Vocal\nZ4|\nV: Ins\nZ|E4B4f2g4d2-|\n`)).toBe(`${head}V: Vocal\nz16|z16|z16|z16|\nV: Ins\nz16|E4B4f2g4d2-|\n`);
  });

  it('leaves headers, comments, chords and plain rests alone', () => {
    const score = `${head}% intro Z4|\nV: Vocal\n"Em"z16|e8b8|\n`;
    expect(engravedScore(score)).toBe(score);
  });

  it('reads common time and leaves a score without a bar length as it is', () => {
    expect(engravedScore('X:1\nM:C\nL:1/8\nK:C\nZ2|\n')).toBe('X:1\nM:C\nL:1/8\nK:C\nz8|z8|\n');
    expect(engravedScore('X:1\nK:C\nZ2|\n')).toBe('X:1\nK:C\nZ2|\n');
  });
});
