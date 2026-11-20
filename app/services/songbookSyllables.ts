import { hyphenateSync as english } from 'hyphen/en';
import { hyphenateSync as portuguese } from 'hyphen/pt';
import { hyphenateSync as russian } from 'hyphen/ru';
import type { Language } from '../i18n/translations';

const dictionaries = { en: english, pt: portuguese, ru: russian };
const kanaContinuation = /^[ぁぃぅぇぉゃゅょゎァィゥェォャュョヮー]$/u;

function splitWord(word: string, language: Language): string[] | null {
  if (/^\p{Script=Han}+$/u.test(word)) {
    return language === 'ja' ? null : Array.from(word);
  }
  if (/^[\p{Script=Hiragana}\p{Script=Katakana}ー]+$/u.test(word)) {
    const syllables: string[] = [];
    for (const kana of word) {
      if (syllables.length && kanaContinuation.test(kana)) syllables[syllables.length - 1] += kana;
      else syllables.push(kana);
    }
    return syllables;
  }
  if (/^\p{Script=Hangul}+$/u.test(word)) {
    return Array.from(new Intl.Segmenter('ko', { granularity: 'grapheme' }).segment(word.normalize('NFC')),
      item => item.segment);
  }
  // Kanji readings and their number of sung morae depend on the word.
  if (/\p{Script=Han}/u.test(word)) return null;
  const dictionary = /\p{Script=Cyrillic}/u.test(word) ? russian : language === 'pt' ? portuguese : dictionaries.en;
  return dictionary(word).split('\u00ad');
}

/** Estimates syllable boundaries for plain lyrics. The songbook will use
 *  these only if there is exactly one syllable per available note attack. */
export function estimateLyricSyllables(lyrics: string, language: Language): string[] | null {
  const segmenter = new Intl.Segmenter(language, { granularity: 'word' });
  const result: string[] = [];
  for (const line of lyrics.split(/\r?\n/)) {
    if (/^\s*\[.*\]\s*$/.test(line)) continue;
    for (const word of segmenter.segment(line)) {
      if (!word.isWordLike) {
        if (result.length && word.segment.trim()) result[result.length - 1] += word.segment;
        continue;
      }
      const syllables = splitWord(word.segment.normalize('NFC'), language);
      if (!syllables) return null;
      const wide = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u.test(word.segment);
      result.push(...syllables.map((part, index) => !wide && index < syllables.length - 1 ? part + '-' : part));
    }
  }
  return result;
}
