import { describe, expect, it } from 'vitest';
import { MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE, PLAYBACK_RATE_PRESETS, semitonesFromPlaybackRate } from './playbackTempo';

describe('shared player and DJ tempo/pitch scale', () => {
  it('offers the same 25%-200% speed range and regular player presets', () => {
    expect(MIN_PLAYBACK_RATE).toBe(0.25);
    expect(MAX_PLAYBACK_RATE).toBe(2);
    expect(PLAYBACK_RATE_PRESETS).toEqual([0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2]);
  });
  it('maps playback speed to pitch in semitones for tape-style playback', () => {
    expect(semitonesFromPlaybackRate(1)).toBe(0);
    expect(semitonesFromPlaybackRate(2)).toBe(12);
    expect(semitonesFromPlaybackRate(0.5)).toBe(-12);
    expect(semitonesFromPlaybackRate(0.25)).toBe(-24);
    expect(semitonesFromPlaybackRate(1.5)).toBeCloseTo(7.01955, 4);
  });
});
