import { describe, expect, it } from 'vitest';
import { translations, type Language, type TranslationKey } from './translations';

describe('translations', () => {
  const keys = Object.keys(translations.en) as TranslationKey[];

  it.each(['ru', 'zh', 'ja', 'ko', 'pt'] as Language[])('%s has every English key', language => {
    const missing = keys.filter(key => !translations[language][key]);
    expect(missing).toEqual([]);
  });
});
