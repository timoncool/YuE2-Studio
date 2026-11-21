import React, { useEffect, useState } from 'react';
import type { YueRequest } from '../types';
import { useI18n } from '../context/I18nContext';
import { SELF_READ } from '../services/studioQueries';
import { apiUrl } from '../services/apiBase';
import { nativeMusicSourceStrings, musicSourceStrings } from '../i18n/musicSource';

const strings = {
  en: { heading: 'Sound effects', description: 'Sound description (English or Chinese)', create: 'Generate sound effect', checking: 'Checking MOSS-SoundEffect…', unavailable: 'MOSS-SoundEffect is unavailable.', saved: 'Sound effects are saved in the library.' },
  ru: { heading: 'Звуковые эффекты', description: 'Описание звука на английском или китайском', create: 'Создать звуковой эффект', checking: 'Проверяется MOSS-SoundEffect…', unavailable: 'MOSS-SoundEffect недоступен.', saved: 'Эффекты сохраняются в библиотеке.' },
  zh: { heading: '音效', description: '声音描述（英语或中文）', create: '生成音效', checking: '正在检查 MOSS-SoundEffect…', unavailable: 'MOSS-SoundEffect 不可用。', saved: '音效会保存到资料库。' },
  ja: { heading: '効果音', description: '音の説明（英語または中国語）', create: '効果音を生成', checking: 'MOSS-SoundEffect を確認しています…', unavailable: 'MOSS-SoundEffect は利用できません。', saved: '効果音はライブラリに保存されます。' },
  ko: { heading: '효과음', description: '소리 설명(영어 또는 중국어)', create: '효과음 생성', checking: 'MOSS-SoundEffect를 확인하는 중…', unavailable: 'MOSS-SoundEffect를 사용할 수 없습니다.', saved: '효과음은 라이브러리에 저장됩니다.' },
  pt: { heading: 'Efeitos sonoros', description: 'Descrição do som (inglês ou chinês)', create: 'Gerar efeito sonoro', checking: 'Verificando MOSS-SoundEffect…', unavailable: 'MOSS-SoundEffect está indisponível.', saved: 'Os efeitos são salvos na biblioteca.' },
};

type Status = {
  id: string;
  name: string;
  ready: boolean;
  missing: string[];
  missing_bytes: number;
  model_directory: string;
  package_url: string;
  license_url: string;
  duration_seconds: { min: number; max: number; default: number };
};

export function SoundEffectsTool({ card, control, busy, onGenerate }: {
  card: string;
  control: string;
  busy: boolean;
  onGenerate: (request: YueRequest) => void;
}) {
  const { language } = useI18n();
  const text = strings[language];
  const model = nativeMusicSourceStrings[language];
  const common = musicSourceStrings[language];
  const [description, setDescription] = useState('');
  const [duration, setDuration] = useState('');
  const [status, setStatus] = useState<Status>();
  const [error, setError] = useState(false);
  const [revision, refresh] = useState(0);

  useEffect(() => {
    const abort = new AbortController();
    setStatus(undefined);
    setError(false);
    void fetch(apiUrl('/v1/tools/sound-effects/model'), { headers: SELF_READ, signal: abort.signal })
      .then(response => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return response.json() as Promise<Status>;
      })
      .then(value => { if (!abort.signal.aborted) setStatus(value); })
      .catch(() => { if (!abort.signal.aborted) setError(true); });
    return () => abort.abort();
  }, [revision]);

  const limit = status?.duration_seconds;
  const seconds = duration.trim() ? Number(duration) : limit?.default;
  const canCreate = Boolean(status?.ready && description.trim() && !busy && limit && seconds != null &&
    seconds >= limit.min && seconds <= limit.max);

  return <section className={card} aria-label={text.heading}>
    <h2 className="text-sm font-semibold">{text.heading}</h2>
    {error ? <p role="alert" className="mt-2 text-xs">{text.unavailable}</p>
      : !status ? <p role="status" className="mt-2 text-xs">{text.checking}</p>
      : <div className="mt-2 space-y-2 text-xs">
        <p>{status.name}</p>
        {!status.ready && <p role="status">{model.unavailable} · {status.missing.join(', ')}</p>}
        {status.missing_bytes > 0 && <>
          <p>{model.manualWeights} · {new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(status.missing_bytes / 2 ** 30)} GiB</p>
          <p>{model.modelDirectory} <code>{status.model_directory}</code></p>
          <a href={status.package_url} target="_blank" rel="noreferrer" className="mr-3 underline">{model.weightFiles}</a>
        </>}
        <a href={status.license_url} target="_blank" rel="noreferrer" className="underline">{model.modelLicense}</a>
      </div>}
    <button type="button" className="mt-2 text-xs underline" onClick={() => refresh(value => value + 1)}>{common.refresh}</button>
    <div className="mt-3 grid gap-2 text-xs">
      <label className="grid gap-1">
        <span>{text.description}</span>
        <textarea rows={3} value={description} className={control} onChange={event => setDescription(event.target.value)} />
      </label>
      <label className="grid gap-1">
        <span>{model.duration}</span>
        <input type="number" className={control} value={duration} min={limit?.min} max={limit?.max}
          step={0.1} placeholder={String(limit?.default ?? 10)} onChange={event => setDuration(event.target.value)} />
      </label>
    </div>
    <button type="button" disabled={!canCreate} className="mt-3 text-xs underline disabled:opacity-50"
      onClick={() => {
        if (!status || !canCreate) return;
        onGenerate({ music_source: { kind: 'audio_cpp', model: status.id }, style: description.trim(),
          lyrics: '', duration_seconds: seconds, output_format: 'wav32' });
      }}>{text.create}</button>
    <p className="mt-2 text-xs text-zinc-500">{text.saved}</p>
  </section>;
}
