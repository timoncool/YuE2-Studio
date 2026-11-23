import React, { useEffect, useRef, useState } from 'react';
import { Ear, ListChecks, Loader2, Pause, Play, Upload } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { useLibrarySongs } from '../services/studioQueries';
import { CHOICE, CONTROL, Field, LABEL, SliderRow, ditKind } from './AceStudioFields';
import { splitDit } from './AceDitSwitcher';

/**
 * ACE-Step Studio 4.0.1's source card, carried over: the song a task works on, with a listen and a file upload, the
 * tasks it can run (cover, cover without FSQ, repaint, add track, extract, complete), their settings and the tracks
 * the base-model tasks act on, the engine describing the source or handing its audio codes, and the timbre reference.
 */

const TASKS = ['text2music', 'cover', 'cover-nofsq', 'repaint', 'lego', 'extract', 'complete'] as const;
type AceTask = typeof TASKS[number];
const baseOnly = (task: string) => task === 'lego' || task === 'extract' || task === 'complete';
export const ACE_TRACKS = ['vocals', 'backing_vocals', 'drums', 'bass', 'guitar', 'keyboard', 'percussion', 'strings', 'synth', 'fx', 'brass', 'woodwinds'];

export type AceCreateOptions = {
  task_type?: AceTask;
  source_song_id?: string;
  reference_song_id?: string;
  audio_cover_strength?: number;
  cover_noise_strength?: number;
  repainting_start?: number;
  repainting_end?: number;
  track?: string;
  guidance_scale?: number;
  adapters?: { name: string; scale: number }[];
};
type AceTaskField = 'audio_cover_strength' | 'cover_noise_strength' | 'repainting_start' | 'repainting_end';
export type AceTaskDraft = { task: string; source: string; reference: string; tracks?: string[] } & Partial<Record<AceTaskField, string>>;
export const newAceTask: AceTaskDraft = { task: 'text2music', source: '', reference: '' };

const coverFields = ['audio_cover_strength', 'cover_noise_strength'] as const;
const repaintFields = ['repainting_start', 'repainting_end'] as const;

function taskFields(task: string): readonly AceTaskField[] {
  if (task === 'repaint' || task === 'lego') return repaintFields;
  if (task === 'cover' || task === 'cover-nofsq') return coverFields;
  return [];
}

export function restoreAceTask(settings: Record<string, unknown>): AceTaskDraft {
  const text = (value: unknown) => typeof value === 'string' ? value : '';
  const parameters = [...coverFields, ...repaintFields].flatMap(key =>
    typeof settings[key] === 'number' ? [[key, key === 'repainting_end' && settings[key] === -1 ? '' : String(settings[key])]] : []);
  const tracks = text(settings.track).split('|').map(track => track.trim().toLowerCase()).filter(track => ACE_TRACKS.includes(track));
  return {
    task: text(settings.task_type) || 'text2music',
    source: text(settings.source_song_id),
    reference: text(settings.reference_song_id),
    ...(tracks.length ? { tracks } : {}),
    ...Object.fromEntries(parameters),
  };
}

export function aceTaskOptions(draft: AceTaskDraft): AceCreateOptions | undefined {
  const task = TASKS.find(value => value === draft.task);
  if (!task || (task !== 'text2music' && !draft.source)) return undefined;
  const options: AceCreateOptions = {
    ...(task !== 'text2music' ? { task_type: task, source_song_id: draft.source } : {}),
    ...(draft.reference ? { reference_song_id: draft.reference } : {}),
  };
  for (const key of taskFields(task)) {
    const text = draft[key]?.trim();
    if (!text) continue;
    const value = Number(text);
    if (!Number.isFinite(value) || value < 0 || (!['repaint', 'lego'].includes(task) && value > 1)) return undefined;
    options[key] = value;
  }
  if (options.repainting_end !== undefined && options.repainting_end <= (options.repainting_start ?? 0)) return undefined;
  if (baseOnly(task)) {
    const tracks = draft.tracks ?? [];
    if (!tracks.length) return undefined;
    // complete names its tracks in capitals, as ACE-Step Studio sent them
    options.track = tracks.map(track => (task === 'complete' ? track.toUpperCase() : track)).join(' | ');
  }
  return options;
}

