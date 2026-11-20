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

  it('no string names the model this studio was forked from', () => {
    for (const [language, strings] of Object.entries(yue2)) {
      for (const [key, value] of Object.entries(strings)) {
        expect(/music ?3|minimax/i.test(value), `${language}.${key}: ${value}`).toBe(false);
      }
    }
  });
});

/** About and library migration identify the MiniMax Studio installation by name. */
const SIBLING_STUDIOS = new Set(['studioMiniMax', 'legacyLibraryTitle']);

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
