import { en } from './en';
import { zh } from './zh';
import { ja } from './ja';
import { ko } from './ko';
import { ru } from './ru';
import { pt } from './pt';
import { yue2 } from './yue2';
import { adapterStrings } from './adapters';
import { processingStrings } from './processing';
import { trainingStrings } from './training';
import { hubStrings } from './hub';
import { scoreStrings } from './score';
import { midiEditorStrings } from './midiEditor';
import { noticeStrings } from './notices';
import { shellStrings } from './shell';
import { exportStrings } from './exportDialog';
import { stemMixerStrings } from './stemMixer';
import { modelStorageStrings } from './modelStorage';

export type Language = 'en' | 'zh' | 'ja' | 'ko' | 'ru' | 'pt';

/** Each window language by its own name, the same in every language, so a reader finds theirs in any window. */
export const LANGUAGE_NAMES: Record<Language, string> = {
  ru: 'Русский',
  en: 'English',
  zh: '简体中文',
  ja: '日本語',
  ko: '한국어',
  pt: 'Português',
};

const enAll = { ...en, ...yue2.en, ...adapterStrings.en, ...processingStrings.en, ...trainingStrings.en, ...hubStrings.en, ...scoreStrings.en, ...midiEditorStrings.en, ...noticeStrings.en, ...shellStrings.en, ...exportStrings.en, ...stemMixerStrings.en, ...modelStorageStrings.en };

export type TranslationKey = keyof typeof enAll;

export const translations: Record<Language, Partial<Record<TranslationKey, string>>> = {
  en: enAll,
  zh: { ...zh, ...yue2.zh, ...adapterStrings.zh, ...processingStrings.zh, ...trainingStrings.zh, ...hubStrings.zh, ...scoreStrings.zh, ...midiEditorStrings.zh, ...noticeStrings.zh, ...shellStrings.zh, ...exportStrings.zh, ...stemMixerStrings.zh, ...modelStorageStrings.zh },
  ja: { ...ja, ...yue2.ja, ...adapterStrings.ja, ...processingStrings.ja, ...trainingStrings.ja, ...hubStrings.ja, ...scoreStrings.ja, ...midiEditorStrings.ja, ...noticeStrings.ja, ...shellStrings.ja, ...exportStrings.ja, ...stemMixerStrings.ja, ...modelStorageStrings.ja },
  ko: { ...ko, ...yue2.ko, ...adapterStrings.ko, ...processingStrings.ko, ...trainingStrings.ko, ...hubStrings.ko, ...scoreStrings.ko, ...midiEditorStrings.ko, ...noticeStrings.ko, ...shellStrings.ko, ...exportStrings.ko, ...stemMixerStrings.ko, ...modelStorageStrings.ko },
  ru: { ...ru, ...yue2.ru, ...adapterStrings.ru, ...processingStrings.ru, ...trainingStrings.ru, ...hubStrings.ru, ...scoreStrings.ru, ...midiEditorStrings.ru, ...noticeStrings.ru, ...shellStrings.ru, ...exportStrings.ru, ...stemMixerStrings.ru, ...modelStorageStrings.ru },
  // one file for the whole language: what it lacks is read in English, never as a bare key
  pt: { ...enAll, ...pt, ...exportStrings.pt, ...stemMixerStrings.pt, ...modelStorageStrings.pt },
};
