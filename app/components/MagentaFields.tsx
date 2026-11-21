import React from 'react';
import { useI18n } from '../context/I18nContext';

export const magentaModel = 'magenta_realtime_2';
export type MagentaOptions = { reference_song_id?: string };
export type MagentaReferenceTrack = { id: string; title: string };

export function restoreMagentaOptions(saved: unknown): MagentaOptions {
  if (saved && typeof saved === 'object' && 'reference_song_id' in saved
      && typeof saved.reference_song_id === 'string' && saved.reference_song_id.trim()) {
    return { reference_song_id: saved.reference_song_id };
  }
  return {};
}

const labels = {
  en: ['Style reference', 'Use text style', 'Reference unavailable', 'Use the selected track as the musical style.'],
  ru: ['Звуковой референс стиля', 'Использовать описание стиля', 'Референс недоступен', 'Использовать стиль выбранного трека.'],
  zh: ['风格参考', '使用文字描述风格', '参考音频不可用', '将所选曲目用作风格参考。'],
  ja: ['スタイル参照音源', 'テキストでスタイルを指定', '参照音源を利用できません', '選んだ曲のスタイルを使います。'],
  ko: ['스타일 참조 오디오', '텍스트 스타일 사용', '참조 오디오를 사용할 수 없음', '선택한 곡의 스타일을 사용합니다.'],
  pt: ['Referência de estilo', 'Usar descrição de estilo', 'Referência indisponível', 'Usar o estilo da faixa selecionada.'],
};

export function MagentaFields({ value, onChange, tracks, controlClassName }: {
  value: MagentaOptions;
  onChange: (value: MagentaOptions) => void;
  tracks: readonly MagentaReferenceTrack[];
  controlClassName?: string;
}) {
  const { language } = useI18n();
  const [label, textMode, unavailable, audioMode] = labels[language] ?? labels.en;
  const reference = value.reference_song_id ?? '';
  const missing = Boolean(reference) && !tracks.some(track => track.id === reference);

  return <label className="grid gap-1 text-xs text-zinc-700 dark:text-zinc-300">
    <span>{label}</span>
    <select className={controlClassName} value={reference}
      onChange={event => onChange(event.target.value ? { reference_song_id: event.target.value } : {})}>
      <option value="">{textMode}</option>
      {missing && <option value={reference} disabled>{unavailable}</option>}
      {tracks.map(track => <option key={track.id} value={track.id}>{track.title}</option>)}
    </select>
    {reference && !missing && <span className="text-zinc-500">{audioMode}</span>}
  </label>;
}
