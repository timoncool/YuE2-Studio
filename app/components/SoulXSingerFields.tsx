import React, { useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { useLibrarySongs } from '../services/studioQueries';

export type SoulXInputs = {
  reference_song_id: string;
  source_song_id?: string;
  reference_metadata?: string;
  target_metadata?: string;
  control?: 'score' | 'melody';
};

const strings = {
  en: ['Voice reference', 'Source vocals', 'Choose from the library', 'Reference metadata JSON', 'Target metadata JSON', 'Melody control', 'Notes', 'Pitch contour', 'Use an aligned vocals JSON file up to 256 KiB.', 'JSON attached', 'Choose the reference and supply the inputs for this mode.'],
  ru: ['Образец голоса', 'Исходный вокал', 'Выберите из библиотеки', 'Разметка образца JSON', 'Целевая разметка JSON', 'Управление мелодией', 'Ноты', 'Контур высоты тона', 'Загрузите JSON с разметкой вокала размером до 256 КиБ.', 'JSON прикреплён', 'Выберите образец голоса и укажите входные данные для этого режима.'],
  zh: ['嗓音参考', '原始人声', '从资料库选择', '参考音频标注 JSON', '目标音频标注 JSON', '旋律控制', '音符', '音高轮廓', '请上传不超过 256 KiB 的对齐演唱标注 JSON。', '已添加 JSON', '请选择嗓音参考并提供此模式的输入。'],
  ja: ['声の参照音源', '元のボーカル', 'ライブラリから選択', '参照音源のメタデータ JSON', '対象のメタデータ JSON', 'メロディー制御', '音符', 'ピッチ輪郭', '256 KiB 以下の歌唱アラインメント JSON を指定してください。', 'JSON を添付済み', '参照音源を選択し、このモードに必要な入力を指定してください。'],
  ko: ['음성 참조', '원본 보컬', '라이브러리에서 선택', '참조 메타데이터 JSON', '대상 메타데이터 JSON', '멜로디 제어', '음표', '음높이 윤곽', '256 KiB 이하의 정렬된 보컬 JSON 파일을 선택하세요.', 'JSON 첨부됨', '음성 참조를 선택하고 이 모드에 필요한 입력을 제공하세요.'],
  pt: ['Referência de voz', 'Voz original', 'Escolha na biblioteca', 'JSON de referência', 'JSON de destino', 'Controle de melodia', 'Notas', 'Contorno de altura', 'Use um JSON de vocais alinhados de até 256 KiB.', 'JSON anexado', 'Escolha a referência e forneça as entradas para este modo.'],
} as const;

export function soulxRequired(language: keyof typeof strings) {
  return strings[language][10];
}

type Props = {
  conversion: boolean;
  value: SoulXInputs;
  onChange: React.Dispatch<React.SetStateAction<SoulXInputs>>;
  controlClassName: string;
};

export function SoulXSingerFields({ conversion, value, onChange, controlClassName }: Props) {
  const { language } = useI18n();
  const [referenceLabel, sourceLabel, choose, refJson, targetJson, control, score, melody, format, attached] = strings[language];
  const { data } = useLibrarySongs();
  const songs = (data ?? []).filter(song => song.audioUrl && !song.isGenerating);
  const [invalid, setInvalid] = useState<Partial<Record<'reference_metadata' | 'target_metadata', boolean>>>({});

  function recording(label: string, field: 'reference_song_id' | 'source_song_id') {
    const selected = value[field] ?? '';
    return <label className="grid gap-1">
      <span>{label}</span>
      <select value={selected} className={controlClassName}
        onChange={event => { const id = event.target.value; onChange(current => ({ ...current, [field]: id })); }}>
        <option value="">{choose}</option>
        {selected && !songs.some(song => song.id === selected) && <option disabled value={selected}>{selected}</option>}
        {songs.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
      </select>
    </label>;
  }

  function metadata(label: string, field: 'reference_metadata' | 'target_metadata') {
    return <label className="grid gap-1">
      <span>{label}</span>
      <input type="file" accept=".json,application/json" className={controlClassName}
        onChange={async event => {
          const file = event.target.files?.[0];
          if (!file) return;
          try {
            if (file.size > 256 * 1024) throw new Error();
            const text = await file.text();
            const parsed: unknown = JSON.parse(text);
            if (!Array.isArray(parsed) || !parsed.length || parsed.some(row => !row || typeof row !== 'object' || Array.isArray(row))) throw new Error();
            setInvalid(current => ({ ...current, [field]: false }));
            onChange(current => ({ ...current, [field]: text }));
          } catch {
            setInvalid(current => ({ ...current, [field]: true }));
            onChange(current => ({ ...current, [field]: undefined }));
          }
        }} />
      {invalid[field] ? <span role="alert" className="text-xs">{format}</span> : value[field] && <span className="text-xs">{attached}</span>}
    </label>;
  }

  return <div className="grid gap-3 sm:grid-cols-2">
    {recording(referenceLabel, 'reference_song_id')}
    {conversion ? recording(sourceLabel, 'source_song_id') : <>
      <label className="grid gap-1">
        <span>{control}</span>
        <select className={controlClassName} value={value.control ?? 'score'}
          onChange={event => { const mode = event.target.value === 'melody' ? 'melody' : 'score'; onChange(current => ({ ...current, control: mode })); }}>
          <option value="score">{score}</option>
          <option value="melody">{melody}</option>
        </select>
      </label>
      {metadata(refJson, 'reference_metadata')}
      {metadata(targetJson, 'target_metadata')}
    </>}
  </div>;
}
