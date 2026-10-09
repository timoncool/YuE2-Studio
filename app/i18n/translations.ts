import { en } from './en';
import { zh } from './zh';
import { ja } from './ja';
import { ko } from './ko';
import { ru } from './ru';
import { yue2 } from './yue2';
import { adapterStrings } from './adapters';
import { processingStrings } from './processing';
import { trainingStrings } from './training';
import { hubStrings } from './hub';
import { scoreStrings } from './score';
import { midiEditorStrings } from './midiEditor';
import { noticeStrings } from './notices';

export type Language = 'en' | 'zh' | 'ja' | 'ko' | 'ru';

const enAll = { ...en, ...yue2.en, ...adapterStrings.en, ...processingStrings.en, ...trainingStrings.en, ...hubStrings.en, ...scoreStrings.en, ...midiEditorStrings.en, ...noticeStrings.en };

export type TranslationKey = keyof typeof enAll;

export const translations: Record<Language, Partial<Record<TranslationKey, string>>> = {
  en: enAll,
  zh: { ...zh, ...yue2.zh, ...adapterStrings.zh, ...processingStrings.zh, ...trainingStrings.zh, ...hubStrings.zh, ...scoreStrings.zh, ...midiEditorStrings.zh, ...noticeStrings.zh },
  ja: { ...ja, ...yue2.ja, ...adapterStrings.ja, ...processingStrings.ja, ...trainingStrings.ja, ...hubStrings.ja, ...scoreStrings.ja, ...midiEditorStrings.ja, ...noticeStrings.ja },
  ko: { ...ko, ...yue2.ko, ...adapterStrings.ko, ...processingStrings.ko, ...trainingStrings.ko, ...hubStrings.ko, ...scoreStrings.ko, ...midiEditorStrings.ko, ...noticeStrings.ko },
  ru: { ...ru, ...yue2.ru, ...adapterStrings.ru, ...processingStrings.ru, ...trainingStrings.ru, ...hubStrings.ru, ...scoreStrings.ru, ...midiEditorStrings.ru, ...noticeStrings.ru },
};
