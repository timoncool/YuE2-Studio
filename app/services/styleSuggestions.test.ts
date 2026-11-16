import { describe, expect, it } from 'vitest';
import { MAIN_STYLES } from '../data/genres';
import { GENRE_GLOSSES, STYLE_GLOSSES, STYLE_SUGGESTIONS, genreGloss, styleSuggestions } from './styleSuggestions';

describe('style suggestions', () => {
  it('are style phrases, not tempos, weights, sentence fragments or the genres', () => {
    expect(STYLE_SUGGESTIONS.length).toBeGreaterThanOrEqual(398);
    expect(new Set(STYLE_SUGGESTIONS.map(phrase => phrase.toLowerCase())).size).toBe(STYLE_SUGGESTIONS.length);
    const genres = new Set(MAIN_STYLES.map(genre => genre.toLowerCase()));
    for (const phrase of STYLE_SUGGESTIONS) {
      expect(phrase).not.toMatch(/\bbpm\b|[():]/i);
      expect(phrase).not.toMatch(/^(a|an|and|with) /i);
      expect(phrase.split(' ').length).toBeLessThanOrEqual(5);
      expect(genres.has(phrase.toLowerCase()), phrase).toBe(false);
    }
  });

  it('gloss every phrase of the list in each language that has a file, and nothing else', () => {
    expect(Object.keys(STYLE_GLOSSES).sort()).toEqual(['ja', 'ko', 'pt', 'ru', 'zh']);
    for (const [language, glosses] of Object.entries(STYLE_GLOSSES)) {
      expect([...glosses!.keys()], language).toEqual(STYLE_SUGGESTIONS);
      for (const [phrase, gloss] of glosses!) expect(gloss, `${language}: ${phrase}`).not.toBe('');
    }
  });

  it('gloss every genre of the studio in each language that has a file, and nothing else', () => {
    expect(Object.keys(GENRE_GLOSSES).sort()).toEqual(['ja', 'ko', 'pt', 'ru', 'zh']);
    for (const [language, glosses] of Object.entries(GENRE_GLOSSES)) {
      expect([...glosses!.keys()], language).toEqual(MAIN_STYLES);
      for (const [genre, gloss] of glosses!) expect(gloss, `${language}: ${genre}`).not.toBe('');
    }
  });

  it('show the gloss beside the English the model reads, the genres first', () => {
    expect(genreGloss('dubstep', 'zh')).toBe('回响贝斯');
    expect(genreGloss('dubstep', 'en')).toBeUndefined();
    const zh = styleSuggestions('zh', MAIN_STYLES);
    expect(zh).toHaveLength(MAIN_STYLES.length + STYLE_SUGGESTIONS.length);
    expect(zh[0]).toEqual({ value: MAIN_STYLES[0], label: genreGloss(MAIN_STYLES[0], 'zh') });
    expect(zh.find(item => item.value === 'male vocals')).toEqual({ value: 'male vocals', label: '男声演唱' });
    expect(styleSuggestions('ru', MAIN_STYLES).find(item => item.value === 'dubstep')).toEqual({ value: 'dubstep', label: 'дабстеп' });
    expect(styleSuggestions('en', MAIN_STYLES).every(item => item.label === undefined)).toBe(true);
  });
});
