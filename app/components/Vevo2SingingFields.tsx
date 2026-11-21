import React from 'react';
import { useI18n } from '../context/I18nContext';
import { useLibrarySongs } from '../services/studioQueries';

export type Vevo2References = { voice_song_id: string; melody_song_id?: string };

export const vevo2SingingStrings = {
  en: { voice: 'Voice reference', melody: 'Melody reference (optional)', choose: 'Choose from the library', none: 'No melody reference', empty: 'Import a voice recording into the library.', required: 'Enter text to sing and choose a voice reference.' },
  ru: { voice: 'Образец голоса', melody: 'Образец мелодии (необязательно)', choose: 'Выберите из библиотеки', none: 'Без образца мелодии', empty: 'Добавьте запись голоса в библиотеку.', required: 'Введите текст для пения и выберите образец голоса.' },
  zh: { voice: '嗓音参考', melody: '旋律参考（可选）', choose: '从资料库选择', none: '无旋律参考', empty: '请将一段人声录音导入资料库。', required: '请输入演唱文本并选择嗓音参考。' },
  ja: { voice: '声の参照音源', melody: 'メロディーの参照音源（任意）', choose: 'ライブラリから選択', none: 'メロディーの参照なし', empty: '声の録音をライブラリにインポートしてください。', required: '歌うテキストを入力して声の参照音源を選択してください。' },
  ko: { voice: '음성 참조', melody: '멜로디 참조(선택)', choose: '라이브러리에서 선택', none: '멜로디 참조 없음', empty: '음성 녹음을 라이브러리로 가져오세요.', required: '노래할 텍스트를 입력하고 음성 참조를 선택하세요.' },
  pt: { voice: 'Referência de voz', melody: 'Referência de melodia (opcional)', choose: 'Escolha na biblioteca', none: 'Sem referência de melodia', empty: 'Importe uma gravação de voz para a biblioteca.', required: 'Digite o texto para cantar e selecione uma referência de voz.' },
};

type Props = {
  value: Vevo2References;
  onChange: (value: Vevo2References) => void;
  controlClassName: string;
};

export function Vevo2SingingFields({ value, onChange, controlClassName }: Props) {
  const { language } = useI18n();
  const strings = vevo2SingingStrings[language];
  const { data, isSuccess } = useLibrarySongs();
  const songs = (data ?? []).filter(song => song.audioUrl && !song.isGenerating);

  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <label className="grid gap-1">
        <span>{strings.voice}</span>
        <select required value={value.voice_song_id} className={controlClassName}
          onChange={event => onChange({ ...value, voice_song_id: event.target.value })}>
          <option value="">{strings.choose}</option>
          {value.voice_song_id && !songs.some(song => song.id === value.voice_song_id) &&
            <option value={value.voice_song_id} disabled>{value.voice_song_id}</option>}
          {songs.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
        </select>
      </label>
      <label className="grid gap-1">
        <span>{strings.melody}</span>
        <select value={value.melody_song_id ?? ''} className={controlClassName}
          onChange={event => onChange({ ...value, melody_song_id: event.target.value || undefined })}>
          <option value="">{strings.none}</option>
          {value.melody_song_id && !songs.some(song => song.id === value.melody_song_id) &&
            <option value={value.melody_song_id} disabled>{value.melody_song_id}</option>}
          {songs.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
        </select>
      </label>
      {isSuccess && !songs.length &&
        <p className="text-xs text-zinc-500 sm:col-span-2">{strings.empty}</p>}
    </div>
  );
}
