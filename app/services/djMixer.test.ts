import { describe, it, expect } from 'vitest';
import { crossfadeGains, rateForSync, songBpm } from './djMixer';
import type { Song } from '../types';

describe('DJ mixing math', () => {
  it('keeps both decks audible at center and uses equal power through the crossfader', () => {
    expect(crossfadeGains(0)).toEqual([1, 0]);
    const [a, b] = crossfadeGains(0.5);
    expect(a).toBeCloseTo(Math.SQRT1_2);
    expect(b).toBeCloseTo(Math.SQRT1_2);
    expect(a * a + b * b).toBeCloseTo(1);
    expect(crossfadeGains(1)[0]).toBeCloseTo(0);
  });

  it('matches deck speed to the other deck BPM and tempo', () => {
    expect(rateForSync(100, 125, 1)).toBe(1.25);
    expect(rateForSync(140, 105, 1)).toBe(0.75);
    expect(rateForSync(60, 180, 1)).toBe(2);
  });

  it('reads BPM metadata and song style as a fallback', () => {
    expect(songBpm(null)).toBeNull();
    expect(songBpm({ bpm: 135, style: 'piano pop' } as Song)).toBe(135);
    expect(songBpm({ style: 'piano pop, 112 BPM' } as Song)).toBe(112);
    expect(songBpm({ style: 'indie pop' } as Song)).toBeNull();
  });
});
