import React, { useEffect, useRef, useState } from 'react';
import type { YueRequest } from '../types';
import { useI18n } from '../context/I18nContext';
import { SELF_READ } from '../services/studioQueries';
import { apiUrl } from '../services/apiBase';
import { nativeMusicSourceStrings, musicSourceStrings } from '../i18n/musicSource';
import { AudioCppModelInstall } from './HeartMuLaControls';
import { AudioWaveform } from 'lucide-react';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import { ActionReason, ToolCard, ToolState } from './kit/ToolCard';
import { FieldHint, FieldLabel } from './kit/FieldLabel';
import { toolsPageStrings } from '../i18n/toolsPage';
import { formatSize } from '../services/fileSize';

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
  installable?: boolean;
  license_url: string;
  duration_seconds: { min: number; max: number; default: number };
};

export function SoundEffectsTool({ control, busy, onGenerate }: {
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

  const page = toolsPageStrings[language];
  const limit = status?.duration_seconds;
  const filled = useRef(false);
  useEffect(() => { if (limit && !filled.current) { filled.current = true; setDuration(String(limit.default)); } }, [limit]);
  const seconds = Number(duration);
  const inRange = Boolean(limit && duration.trim() && seconds >= limit.min && seconds <= limit.max);
  const reason = !description.trim() ? page.describeSound : limit && !inRange ? `${page.duration}: ${page.range(new Intl.NumberFormat(language).format(limit.min), new Intl.NumberFormat(language).format(limit.max))}` : busy ? page.otherGeneration : null;
  const canCreate = Boolean(status?.ready && inRange && !reason);
  const download = status && !status.ready && status.missing_bytes > 0 && status.installable;

  return <ToolCard icon={<AudioWaveform size={17} />} title={text.heading}
    state={status && (status.ready ? <ToolState tone="ready">{page.ready}</ToolState>
      : download ? <ToolState tone="missing">{page.needsDownload(formatSize(status.missing_bytes, language))}</ToolState>
        : <ToolState tone="blocked">{page.unavailable}</ToolState>)}
    description={page.sfxDescription}
    action={error ? <div className="flex flex-wrap items-center gap-2 text-xs">
        <p role="alert" className="text-rose-600 dark:text-rose-300">{text.unavailable}</p>
        <SecondaryButton onClick={() => { setError(false); refresh(value => value + 1); }}>{common.refresh}</SecondaryButton>
      </div>
      : !status ? <p role="status" className="text-xs text-zinc-500 dark:text-zinc-400">{text.checking}</p>
      : download ? <AudioCppModelInstall model={status.id} bytes={status.missing_bytes} fit
          target={{ catalogue: '/v1/tools/sound-effects/model', installPath: '/v1/music/audio-cpp/models/install', label: status.name }}
          language={language} onInstalled={() => refresh(value => value + 1)} />
      : !status.ready ? <p role="status" className="text-xs">{model.unavailable}</p>
      : <>
        <PrimaryButton disabled={!canCreate}
          onClick={() => {
            if (!canCreate) return;
            onGenerate({ music_source: { kind: 'audio_cpp', model: status.id }, style: description.trim(),
              lyrics: '', duration_seconds: seconds, output_format: 'wav32' });
          }}>{text.create}</PrimaryButton>
        <ActionReason>{reason ?? page.inLibrary}</ActionReason>
      </>}
    footer={status && <>{page.model}: {status.name} · <a href={status.license_url} target="_blank" rel="noreferrer" className="underline-offset-2 hover:text-pink-600 hover:underline">{page.license} ↗</a></>}>
    <label className="grid text-xs">
      <FieldLabel>{page.sfxPrompt}</FieldLabel>
      <FieldHint>{page.sfxPromptHint}</FieldHint>
      <textarea rows={3} value={description} className={control} onChange={event => setDescription(event.target.value)} />
    </label>
    <label className="grid text-xs">
      <FieldLabel>{page.duration}</FieldLabel>
      {limit && <FieldHint>{page.range(new Intl.NumberFormat(language).format(limit.min), new Intl.NumberFormat(language).format(limit.max))}</FieldHint>}
      <input type="number" className={`${control} max-w-32`} value={duration} min={limit?.min} max={limit?.max}
        step={0.1} onChange={event => setDuration(event.target.value)} />
    </label>
  </ToolCard>;
}
