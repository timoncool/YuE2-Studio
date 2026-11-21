import { describe, expect, it } from 'vitest';
import { AUTO_DURATION, clampDurationToKept, durationAfterLoad, isAutoDuration, keptDurationText, scoreSeconds } from './createDuration';

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

it('estimates the written length through trailing and multimeasure rests on parallel voices', () => {
  const header = 'X:1\nM:4/4\nL:1/16\nQ:1/4=96\nV:Vocal\nV:Ins\nK:Em\n';
  expect(scoreSeconds(header + 'V:Vocal\nZ4|\nV:Ins\nC16|z16|z16|z16|')).toBe(10);
  expect(scoreSeconds(header + 'V:Vocal\nZ84|\nV:Ins\n' + 'z16|'.repeat(84))).toBe(210);
  expect(scoreSeconds('')).toBeUndefined();
  expect(scoreSeconds('not a score')).toBeUndefined();
});

describe('Auto max duration', () => {
  it('is what a new studio starts with, and what Auto keeps', () => {
    expect(keptDurationText(null, 10, 600)).toBe(AUTO_DURATION);
    expect(keptDurationText('auto', 10, 600)).toBe(AUTO_DURATION);
    expect(keptDurationText(' Auto ', 10, 600)).toBe(AUTO_DURATION);
  });
  it('keeps seconds chosen by hand, and falls back to Auto for a value out of range', () => {
    expect(keptDurationText('240', 10, 600)).toBe('240');
    expect(keptDurationText('5', 10, 600)).toBe(AUTO_DURATION);
    expect(keptDurationText('nonsense', 10, 600)).toBe(AUTO_DURATION);
  });
  it('stays Auto when a request or the assistant brings a duration of its own', () => {
    expect(durationAfterLoad(220, AUTO_DURATION, 600)).toBe(AUTO_DURATION);
    expect(durationAfterLoad(undefined, AUTO_DURATION, 600)).toBe(AUTO_DURATION);
  });
  it('follows clampDurationToKept for seconds, under the form maximum', () => {
    expect(durationAfterLoad(300, '130', 600)).toBe('300');
    expect(durationAfterLoad(120, '130', 600)).toBe('130');
    expect(durationAfterLoad(900, '130', 600)).toBe('600');
  });
  it('tells Auto from seconds', () => {
    expect(isAutoDuration('auto')).toBe(true);
    expect(isAutoDuration('130')).toBe(false);
    expect(isAutoDuration('')).toBe(false);
  });
});
