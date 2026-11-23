import React, { useState } from 'react';
import { apiUrl } from '../services/apiBase';
import { useI18n } from '../context/I18nContext';
import { LibrarySongPicker, useFinishedSongs } from './LibrarySongPicker';
import { FieldLabel } from './kit/FieldLabel';
import { SecondaryButton } from './kit/Buttons';
import { FolderOpen } from 'lucide-react';

export type Vevo2References = { voice_song_id: string; melody_song_id?: string };

export const vevo2SingingStrings = {
  en: { voice: 'Voice reference', melody: 'Melody reference (optional)', choose: 'Choose from the library', none: 'No melody reference', empty: 'Import a voice recording into the library.', required: 'Enter text to sing and choose a voice reference.' },
  ru: { voice: 'Образец голоса', melody: 'Образец мелодии (необязательно)', choose: 'Выберите из библиотеки', none: 'Без образца мелодии', empty: 'Добавьте запись голоса в библиотеку.', required: 'Введите текст для пения и выберите образец голоса.' },
  zh: { voice: '嗓音参考', melody: '旋律参考（可选）', choose: '从资料库选择', none: '无旋律参考', empty: '请将一段人声录音导入资料库。', required: '请输入演唱文本并选择嗓音参考。' },
  ja: { voice: '声の参照音源', melody: 'メロディーの参照音源（任意）', choose: 'ライブラリから選択', none: 'メロディーの参照なし', empty: '声の録音をライブラリにインポートしてください。', required: '歌うテキストを入力して声の参照音源を選択してください。' },
  ko: { voice: '음성 참조', melody: '멜로디 참조(선택)', choose: '라이브러리에서 선택', none: '멜로디 참조 없음', empty: '음성 녹음을 라이브러리로 가져오세요.', required: '노래할 텍스트를 입력하고 음성 참조를 선택하세요.' },
  pt: { voice: 'Referência de voz', melody: 'Referência de melodia (opcional)', choose: 'Escolha na biblioteca', none: 'Sem referência de melodia', empty: 'Importe uma gravação de voz para a biblioteca.', required: 'Digite o texto para cantar e selecione uma referência de voz.' },
};

const modelStrings = {
  en: { folder: 'Weights folder', set: 'Choose folder', reset: 'Use the Studio folder', terms: 'Original model terms', have: 'Already have the weights?', failure: 'Could not save the Vevo2 folder.' },
  ru: { folder: 'Папка с весами', set: 'Указать папку', reset: 'Вернуть папку Studio', terms: 'Условия автора модели', have: 'Веса уже скачаны?', failure: 'Не удалось сохранить папку Vevo2.' },
  zh: { folder: '权重文件夹', set: '选择文件夹', reset: '使用 Studio 文件夹', terms: '原始模型使用条款', have: '已有权重？', failure: '无法保存 Vevo2 文件夹。' },
  ja: { folder: '重みのフォルダー', set: 'フォルダーを選ぶ', reset: 'Studio のフォルダーを使う', terms: '元のモデルの利用条件', have: '重みをお持ちですか？', failure: 'Vevo2 フォルダーを保存できませんでした。' },
  ko: { folder: '가중치 폴더', set: '폴더 선택', reset: 'Studio 폴더 사용', terms: '원본 모델 이용 조건', have: '가중치가 이미 있나요?', failure: 'Vevo2 폴더를 저장하지 못했습니다.' },
  pt: { folder: 'Pasta dos pesos', set: 'Escolher pasta', reset: 'Usar a pasta do Studio', terms: 'Termos do modelo original', have: 'Já tem os pesos?', failure: 'Não foi possível salvar a pasta do Vevo2.' },
};

export function Vevo2ModelDirectory({ directory, licenseUrl, onChanged }: {
  directory: string;
  licenseUrl?: string;
  onChanged: () => void;
}) {
  const { language } = useI18n();
  const strings = modelStrings[language];
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  async function save(pick: boolean) {
    setBusy(true);
    setError('');
    try {
      const response = await fetch(apiUrl('/v1/music/audio-cpp/vevo2/model-directory'), {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ directory: null, pick }),
      });
      if (!response.ok) {
        const problem = await response.json().catch(() => null);
        throw new Error(problem?.error || strings.failure);
      }
      onChanged();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : strings.failure);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
    {licenseUrl && <a href={licenseUrl} target="_blank" rel="noreferrer" className="block text-xs underline">{strings.terms}</a>}
    {directory ? <div className="grid gap-1 text-xs">
      <FieldLabel>{strings.folder}</FieldLabel>
      <p className="break-all font-mono text-zinc-700 dark:text-zinc-200">{directory}</p>
      <div className="flex flex-wrap items-center gap-3">
        <SecondaryButton disabled={busy} onClick={() => void save(true)}><FolderOpen size={14} aria-hidden />{strings.set}</SecondaryButton>
        <button type="button" className="underline disabled:opacity-50" disabled={busy} onClick={() => void save(false)}>{strings.reset}</button>
      </div>
    </div> : <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-zinc-600 dark:text-zinc-300">
      <span>{strings.have}</span>
      <SecondaryButton disabled={busy} onClick={() => void save(true)}><FolderOpen size={14} aria-hidden />{strings.set}</SecondaryButton>
    </div>}
    {error && <p role="alert" className="text-xs text-red-600 dark:text-red-400">{error}</p>}
    </>
  );
}

type Props = {
  value: Vevo2References;
  onChange: (value: Vevo2References) => void;
  controlClassName: string;
};

export function Vevo2SingingFields({ value, onChange, controlClassName }: Props) {
  const { language } = useI18n();
  const strings = vevo2SingingStrings[language];
  const { songs, loaded } = useFinishedSongs();

  return (
    <div className="grid gap-3">
      <LibrarySongPicker label={strings.voice} value={value.voice_song_id} placeholder={strings.choose} controlClassName={controlClassName}
        onChange={id => onChange({ ...value, voice_song_id: id })} />
      <LibrarySongPicker label={strings.melody} value={value.melody_song_id ?? ''} placeholder={strings.none} none={strings.none}
        controlClassName={controlClassName} onChange={id => onChange({ ...value, melody_song_id: id || undefined })} />
      {loaded && !songs.length &&
        <p className="text-xs text-zinc-500 dark:text-zinc-400">{strings.empty}</p>}
    </div>
  );
}
