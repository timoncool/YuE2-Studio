import type { Language } from './translations';

/** The headings that group a song's menu: what changes its sound, what turns it into notes, what makes it again. */
export const songMenuStrings: Record<Language, { sound: string; notes: string; again: string; score: string }> = {
  en: { sound: 'Change the sound', notes: 'Notes', again: 'Make again', score: 'Score' },
  ru: { sound: 'Изменить звук', notes: 'Ноты', again: 'Ещё раз', score: 'Партитура' },
  zh: { sound: '修改声音', notes: '乐谱', again: '再来一次', score: '乐谱' },
  ja: { sound: '音を変える', notes: '楽譜', again: 'もう一度', score: '楽譜' },
  ko: { sound: '소리 바꾸기', notes: '악보', again: '다시 만들기', score: '악보' },
  pt: { sound: 'Mudar o som', notes: 'Partitura', again: 'Fazer de novo', score: 'Partitura' },
};