const local = {
  en: { invalid: 'Choose a source and use valid values for the selected task.', installBase: 'This task needs a base DiT. Download one in the DiT list above (Base or Base XL), then pick it here.' },
  ru: { invalid: 'Выберите источник и заполните допустимые значения для задачи.', installBase: 'Этой задаче нужен base DiT. Скачайте его в списке DiT выше (Base или Base XL), затем выберите здесь.' },
  zh: { invalid: '请选择源歌曲并填写有效的任务参数。', installBase: '此任务需要基础 DiT。请在上方的 DiT 列表中下载（Base 或 Base XL），然后在此选择。' },
  ja: { invalid: '元の曲と有効なタスク設定を指定してください。', installBase: 'このタスクには base DiT が必要です。上の DiT 一覧でダウンロードし（Base または Base XL）、ここで選んでください。' },
  ko: { invalid: '원본 곡을 선택하고 유효한 작업 값을 입력하세요.', installBase: '이 작업에는 base DiT가 필요합니다. 위 DiT 목록에서 내려받은 뒤(Base 또는 Base XL) 여기서 고르세요.' },
  pt: { invalid: 'Selecione uma origem e use valores válidos para a tarefa.', installBase: 'Esta tarefa precisa de um DiT base. Baixe um na lista de DiT acima (Base ou Base XL) e escolha-o aqui.' },
};

const ICON = 'inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-zinc-200 text-zinc-500 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300';
const ACTION = 'inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-[11px] font-semibold text-zinc-600 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/10 dark:text-zinc-300';
const AUDIO_ACCEPT = 'audio/*,.flac,.wav,.mp3,.ogg,.m4a';

