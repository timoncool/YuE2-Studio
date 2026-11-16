import { describe, expect, it } from 'vitest';
import { yue2 } from './yue2';

describe('YuE2 strings', () => {
  it('every language carries every key, none empty', () => {
    const keys = Object.keys(yue2.en).sort();
    for (const [language, strings] of Object.entries(yue2)) {
      expect(Object.keys(strings).sort(), language).toEqual(keys);
      for (const key of keys) {
        expect((strings as Record<string, string>)[key]?.trim().length, `${language}.${key}`).toBeGreaterThan(0);
      }
    }
  });

  it('HIP auto-device guidance is written in the matching locale', () => {
    expect(yue2.en.computeBackendHint_auto).toContain('HIP');
    expect(yue2.en.computeBackendHint_auto).not.toMatch(/[\u0400-\u04ff\u3400-\u9fff]/);
    expect(yue2.ru.computeBackendHint_auto).toContain('HIP');
    expect(yue2.ru.computeBackendHint_auto).toMatch(/[\u0400-\u04ff]/);
    expect(yue2.ru.computeBackendHint_auto).not.toMatch(/[\u3400-\u9fff]/);
    expect(yue2.zh.computeBackendHint_auto).toMatch(/[\u3400-\u9fff]/);
    for (const strings of Object.values(yue2)) {
      expect(strings.computeBackendHint_hip).toContain('HIP');
    }
  });

  it('no string names the model this studio was forked from', () => {
    for (const [language, strings] of Object.entries(yue2)) {
      for (const [key, value] of Object.entries(strings)) {
        expect(/music ?3|minimax/i.test(value), `${language}.${key}: ${value}`).toBe(false);
      }
    }
  });
});

/** The About page names the sibling studios on purpose: the MiniMax one is one of them. */
const SIBLING_STUDIOS = new Set(['studioMiniMax']);

describe('merged interface strings', () => {
  it('nothing the interface shows names the forked-from model', async () => {
    const { translations } = await import('./translations');
    const leaks: string[] = [];
    for (const [language, strings] of Object.entries(translations)) {
      for (const [key, value] of Object.entries(strings)) {
        if (SIBLING_STUDIOS.has(key)) continue;
        if (typeof value === 'string' && /music ?3|minimax/i.test(value)) leaks.push(`${language}.${key}`);
      }
    }
    expect(leaks).toEqual([]);
  });
});
