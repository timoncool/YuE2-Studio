import type { Language } from './translations';

/** Titles and shared words of the create-form cards that several models use. */
export const formCardStrings: Record<Language, { voiceMelody: string; scene: string; chooseJson: string; missingSong: string; noBacking: string }> = {
  en: { voiceMelody: 'Voice and melody', scene: 'Sound scene', chooseJson: 'Choose JSON', missingSong: 'No longer in the library', noBacking: 'No backing track' },
  ru: { voiceMelody: 'Голос и мелодия', scene: 'Звуковая сцена', chooseJson: 'Выбрать JSON', missingSong: 'Песни больше нет в медиатеке', noBacking: 'Без аккомпанемента' },
  zh: { voiceMelody: '嗓音与旋律', scene: '声音场景', chooseJson: '选择 JSON', missingSong: '资料库中已没有这首歌', noBacking: '不用伴奏' },
  ja: { voiceMelody: '声とメロディー', scene: 'サウンドシーン', chooseJson: 'JSON を選択', missingSong: 'ライブラリにもうありません', noBacking: '伴奏なし' },
  ko: { voiceMelody: '목소리와 멜로디', scene: '사운드 장면', chooseJson: 'JSON 선택', missingSong: '라이브러리에 더 이상 없음', noBacking: '반주 없음' },
  pt: { voiceMelody: 'Voz e melodia', scene: 'Cena sonora', chooseJson: 'Escolher JSON', missingSong: 'Não está mais na biblioteca', noBacking: 'Sem acompanhamento' },
};
