import { describe, expect, it } from 'vitest';
import { clampLoop, drifted, freshChannel, heardGain, loopJump, panLabel, volumeDb } from './stemMix';

describe('stem mix', () => {
  it('mutes, solos and limits gain', () => {
    const drums = { ...freshChannel(), volume: 0.5 };
    const bass = { ...freshChannel(), solo: true };
    const vocals = { ...freshChannel(), solo: true, mute: true };
    const all = [drums, bass, vocals];
    expect(heardGain(drums, all)).toBe(0);
    expect(heardGain(bass, all)).toBe(1);
    expect(heardGain(vocals, all)).toBe(0);
    expect(heardGain(drums, [drums])).toBe(0.5);
    expect(heardGain({ ...drums, volume: 9 }, [drums])).toBe(2);
  });

  it('keeps a loop inside the track and long enough to follow', () => {
    expect(clampLoop({ on: true, start: -3, end: 500 }, 120)).toEqual({ on: true, start: 0, end: 120 });
    expect(clampLoop({ on: true, start: 10, end: 10 }, 120)).toEqual({ on: true, start: 10, end: 10.25 });
    expect(clampLoop({ on: true, start: 200, end: 210 }, 120)?.start).toBeCloseTo(119.75);
    expect(clampLoop({ on: true, start: 0, end: 1 }, Number.NaN)).toBeNull();
  });

  it('jumps back at the loop end and only then', () => {
    const loop = { on: true, start: 8, end: 16 };
    expect(loopJump(12, loop)).toBeNull();
    expect(loopJump(16, loop)).toBe(8);
    expect(loopJump(30, loop)).toBe(8);
    expect(loopJump(2, loop)).toBe(8);
    expect(loopJump(30, { ...loop, on: false })).toBeNull();
  });

  it('notices drift and labels the controls', () => {
    expect(drifted(10.02, 10)).toBe(false);
    expect(drifted(10.05, 10)).toBe(true);
    expect(volumeDb(1)).toBe('+0.0 dB');
    expect(volumeDb(0.5)).toBe('-6.0 dB');
    expect(volumeDb(0)).toBe('-∞ dB');
    expect(panLabel(0)).toBe('C');
    expect(panLabel(-0.3)).toBe('L30');
    expect(panLabel(1)).toBe('R100');
  });
});
