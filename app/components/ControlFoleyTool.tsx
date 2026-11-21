import React, { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { YueRequest } from '../types';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { queryClient, readJson } from '../services/studioQueries';
import { musicSourceStrings, nativeMusicSourceStrings } from '../i18n/musicSource';

const strings = {
  en: { heading: 'Sound from video', source: 'Original video', prompt: 'Sound description (optional)', limit: 'Seconds from the start of the video', create: 'Generate sound', error: 'Could not import the video.', ffmpeg: 'Install FFmpeg and make it available on the Studio computer’s PATH.' },
  ru: { heading: 'Звук для видео', source: 'Исходное видео', prompt: 'Описание звука (необязательно)', limit: 'Секунды от начала видео', create: 'Создать звук', error: 'Не удалось импортировать видео.', ffmpeg: 'Установите FFmpeg и добавьте его в PATH на компьютере со Studio.' },
  zh: { heading: '为视频配音效', source: '源视频', prompt: '声音描述（可选）', limit: '从视频起始处截取的秒数', create: '生成音频', error: '无法导入视频。', ffmpeg: '请安装 FFmpeg 并将其加入 Studio 所在电脑的 PATH。' },
  ja: { heading: '動画の効果音', source: '元の動画', prompt: '音の説明（任意）', limit: '動画の先頭からの秒数', create: '音声を生成', error: '動画をインポートできませんでした。', ffmpeg: 'Studio が動くコンピューターで FFmpeg をインストールし、PATH に追加してください。' },
  ko: { heading: '동영상 효과음', source: '원본 동영상', prompt: '소리 설명(선택)', limit: '동영상 시작부터의 초', create: '소리 생성', error: '동영상을 가져오지 못했습니다.', ffmpeg: 'Studio 컴퓨터에 FFmpeg를 설치하고 PATH에 추가하세요.' },
  pt: { heading: 'Som para vídeo', source: 'Vídeo original', prompt: 'Descrição do som (opcional)', limit: 'Segundos desde o início do vídeo', create: 'Gerar som', error: 'Não foi possível importar o vídeo.', ffmpeg: 'Instale o FFmpeg e adicione-o ao PATH do computador do Studio.' },
};

type Model = {
  name: string;
  ready: boolean;
  missing: string[];
  missing_bytes: number;
  model_directory: string;
  package_url: string;
  license_url: string;
  duration_seconds: { min: number; max: number; default: number };
};

export function ControlFoleyTool({ card, control, busy, onGenerate }: {
  card: string;
  control: string;
  busy: boolean;
  onGenerate: (request: YueRequest) => void;
}) {
  const { language } = useI18n();
  const text = strings[language];
  const modelText = nativeMusicSourceStrings[language];
  const common = musicSourceStrings[language];
  const model = useQuery({
    queryKey: ['controlfoley-model'],
    queryFn: () => readJson<Model>('/v1/tools/controlfoley/model'),
  }, queryClient);
  const [source, setSource] = useState<File>();
  const [description, setDescription] = useState('');
  const [duration, setDuration] = useState('');
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState('');
  const status = model.data;
  const limits = status?.duration_seconds;
  const seconds = duration.trim() ? Number(duration) : limits?.default;
  const canCreate = Boolean(source && status?.ready && !busy && !uploading && limits && seconds != null &&
    seconds >= limits.min && seconds <= limits.max);

  async function generate() {
    if (!source || !canCreate) return;
    setUploading(true);
    setError('');
    try {
      const ext = source.name.split('.').at(-1)?.toLowerCase();
      if (!ext || !['mp4', 'mov', 'webm', 'mkv'].includes(ext)) throw new Error(text.error);
      const name = `foley-${crypto.randomUUID()}.${ext}`;
      const response = await fetch(apiUrl(`/v1/videos?name=${encodeURIComponent(name)}`), {
        method: 'POST', body: source,
      });
      if (!response.ok) {
        const problem = await response.json().catch(() => null);
        throw new Error(problem?.error || text.error);
      }
      const imported = await response.json() as { name?: string };
      if (!imported.name) throw new Error(text.error);
      onGenerate({ music_source: { kind: 'audio_cpp', model: 'controlfoley' }, controlfoley_video: imported.name,
        style: description.trim(), lyrics: '', duration_seconds: seconds, output_format: 'wav32' });
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : text.error);
    } finally {
      setUploading(false);
    }
  }

  return <section className={card} aria-label={text.heading}>
    <h2 className="text-sm font-semibold">{text.heading}</h2>
    {model.isError && <p role="alert" className="mt-2 text-xs">{modelText.error}</p>}
    {model.isPending && <p role="status" className="mt-2 text-xs">{modelText.loading}</p>}
    {status && <div className="mt-2 space-y-2 text-xs">
      <p>{status.name}</p>
      {!status.ready && <p role="status">{modelText.unavailable} · {status.missing.join(', ')}</p>}
      {status.missing.includes('FFmpeg') && <p>{text.ffmpeg}</p>}
      {status.missing_bytes > 0 && <>
        <p>{modelText.manualWeights} · {new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(status.missing_bytes / 2 ** 30)} GiB</p>
        <p>{modelText.modelDirectory} <code>{status.model_directory}</code></p>
        <a href={status.package_url} target="_blank" rel="noreferrer" className="mr-3 underline">{modelText.weightFiles}</a>
      </>}
      <a href={status.license_url} target="_blank" rel="noreferrer" className="underline">{modelText.modelLicense}</a>
    </div>}
    <button type="button" className="mt-2 text-xs underline" onClick={() => void model.refetch()}>{common.refresh}</button>
    <div className="mt-3 grid gap-2 text-xs">
      <label className="grid gap-1">
        <span>{text.source}</span>
        <input type="file" className={control} accept=".mp4,.mov,.webm,.mkv" disabled={uploading}
          onChange={event => setSource(event.target.files?.[0])} />
      </label>
      <label className="grid gap-1">
        <span>{text.prompt}</span>
        <textarea rows={2} className={control} value={description} onChange={event => setDescription(event.target.value)} />
      </label>
      <label className="grid gap-1">
        <span>{text.limit}</span>
        <input type="number" className={control} value={duration} min={limits?.min} max={limits?.max}
          step={0.1} placeholder={String(limits?.default ?? 8)} onChange={event => setDuration(event.target.value)} />
      </label>
      <button type="button" className="justify-self-start underline disabled:opacity-50" disabled={!canCreate}
        onClick={() => void generate()}>{text.create}</button>
      {error && <p role="alert">{error}</p>}
    </div>
  </section>;
}
