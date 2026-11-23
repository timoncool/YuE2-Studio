import type { Language } from './translations';

/** Titles of the create-form cards that several models share. */
export const formCardStrings: Record<Language, { voiceMelody: string; scene: string; chooseJson: string }> = {
  en: { voiceMelody: 'Voice and melody', scene: 'Sound scene', chooseJson: 'Choose JSON' },
  ru: { voiceMelody: 'Голос и мелодия', scene: 'Звуковая сцена', chooseJson: 'Выбрать JSON' },
  zh: { voiceMelody: '嗓音与旋律', scene: '声音场景', chooseJson: '选择 JSON' },
  ja: { voiceMelody: '声とメロディー', scene: 'サウンドシーン', chooseJson: 'JSON を選択' },
  ko: { voiceMelody: '목소리와 멜로디', scene: '사운드 장면', chooseJson: 'JSON 선택' },
  pt: { voiceMelody: 'Voz e melodia', scene: 'Cena sonora', chooseJson: 'Escolher JSON' },
};