export function AceTaskControls({ value, onChange, profileReady, dit, onPickDit, listening, onListen, card }: {
  value: AceTaskDraft;
  onChange: (value: AceTaskDraft) => void;
  profileReady: boolean;
  /** The DiT the song renders with. */
  dit?: string;
  onPickDit: (file: string) => void;
  listening: 'describe' | 'codes' | null;
  onListen: (what: 'describe' | 'codes') => void;
  card: (title: string, body: React.ReactNode, actions?: React.ReactNode) => React.ReactNode;
}) {
  const { t, language } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const own = local[language];
  const { data, refetch } = useLibrarySongs();
  const songs = (data ?? []).filter(song => song.audioUrl && !song.isGenerating);
  const [playing, setPlaying] = useState<string | null>(null);
  const [uploading, setUploading] = useState<'source' | 'reference' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [baseDits, setBaseDits] = useState<string[] | null>(null);
  const player = useRef<HTMLAudioElement | null>(null);
  const task = value.task;
  const kind = ditKind(dit);
  const needsBase = baseOnly(task) && kind !== 'base';

  useEffect(() => () => player.current?.pause(), []);
  useEffect(() => {
    if (!needsBase) return;
    void fetch(apiUrl('/v1/ace/dits'))
      .then(response => response.json() as Promise<{ dits?: { file: string; installed: boolean }[] }>)
      .then(body => setBaseDits((body.dits ?? []).filter(entry => entry.installed && ditKind(entry.file) === 'base').map(entry => entry.file)))
      .catch(() => setBaseDits([]));
  }, [needsBase]);

  const chooseSource = (id: string) => onChange({ ...value, source: id, task: id ? (task === 'text2music' ? 'cover' : task) : 'text2music' });

  const togglePreview = (songId: string) => {
    if (playing === songId) {
      player.current?.pause();
      setPlaying(null);
      return;
    }
    player.current?.pause();
    const audio = new Audio(apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`));
    audio.onended = () => setPlaying(null);
    player.current = audio;
    void audio.play().then(() => setPlaying(songId)).catch(() => setPlaying(null));
  };

  const upload = async (target: 'source' | 'reference', file: File) => {
    setUploading(target);
    setError(null);
    try {
      const form = new FormData();
      form.append('audio', file);
      const response = await fetch(apiUrl('/v1/library/import'), { method: 'POST', body: form });
      const body = await response.json().catch(() => null) as { id?: string; error?: string } | null;
      if (!response.ok || !body?.id) throw new Error(body?.error || String(response.status));
      await refetch();
      window.dispatchEvent(new CustomEvent('studio:library-changed'));
      if (target === 'source') chooseSource(body.id);
      else onChange({ ...value, reference: body.id });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setUploading(null);
    }
  };

  const songOptions = (chosen: string, choose: (id: string) => void, empty: string, target: 'source' | 'reference') => (
    <div className="flex items-center gap-1.5">
      <select value={chosen} onChange={event => choose(event.target.value)} className={CONTROL} aria-label={target === 'source' ? tt('aceSource') : tt('aceReference')}>
        <option value="">{empty}</option>
        {chosen && !songs.some(song => song.id === chosen) && <option value={chosen} disabled>{chosen}</option>}
        {songs.map(song => <option key={song.id} value={song.id}>{song.title}</option>)}
      </select>
      {chosen && (
        <button type="button" onClick={() => togglePreview(chosen)} className={ICON} title={tt('acePreview')} aria-label={tt('acePreview')}>
          {playing === chosen ? <Pause size={14} /> : <Play size={14} />}
        </button>
      )}
      <label className={`${ICON} cursor-pointer`} title={tt('aceUpload')}>
        {uploading === target ? <Loader2 size={14} className="animate-spin" /> : <Upload size={14} />}
        <input type="file" accept={AUDIO_ACCEPT} className="hidden" aria-label={tt('aceUpload')}
          onChange={event => { const file = event.target.files?.[0]; if (file) void upload(target, file); event.target.value = ''; }} />
      </label>
    </div>
  );

  const tracks = value.tracks ?? [];
  const setTracks = (track: string) => {
    const on = tracks.includes(track);
    const next = task === 'complete' ? (on ? tracks.filter(item => item !== track) : [...tracks, track]) : (on ? [] : [track]);
    onChange({ ...value, tracks: next });
  };
  const invalid = profileReady && (task === 'text2music' || value.source) && !aceTaskOptions(value);

  return <>{card(tt('aceSourceSection'), <div className="space-y-3">
    <Field label={tt('aceSource')}>{songOptions(value.source, chooseSource, tt('aceChooseTrack'), 'source')}</Field>
    {!value.source ? (
      <p className="text-[11px] leading-4 text-zinc-500">{tt('aceSourceEmpty')}</p>
    ) : (
      <>
        <div className="grid grid-cols-3 gap-1.5">
          {TASKS.filter(entry => entry !== 'text2music').map(entry => (
            <button key={entry} type="button" aria-pressed={task === entry} onClick={() => onChange({ ...value, task: entry })}
              className={CHOICE(task === entry)} title={tt(`aceTaskHint_${entry}`)}>{tt(`aceTask_${entry}`)}</button>
          ))}
        </div>
        <p className="text-[11px] leading-4 text-zinc-500">{tt(`aceTaskHint_${task}`)}</p>
        {needsBase && (
          <div className="rounded-lg bg-amber-500/10 p-2 text-[11px] leading-4 text-amber-700 dark:text-amber-300">
            {baseDits?.length ? tt('aceBaseOnlyPick') : own.installBase}
            {baseDits && baseDits.length > 0 && (
              <select value="" onChange={event => event.target.value && onPickDit(event.target.value)} className={`${CONTROL} mt-2`} aria-label={tt('aceChooseBase')}>
                <option value="">{tt('aceChooseBase')}</option>
                {baseDits.map(file => <option key={file} value={file}>{tt(`aceVariant_${splitDit(file).variant}`)} · {file.replace(/\.gguf$/, '')}</option>)}
              </select>
            )}
          </div>
        )}
        {(task === 'cover' || task === 'cover-nofsq') && (
          <div className="space-y-3">
            <SliderRow label={tt('aceCoverStrength')} value={value.audio_cover_strength ?? ''} fallback={1} min={0} max={1} step={0.05} onChange={next => onChange({ ...value, audio_cover_strength: next })} />
            <SliderRow label={tt('aceCoverNoise')} value={value.cover_noise_strength ?? ''} fallback={0} min={0} max={1} step={0.05} onChange={next => onChange({ ...value, cover_noise_strength: next })} />
            <p className="text-[11px] leading-4 text-zinc-500">{tt('aceCoverHint')}</p>
          </div>
        )}
        {(task === 'repaint' || task === 'lego') && (
          <>
            <div className="grid grid-cols-2 gap-2">
              <Field label={tt('aceRepaintStart')}><input value={value.repainting_start ?? ''} onChange={event => onChange({ ...value, repainting_start: event.target.value })} placeholder="0" inputMode="decimal" aria-label={tt('aceRepaintStart')} className={CONTROL} /></Field>
              <Field label={tt('aceRepaintEnd')}><input value={value.repainting_end ?? ''} onChange={event => onChange({ ...value, repainting_end: event.target.value })} placeholder={tt('aceToTheEnd')} inputMode="decimal" aria-label={tt('aceRepaintEnd')} className={CONTROL} /></Field>
            </div>
            <p className="text-[11px] leading-4 text-zinc-500">{tt('aceRepaintHint')}</p>
          </>
        )}
        {baseOnly(task) && (
          <div>
            <span className={LABEL}>{task === 'extract' ? tt('aceTrackExtract') : task === 'lego' ? tt('aceTrackAdd') : tt('aceTrackComplete')}</span>
            <div className="flex flex-wrap gap-1.5">
              {ACE_TRACKS.map(track => (
                <button key={track} type="button" aria-pressed={tracks.includes(track)} onClick={() => setTracks(track)}
                  className={`rounded-full border px-2.5 py-1 text-[11px] ${tracks.includes(track) ? 'border-pink-500 bg-pink-500/10 text-pink-600 dark:text-pink-300' : 'border-zinc-200 text-zinc-600 dark:border-white/10 dark:text-zinc-300'}`}>
                  {tt(`aceTrack_${track}`)}
                </button>
              ))}
            </div>
          </div>
        )}
        <div className="flex flex-wrap gap-1.5">
          <button type="button" onClick={() => onListen('describe')} disabled={listening !== null || !profileReady} className={ACTION}>
            {listening === 'describe' ? <Loader2 size={12} className="animate-spin" /> : <Ear size={12} />}{tt('aceDescribeSource')}
          </button>
          <button type="button" onClick={() => onListen('codes')} disabled={listening !== null || !profileReady} className={ACTION}>
            {listening === 'codes' ? <Loader2 size={12} className="animate-spin" /> : <ListChecks size={12} />}{tt('aceSourceCodes')}
          </button>
        </div>
      </>
    )}
    <div className="border-t border-zinc-100 pt-3 dark:border-white/5">
      <Field label={tt('aceReference')} hint={tt('aceReferenceHint')}>{songOptions(value.reference, id => onChange({ ...value, reference: id }), tt('aceNoReference'), 'reference')}</Field>
    </div>
    {baseOnly(task) && value.source && tracks.length === 0 && <p role="alert" className="text-[11px] text-amber-700 dark:text-amber-300">{tt('aceTrackRequired')}</p>}
    {invalid && !(baseOnly(task) && tracks.length === 0) && <p role="alert" className="text-[11px] text-rose-600 dark:text-rose-300">{own.invalid}</p>}
    {error && <p role="alert" className="text-[11px] text-rose-600 dark:text-rose-300">{error}</p>}
  </div>)}</>;
}
