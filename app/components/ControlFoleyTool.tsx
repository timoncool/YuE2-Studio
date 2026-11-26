import React, { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { YueRequest } from '../types';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { queryClient, readJson } from '../services/studioQueries';
import { musicSourceStrings, nativeMusicSourceStrings } from '../i18n/musicSource';
import { AudioCppModelInstall } from './HeartMuLaControls';
import { Clapperboard, FileVideo } from 'lucide-react';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import { ActionReason, ToolCard, ToolState } from './kit/ToolCard';
import { ToolDevice } from './ToolDevice';
import { FieldHint, FieldLabel } from './kit/FieldLabel';
import { toolsPageStrings } from '../i18n/toolsPage';
import { formatSize } from '../services/fileSize';

const strings = {
  en: { description: 'Makes the sound for a video — steps, hits, ambience — from the picture itself and, if you like, a short description.', chooseVideo: 'Choose a video', heading: 'Sound from video', source: 'Original video', prompt: 'Sound description (optional)', limit: 'Seconds from the start of the video', create: 'Generate sound', error: 'Could not import the video.', ffmpeg: 'Install FFmpeg and make it available on the Studio computer’s PATH.' },
  ru: { description: 'Создаёт звук под видео — шаги, удары, фон — по самой картинке и, если нужно, по короткому описанию.', chooseVideo: 'Выбрать видео', heading: 'Звук для видео', source: 'Исходное видео', prompt: 'Описание звука (необязательно)', limit: 'Секунды от начала видео', create: 'Создать звук', error: 'Не удалось импортировать видео.', ffmpeg: 'Установите FFmpeg и добавьте его в PATH на компьютере со Studio.' },
  zh: { description: '根据画面本身（可附简短描述）为视频生成声音：脚步、撞击、环境声。', chooseVideo: '选择视频', heading: '为视频配音效', source: '源视频', prompt: '声音描述（可选）', limit: '从视频起始处截取的秒数', create: '生成音频', error: '无法导入视频。', ffmpeg: '请安装 FFmpeg 并将其加入 Studio 所在电脑的 PATH。' },
  ja: { description: '映像そのものと、必要なら短い説明から、足音・衝撃音・環境音などの音を作ります。', chooseVideo: '動画を選ぶ', heading: '動画の効果音', source: '元の動画', prompt: '音の説明（任意）', limit: '動画の先頭からの秒数', create: '音声を生成', error: '動画をインポートできませんでした。', ffmpeg: 'Studio が動くコンピューターで FFmpeg をインストールし、PATH に追加してください。' },
  ko: { description: '영상 자체와 원하면 짧은 설명으로 발소리, 타격음, 배경음 같은 소리를 만듭니다.', chooseVideo: '동영상 선택', heading: '동영상 효과음', source: '원본 동영상', prompt: '소리 설명(선택)', limit: '동영상 시작부터의 초', create: '소리 생성', error: '동영상을 가져오지 못했습니다.', ffmpeg: 'Studio 컴퓨터에 FFmpeg를 설치하고 PATH에 추가하세요.' },
  pt: { description: 'Cria o som de um vídeo — passos, impactos, ambiente — a partir da própria imagem e, se quiser, de uma breve descrição.', chooseVideo: 'Escolher vídeo', heading: 'Som para vídeo', source: 'Vídeo original', prompt: 'Descrição do som (opcional)', limit: 'Segundos desde o início do vídeo', create: 'Gerar som', error: 'Não foi possível importar o vídeo.', ffmpeg: 'Instale o FFmpeg e adicione-o ao PATH do computador do Studio.' },
};

type Model = {
  name: string;
  ready: boolean;
  missing: string[];
  missing_bytes: number;
  installable?: boolean;
  license_url: string;
  duration_seconds: { min: number; max: number; default: number };
};

export function ControlFoleyTool({ control, busy, onGenerate }: {
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
  const fileInput = useRef<HTMLInputElement>(null);
  const [description, setDescription] = useState('');
  const [duration, setDuration] = useState('');
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState('');
  const status = model.data;
  const page = toolsPageStrings[language];
  const limits = status?.duration_seconds;
  const filled = useRef(false);
  useEffect(() => { if (limits && !filled.current) { filled.current = true; setDuration(String(limits.default)); } }, [limits]);
  const seconds = Number(duration);
  const inRange = Boolean(limits && duration.trim() && seconds >= limits.min && seconds <= limits.max);
  const noFfmpeg = Boolean(status?.missing.includes('FFmpeg'));
  const reason = noFfmpeg ? text.ffmpeg : !source ? page.chooseVideo : limits && !inRange ? `${page.foleySeconds}: ${page.range(new Intl.NumberFormat(language).format(limits.min), new Intl.NumberFormat(language).format(limits.max))}` : busy ? page.otherGeneration : null;
  const canCreate = Boolean(status?.ready && inRange && !reason && !uploading);
  const download = status && !status.ready && status.missing_bytes > 0 && status.installable;
  const [queued, setQueued] = useState(false);

  async function generate(): Promise<boolean> {
    if (!source || !canCreate) return false;
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
      return true;
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : text.error);
      return false;
    } finally {
      setUploading(false);
    }
  }

  return <ToolCard icon={<Clapperboard size={17} />} title={text.heading}
    state={status && (status.ready && !noFfmpeg ? <ToolState tone="ready">{page.ready}</ToolState>
      : download ? <ToolState tone="missing">{page.needsDownload(formatSize(status.missing_bytes, language))}</ToolState>
        : <ToolState tone="blocked">{page.unavailable}</ToolState>)}
    description={text.description}
    action={model.isError ? <div className="flex flex-wrap items-center gap-2 text-xs">
        <p role="alert" className="text-rose-600 dark:text-rose-300">{modelText.error}</p>
        <SecondaryButton onClick={() => void model.refetch()}>{common.refresh}</SecondaryButton>
      </div>
      : !status ? <p role="status" className="text-xs text-zinc-500 dark:text-zinc-400">{modelText.loading}</p>
      : download ? <>
          <AudioCppModelInstall model="controlfoley" bytes={status.missing_bytes} fit
            target={{ catalogue: '/v1/tools/controlfoley/model', installPath: '/v1/music/audio-cpp/models/install', label: status.name }}
            language={language} onInstalled={() => void model.refetch()} />
          {noFfmpeg && <ActionReason>{text.ffmpeg}</ActionReason>}
        </>
      : !status.ready && !noFfmpeg ? <p role="status" className="text-xs">{modelText.unavailable}</p>
      : <>
        <PrimaryButton disabled={!canCreate} onClick={() => void generate().then(ok => setQueued(ok))}>{text.create}</PrimaryButton>
        <ActionReason>{reason ?? (queued ? page.inLibrary : '')}</ActionReason>
      </>}
    error={error && <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">{error}</p>}
    footer={status && <>{page.model}: {status.name} · <a href={status.license_url} target="_blank" rel="noreferrer" className="underline-offset-2 hover:text-pink-600 hover:underline">{page.license} ↗</a></>}>
    <div className="grid text-xs">
      <FieldLabel hidden>{text.source}</FieldLabel>
      <div className="flex min-w-0 items-center gap-2">
        <SecondaryButton disabled={uploading} aria-label={`${text.source}: ${source?.name ?? text.chooseVideo}`} onClick={() => fileInput.current?.click()}><FileVideo size={13} aria-hidden />{text.chooseVideo}</SecondaryButton>
        {source && <span className="min-w-0 truncate text-zinc-600 dark:text-zinc-300" title={source.name}>{source.name}</span>}
      </div>
      <input ref={fileInput} type="file" className="hidden" accept=".mp4,.mov,.webm,.mkv" aria-label={text.source}
        onChange={event => { setSource(event.target.files?.[0]); setQueued(false); event.target.value = ''; }} />
    </div>
    <label className="grid text-xs">
      <FieldLabel>{text.prompt}</FieldLabel>
      <textarea rows={2} className={control} value={description} onChange={event => setDescription(event.target.value)} />
    </label>
    <label className="grid text-xs">
      <FieldLabel>{page.foleySeconds}</FieldLabel>
      {limits && <FieldHint>{page.range(new Intl.NumberFormat(language).format(limits.min), new Intl.NumberFormat(language).format(limits.max))}</FieldHint>}
      <input type="number" className={`${control} max-w-32`} value={duration} min={limits?.min} max={limits?.max}
        step={0.1} onChange={event => setDuration(event.target.value)} />
    </label>
    <ToolDevice tool="foley" disabled={uploading} onChange={() => void model.refetch()} />
  </ToolCard>;
}
