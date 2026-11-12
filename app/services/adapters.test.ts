import { describe, expect, it } from 'vitest';
import { SCORE_STRENGTH_LIMIT, scoreStrength, usesFromSettings } from './adapters';

describe('restoring adapter strengths', () => {
  it('preserves zero slots and signed strengths from engine and prompt requests', () => {
    expect(usesFromSettings({ adapters: [{ name: 'voice', ar_scale: 0, nar_scale: -10 }] }))
      .toEqual([{ id: 'voice', scales: { ar: 0, nar: -10 } }]);
    expect(usesFromSettings({ adapters: [{ id: 'voice', scales: { ar: 10, nar: 0 } }] }))
      .toEqual([{ id: 'voice', scales: { ar: 10, nar: 0 } }]);
  });
});

describe('the Composition strength the form warns about', () => {
  const slots = [{ id: 'ar', role: 'composition' }, { id: 'nar', role: 'sound' }];

  it('sums the composition slot of the LoRA in use, not the sound one or a switched-off LoRA', () => {
    const uses = [
      { id: 'folk', scales: { ar: 1, nar: 3 } },
      { id: 'metal', scales: { ar: 1, nar: 0 } },
      { id: 'off', scales: { ar: 5, nar: 0 }, disabled: true },
    ];
    expect(scoreStrength(uses, slots)).toBe(2);
  });

  it('warns past 1.2 and not at it', () => {
    expect(SCORE_STRENGTH_LIMIT).toBe(1.2);
    expect(scoreStrength([{ id: 'folk', scales: { ar: 1.2 } }], slots) > SCORE_STRENGTH_LIMIT).toBe(false);
    expect(scoreStrength([{ id: 'folk', scales: { ar: 1.25 } }], slots) > SCORE_STRENGTH_LIMIT).toBe(true);
    expect(scoreStrength([], slots)).toBe(0);
  });
});
