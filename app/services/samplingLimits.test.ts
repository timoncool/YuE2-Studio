import { describe, expect, it } from 'vitest';
import { minTokensCutsTheSongOff } from './samplingLimits';

describe('a minimum of frames that leaves the song no room to end', () => {
  it('warns from 90 % of the frames the duration allows', () => {
    // 240 s are 6000 frames; 90 % of them is 5400
    expect(minTokensCutsTheSongOff('6000', 240)).toBe(true);
    expect(minTokensCutsTheSongOff('5400', 240)).toBe(true);
    expect(minTokensCutsTheSongOff('5399', 240)).toBe(false);
    expect(minTokensCutsTheSongOff('6000', 220)).toBe(true);
  });

  it('says nothing for an empty or meaningless field', () => {
    expect(minTokensCutsTheSongOff('', 240)).toBe(false);
    expect(minTokensCutsTheSongOff('  ', 240)).toBe(false);
    expect(minTokensCutsTheSongOff('abc', 240)).toBe(false);
    expect(minTokensCutsTheSongOff('0', 240)).toBe(false);
    expect(minTokensCutsTheSongOff('200', 240)).toBe(false);
  });
});
