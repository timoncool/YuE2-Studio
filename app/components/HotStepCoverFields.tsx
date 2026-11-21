import React, { useEffect, useRef, useState } from 'react';
import type { Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { useLibrarySongs } from '../services/studioQueries';
import { transcribe } from '../services/transcription';

export type HotStepCoverDraft = { original: string; abc: string };

const labels = {
  en: { original: 'Original song', choose: 'Choose from the library', transcribe: 'Hear melody and chords', listening: 'Transcribing…', score: 'Cover score (ABC)', install: 'Install HOT-Step and cover weights', installing: 'Installing HOT-Step…', error: 'HOT-Step installation is incomplete.', license: 'Noncommercial model and output terms' },
  ru: { original: 'Исходная песня', choose: 'Выберите из библиотеки', transcribe: 'Определить мелодию и аккорды', listening: 'Распознавание…', score: 'Партитура кавера (ABC)', install: 'Установить HOT-Step и веса для каверов', installing: 'Устанавливается HOT-Step…', error: 'Установка HOT-Step не завершена.', license: 'Условия некоммерческого использования модели и результатов' },
  zh: { original: '原曲', choose: '从资料库选择', transcribe: '识别旋律和和弦', listening: '正在识别…', score: '翻唱乐谱 (ABC)', install: '安装 HOT-Step 和翻唱权重', installing: '正在安装 HOT-Step…', error: 'HOT-Step 安装尚未完成。', license: '模型及输出的非商业使用条款' },
  ja: { original: '原曲', choose: 'ライブラリから選択', transcribe: 'メロディーとコードを採譜', listening: '採譜中…', score: 'カバー譜 (ABC)', install: 'HOT-Step とカバー用重みをインストール', installing: 'HOT-Step をインストール中…', error: 'HOT-Step のインストールが完了していません。', license: 'モデルと出力の非商用利用条件' },
  ko: { original: '원곡', choose: '라이브러리에서 선택', transcribe: '멜로디와 코드 채보', listening: '채보 중…', score: '커버 악보 (ABC)', install: 'HOT-Step과 커버 가중치 설치', installing: 'HOT-Step 설치 중…', error: 'HOT-Step 설치가 완료되지 않았습니다.', license: '모델과 출력의 비상업적 사용 조건' },
  pt: { original: 'Canção original', choose: 'Escolha na biblioteca', transcribe: 'Transcrever melodia e acordes', listening: 'Transcrevendo…', score: 'Partitura do cover (ABC)', install: 'Instalar HOT-Step e os pesos de cover', installing: 'Instalando o HOT-Step…', error: 'A instalação do HOT-Step está incompleta.', license: 'Termos de uso não comercial do modelo e das saídas' },
};

export function HotStepCoverFields({ draft, onChange, ready, refresh, controlClassName }: {
  draft: HotStepCoverDraft; onChange: (draft: HotStepCoverDraft) => void;
  ready: boolean; refresh: () => void; controlClassName: string;
}) {
  const { language } = useI18n();
  const text = labels[language];
  const { data } = useLibrarySongs();
  const tracks = (data ?? []).filter(song => song.audioUrl && !song.isGenerating);
  const active = useRef<AbortController | null>(null);
  const [listening, setListening] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => () => active.current?.abort(), []);

  const choose = (original: string) => {
    active.current?.abort();
    setListening(false);
    setError('');
    const track = tracks.find(song => song.id === original);
    const savedAbc = (track?.generationParams as Song['generationParams'] & { abc?: string } | undefined)?.abc;
    onChange({ original, abc: typeof savedAbc === 'string' ? savedAbc : '' });
  };
  const hear = async () => {
    const run = new AbortController();
    active.current?.abort();
    active.current = run;
    setError('');
    setListening(true);
    try {
      const abc = await transcribe({ songId: draft.original }, false, run.signal);
      if (!run.signal.aborted) onChange({ original: draft.original, abc });
    } catch (error) {
      if (!run.signal.aborted) setError(String(error));
    } finally {
      if (active.current === run) setListening(false);
    }
  };
  const install = async () => {
    setError('');
    setInstalling(true);
    try {
      const reply = await fetch('/v1/music/native/hotstep/install', { method: 'POST' });
      const state = await reply.json();
      if (!reply.ok || !state.ready) throw new Error(state.error || text.error);
      refresh();
    } catch (error) {
      setError(String(error));
    } finally {
      setInstalling(false);
    }
  };

  return <div className="space-y-2">
    <label className="grid gap-1">
      <span>{text.original}</span>
      <select required value={draft.original} className={controlClassName} onChange={event => choose(event.target.value)}>
        <option value="">{text.choose}</option>
        {draft.original && !tracks.some(song => song.id === draft.original) && <option value={draft.original} disabled>{draft.original}</option>}
        {tracks.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
      </select>
    </label>
    <button type="button" disabled={!draft.original || listening} className="text-xs underline disabled:opacity-50"
      onClick={() => void hear()}>{listening ? text.listening : text.transcribe}</button>
    <label className="grid gap-1">
      <span>{text.score}</span>
      <textarea rows={4} className={controlClassName} value={draft.abc} disabled={listening}
        onChange={event => onChange({ ...draft, abc: event.target.value })} />
    </label>
    {!ready && <button type="button" disabled={installing} className="block text-xs underline disabled:opacity-50"
      onClick={() => void install()}>{installing ? text.installing : text.install}</button>}
    {error && <p role="alert" className="text-xs">{error}</p>}
    <a className="text-xs underline" target="_blank" rel="noreferrer"
      href="https://github.com/HeartMuLa/MuLaCover/blob/e27fa7fa0da7c9817879cc6210cc5693f9327946/MODEL_LICENSE">{text.license}</a>
  </div>;
}
