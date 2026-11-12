import { describe, expect, it } from 'vitest';
import { stepsDone } from './training';

describe('the bar of a training run between whole steps', () => {
  it('moves by the parts of the step going once a whole step told how many there are', () => {
    expect(stepsDone(4, { step: 5, stage: 'NAR forward', index: 3, of: 4, step_elapsed_ms: 900 })).toBe(4.75);
  });

  it('stays on the whole steps while the parts of a step are not counted yet', () => {
    expect(stepsDone(undefined, { step: 1, stage: 'AR adapted forward', index: 1, of: null, step_elapsed_ms: 0 })).toBe(0);
    expect(stepsDone(7, null)).toBe(7);
  });
});
