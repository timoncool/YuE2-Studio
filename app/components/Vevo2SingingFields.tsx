import React, { useState } from 'react';
import { apiUrl } from '../services/apiBase';
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

const modelStrings = {
  en: { folder: 'Folder containing your Vevo2 weights', set: 'Use this folder', reset: 'Use the Studio folder', defaultFolder: 'Default relative to the Studio data folder', terms: 'Original model terms', failure: 'Could not save the Vevo2 folder.' },
  ru: { folder: 'Папка с вашими весами Vevo2', set: 'Использовать эту папку', reset: 'Использовать папку Studio', defaultFolder: 'Путь по умолчанию относительно папки данных Studio', terms: 'Условия автора модели', failure: 'Не удалось сохранить папку Vevo2.' },
  zh: { folder: '已有 Vevo2 权重所在的文件夹', set: '使用此文件夹', reset: '使用 Studio 文件夹', defaultFolder: '相对于 Studio 数据目录的默认路径', terms: '原始模型使用条款', failure: '无法保存 Vevo2 文件夹。' },
  ja: { folder: '既存の Vevo2 重みがあるフォルダー', set: 'このフォルダーを使う', reset: 'Studio のフォルダーを使う', defaultFolder: 'Studio データフォルダーからの既定の相対パス', terms: '元のモデルの利用条件', failure: 'Vevo2 フォルダーを保存できませんでした。' },
  ko: { folder: '기존 Vevo2 가중치가 있는 폴더', set: '이 폴더 사용', reset: 'Studio 폴더 사용', defaultFolder: 'Studio 데이터 폴더 기준 기본 경로', terms: '원본 모델 이용 조건', failure: 'Vevo2 폴더를 저장하지 못했습니다.' },
  pt: { folder: 'Pasta com seus pesos do Vevo2', set: 'Usar esta pasta', reset: 'Usar a pasta do Studio', defaultFolder: 'Caminho padrão relativo à pasta de dados do Studio', terms: 'Termos do modelo original', failure: 'Não foi possível salvar a pasta do Vevo2.' },
};

export function Vevo2ModelDirectory({ directory, defaultDirectory, licenseUrl, controlClassName, onChanged }: {
  directory: string;
  defaultDirectory: string;
  licenseUrl?: string;
  controlClassName: string;
  onChanged: () => void;
}) {
  const { language } = useI18n();
  const strings = modelStrings[language];
  const [draft, setDraft] = useState(directory);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  async function save(next: string | null) {
    setBusy(true);
    setError('');
    try {
      const response = await fetch(apiUrl('/v1/music/audio-cpp/vevo2/model-directory'), {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ directory: next }),
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
    <div className="space-y-2 text-xs">
      <label className="grid gap-1">
        <span>{strings.folder}</span>
        <input className={controlClassName} value={draft} disabled={busy}
          onChange={event => setDraft(event.target.value)} />
      </label>
      <p><code>vevo2-q8_0.gguf</code></p>
      <p>{strings.defaultFolder} <code>{defaultDirectory}</code></p>
      {licenseUrl && <a href={licenseUrl} target="_blank" rel="noreferrer" className="block underline">{strings.terms}</a>}
      <div className="flex flex-wrap gap-3">
        <button type="button" className="underline disabled:opacity-50" disabled={busy || !draft.trim() || draft.trim() === directory}
          onClick={() => void save(draft.trim())}>{strings.set}</button>
        {directory && <button type="button" className="underline disabled:opacity-50" disabled={busy}
          onClick={() => void save(null)}>{strings.reset}</button>}
      </div>
      {error && <p role="alert">{error}</p>}
    </div>
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
