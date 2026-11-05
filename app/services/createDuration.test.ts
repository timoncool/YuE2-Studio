import { describe, expect, it } from 'vitest';
import { clampDurationToKept } from './createDuration';

describe('clampDurationToKept (#184)', () => {
  it('keeps the saved maximum when a loaded request is shorter or absent', () => {
    expect(clampDurationToKept(180, 600)).toBe(600);
    expect(clampDurationToKept(undefined, 600)).toBe(600);
    expect(clampDurationToKept(Number.NaN, 600)).toBe(600);
    expect(clampDurationToKept(600, 600)).toBe(600);
  });
  it('honours a loaded request that is longer than the saved maximum', () => {
    expect(clampDurationToKept(300, 130)).toBe(300);
    expect(clampDurationToKept(240.6, 130)).toBe(241);
  });
});
