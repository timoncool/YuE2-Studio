import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Circle, FileAudio, Loader2, Pause, Play, Square, Upload } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { soundSourceStrings } from '../../i18n/soundSource';
import { apiUrl } from '../../services/apiBase';
import { PERSON_ONLY, byPerson } from '../../services/person';
import { LibrarySongPicker, useFinishedSongs } from '../LibrarySongPicker';
import type { Song } from '../../types';
import { SecondaryButton } from './Buttons';
import { CatalogSegments } from './Catalog';
import { FieldHint, FieldLabel } from './FieldLabel';
import { Select } from './Select';

export type SoundSourceKind = 'library' | 'mic' | 'file';

/** What the person gave: always a library song, whichever way it came in. */
export interface SoundPick {
  songId: string;
  title: string;
  origin: SoundSourceKind;
  seconds?: number;
  /** The library song itself, when it was chosen from the list. */
  song?: Song;
}

/** Seconds the place needs: the least it takes at all, enough for a good result, and the most it uses (a recording stops there). */
export interface SoundNeed {
  min: number;
  good: number;
  max: number;
}

const AUDIO = /\.(wav|flac|mp3|m4a|ogg|oga|opus|aiff?|aac|webm)$/i;
const time = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;
const remote = typeof window !== 'undefined' && !['localhost', '127.0.0.1', '[::1]', 'tauri.localhost'].includes(window.location.hostname)
  && window.location.protocol !== 'tauri:';
const os = typeof navigator === 'undefined' ? 'linux' : /Windows/i.test(navigator.userAgent) ? 'windows' : /Mac/i.test(navigator.userAgent) ? 'mac' : 'linux';

function remembered(place: string, sources: SoundSourceKind[], fallback: SoundSourceKind): SoundSourceKind {
  try {
    const kept = localStorage.getItem(`studio.soundSource.${place}`) as SoundSourceKind | null;
    return kept && sources.includes(kept) ? kept : fallback;
  } catch {
    return fallback;
  }
}

type MicState = { devices: string[]; recording: boolean; seconds: number | null; level: number; full: boolean };
type Problem = { code: string; detail: string };

/**
 * Where a tool takes its sound: a track of the library, a recording from the microphone, or a file. One control in
 * every place, Speech Studio's recorder underneath; whatever the person gives lands in the library, so every other
 * tool takes it from there too. The microphone turns on only by the person's own click.
 */
export function SoundSource({ place, label, hint, checklist, sources = ['library', 'mic', 'file'], defaultSource, need, titleFor, value, onChange, disabled, none, showLabel = true, missing }: {
  /** The place's name, for the source it remembers. */
  place: string;
  label: string;
  /** What the place needs, in a line: length, what kind of sound. */
  hint?: string;
  /** How to record well here; the general advice when absent. */
  checklist?: string;
  sources?: SoundSourceKind[];
  defaultSource?: SoundSourceKind;
  need?: SoundNeed;
  /** The library name of a recording, in the person's language. */
  titleFor?: () => string;
  value: SoundPick | null;
  onChange: (pick: SoundPick | null) => void;
  disabled?: boolean;
  /** The library list's first entry for choosing nothing, where the place works without a sound too. */
  none?: string;
  /** False where the form names the field itself. */
  showLabel?: boolean;
  /** The list's words for a chosen song that is no longer in the library. */
  missing?: string;
}) {
  const { language } = useI18n();
  const text = soundSourceStrings[language];
  // a sound already chosen opens where it is; otherwise the place's last source, or its default
  const [source, setSourceState] = useState<SoundSourceKind>(() => value && sources.includes(value.origin) ? value.origin : remembered(place, sources, defaultSource ?? sources[0]));
  const setSource = (next: SoundSourceKind) => {
    setSourceState(next);
    try { localStorage.setItem(`studio.soundSource.${place}`, next); } catch { /* the choice is only a convenience */ }
  };
  const { songs, loaded } = useFinishedSongs();
  const [mic, setMic] = useState<MicState | null>(null);
  const [device, setDevice] = useState('');
  const [phase, setPhase] = useState<'idle' | 'countdown' | 'recording' | 'saving'>('idle');
  const [count, setCount] = useState(3);
  const [quietSince, setQuietSince] = useState<number | null>(null);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [uploading, setUploading] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const [playing, setPlaying] = useState(false);
  const player = useRef<HTMLAudioElement | null>(null);
  const picker = useRef<HTMLInputElement>(null);
  const countdown = useRef<number | null>(null);
  const busy = phase !== 'idle' || uploading !== null;

  const readMic = useCallback(async () => {
    try {
      const state = await fetch(apiUrl('/v1/mic')).then(response => response.json()) as MicState;
      setMic(state);
      setDevice(current => current || state.devices[0] || '');
      return state;
    } catch (failure) {
      setProblem({ code: 'unavailable', detail: failure instanceof Error ? failure.message : String(failure) });
      return null;
    }
  }, []);

  useEffect(() => { if (source === 'mic' && !mic) void readMic(); }, [source, mic, readMic]);
  useEffect(() => () => {
    player.current?.pause();
    if (countdown.current) window.clearInterval(countdown.current);
  }, []);

  const stop = useCallback(async (discard = false) => {
    setPhase(discard ? 'idle' : 'saving');
    try {
      const response = await fetch(apiUrl('/v1/mic/stop'), {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ discard, title: titleFor?.() ?? `${text.recordingTitle} · ${new Date().toLocaleString(language, { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })}`, min_seconds: need?.min }),
      });
      const body = await response.json().catch(() => null) as { song_id?: string; title?: string; seconds?: number; code?: string; error?: string } | null;
      if (!response.ok) {
        setProblem({ code: body?.code ?? 'unavailable', detail: body?.error ?? String(response.status) });
        return;
      }
      if (!discard && body?.song_id) {
        onChange({ songId: body.song_id, title: body.title ?? text.recordingTitle, origin: 'mic', seconds: body.seconds });
        window.dispatchEvent(new CustomEvent('studio:library-changed'));
      }
    } finally {
      setPhase('idle');
      setQuietSince(null);
      void readMic();
    }
  }, [need?.min, onChange, readMic, text.recordingTitle, titleFor]);

  useEffect(() => {
    if (phase !== 'recording') return;
    const timer = window.setInterval(() => {
      void readMic().then(state => {
        if (!state) return;
        if (state.full) { void stop(); return; }
        setQuietSince(since => state.level < 0.05 ? since ?? Date.now() : null);
      });
    }, 150);
    return () => window.clearInterval(timer);
  }, [phase, readMic, stop]);

  const begin = async () => {
    try {
      const response = await fetch(apiUrl('/v1/mic/start'), {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ device: device || undefined, limit_seconds: need?.max }),
      });
      const body = await response.json().catch(() => null) as { code?: string; error?: string } | null;
      if (!response.ok) {
        setPhase('idle');
        setProblem({ code: body?.code ?? 'unavailable', detail: body?.error ?? String(response.status) });
        return;
      }
      setPhase('recording');
      void readMic();
    } catch (failure) {
      setPhase('idle');
      setProblem({ code: 'unavailable', detail: failure instanceof Error ? failure.message : String(failure) });
    }
  };

  const record = (event: React.MouseEvent) => {
    if (!byPerson(event)) return;
    setProblem(null);
    if (value?.origin === 'mic') {
      // the take being replaced goes to the library's trash, where it can still be brought back
      void fetch(apiUrl(`/v1/library/songs/${encodeURIComponent(value.songId)}`), { method: 'DELETE' })
        .then(() => window.dispatchEvent(new CustomEvent('studio:library-changed')));
      onChange(null);
    }
    setCount(3);
    setPhase('countdown');
    let left = 3;
    countdown.current = window.setInterval(() => {
      left -= 1;
      if (left > 0) { setCount(left); return; }
      if (countdown.current) window.clearInterval(countdown.current);
      countdown.current = null;
      void begin();
    }, 1000);
  };

  const cancelCountdown = () => {
    if (countdown.current) window.clearInterval(countdown.current);
    countdown.current = null;
    setPhase('idle');
  };

  const upload = async (file: File) => {
    setProblem(null);
    if (!AUDIO.test(file.name)) {
      setProblem({ code: 'not_audio', detail: file.name });
      return;
    }
    setUploading(file.name);
    try {
      const form = new FormData();
      form.append('audio', file);
      form.append('title', file.name.replace(/\.[^.]+$/, ''));
      const response = await fetch(apiUrl('/v1/library/import'), { method: 'POST', body: form });
      const body = await response.json().catch(() => null) as { id?: string; title?: string; error?: string } | null;
      if (!response.ok || !body?.id) throw new Error(body?.error || String(response.status));
      onChange({ songId: body.id, title: body.title ?? file.name, origin: 'file' });
      window.dispatchEvent(new CustomEvent('studio:library-changed'));
    } catch (failure) {
      setProblem({ code: 'upload', detail: failure instanceof Error ? failure.message : String(failure) });
    } finally {
      setUploading(null);
    }
  };

  const listen = () => {
    if (!value) return;
    player.current?.pause();
    if (playing) { setPlaying(false); return; }
    const audio = new Audio(apiUrl(`/v1/library/media/${encodeURIComponent(value.songId)}`));
    audio.onended = () => setPlaying(false);
    player.current = audio;
    void audio.play().then(() => setPlaying(true)).catch(() => setPlaying(false));
  };

  const seconds = mic?.seconds ?? 0;
  const level = mic?.level ?? 0;
  const loud = phase === 'recording' && level >= 0.98;
  const quiet = phase === 'recording' && quietSince !== null && Date.now() - quietSince > 3000;
  const elsewhere = phase === 'idle' && mic?.recording;
  const origin = value ? { library: text.fromLibrary, mic: text.fromMic, file: text.fromFile }[value.origin] : '';
  const problemText = (() => {
    if (!problem) return null;
    switch (problem.code) {
      case 'no_device': return text.noDevice;
      case 'device_gone': return text.deviceGone(device);
      case 'in_use': return text.inUse;
      case 'silent': return `${text.silent} ${text.silentWhere[os]}.`;
      case 'too_short': return text.tooShort(time(need?.min ?? 1), time(need?.good ?? 10));
      case 'not_audio': return text.notAudio(problem.detail);
      case 'busy': return text.busyElsewhere(time(seconds));
      case 'upload': return problem.detail;
      default: return text.failed(problem.detail);
    }
  })();

  return (
    <div className="grid text-xs">
      {showLabel && <FieldLabel>{label}</FieldLabel>}
      {hint && <FieldHint>{hint}</FieldHint>}
      {sources.length > 1 && (
        <fieldset disabled={disabled || busy} className="mb-2">
          <CatalogSegments<SoundSourceKind> fill label={label} value={source} onChange={next => { setProblem(null); setSource(next); }}
            options={sources.map(kind => ({ value: kind, label: text[kind] }))} />
        </fieldset>
      )}

      {source === 'library' && (loaded && songs.length === 0 ? (
        <p className="text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">
          {text.emptyLibrary[0]}
          {sources.includes('mic') && <button type="button" className="font-semibold text-pink-600 underline-offset-2 hover:underline dark:text-pink-300" onClick={() => setSource('mic')}>{text.emptyLibrary[1]}</button>}
          {text.emptyLibrary[2]}
          {sources.includes('file') && <button type="button" className="font-semibold text-pink-600 underline-offset-2 hover:underline dark:text-pink-300" onClick={() => setSource('file')}>{text.emptyLibrary[3]}</button>}
          {text.emptyLibrary[4]}
        </p>
      ) : (
        <LibrarySongPicker label={label} showLabel={false} value={value?.origin === 'library' ? value.songId : ''} placeholder={text.findTrack} none={none} missing={missing}
          controlClassName="w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 dark:border-white/10 dark:bg-white/5 dark:text-white"
          onChange={(id, song) => onChange(id ? { songId: id, title: song?.title ?? '', origin: 'library', song } : null)} />
      ))}

      {source === 'mic' && (
        <div className="grid gap-2">
          {mic === null ? (
            <p role="status" className="flex items-center gap-2 text-[13px] text-zinc-600 dark:text-zinc-400"><Loader2 size={13} className="animate-spin" />{text.searching}</p>
          ) : mic.devices.length > 1 && (
            <Select<string> label={text.microphone} value={device} disabled={disabled || busy} onChange={setDevice}
              options={mic.devices.map(name => ({ value: name, label: name }))} />
          )}
          <p className="text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">{checklist ?? text.checklist}</p>
          {remote && <p className="text-[13px] leading-[18px] text-amber-700 dark:text-amber-300">{text.remote}</p>}
          <div className="flex items-center gap-3">
            <div className="h-2 min-w-0 flex-1 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10" role="meter" aria-label={text.level}
              aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(Math.min(1, level * 1.4) * 100)}>
              <div className={`h-full transition-[width] duration-100 ${loud ? 'bg-rose-500' : 'bg-pink-500'}`} style={{ width: `${phase === 'recording' ? Math.min(100, level * 140) : 0}%` }} />
            </div>
            <span className="w-24 text-right font-mono text-[13px] tabular-nums text-zinc-700 dark:text-zinc-200">
              {phase === 'countdown' ? <span className="text-lg font-semibold">{count}</span> : time(phase === 'recording' || elsewhere ? seconds : value?.origin === 'mic' ? value.seconds ?? 0 : 0)}
            </span>
            {phase === 'countdown' ? (
              <SecondaryButton onClick={cancelCountdown}>{text.cancel}</SecondaryButton>
            ) : phase === 'recording' || elsewhere ? (
              <button type="button" onClick={() => void stop()} className="inline-flex min-h-9 items-center gap-1.5 rounded-lg bg-rose-600 px-3 text-xs font-semibold text-white hover:bg-rose-700 max-md:min-h-11">
                <Square size={12} aria-hidden />{text.stop}
              </button>
            ) : phase === 'saving' ? (
              <SecondaryButton disabled><Loader2 size={12} className="animate-spin" />{text.saving}</SecondaryButton>
            ) : (
              <SecondaryButton {...PERSON_ONLY} disabled={disabled || !mic || mic.devices.length === 0} onClick={record}>
                <Circle size={11} className="fill-rose-500 text-rose-500" aria-hidden />{value?.origin === 'mic' ? text.rerecord : text.record}
              </SecondaryButton>
            )}
          </div>
          <p role="status" className="text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">
            {phase === 'countdown' ? text.getReady
              : phase === 'recording' ? (loud ? text.tooLoud : quiet ? text.tooQuiet
                : need ? (seconds < need.good ? text.needAtLeast(time(seconds), time(need.good)) : text.enough(time(seconds), time(need.max))) : text.recording)
                : elsewhere ? text.busyElsewhere(time(seconds)) : ''}
          </p>
          {mic && mic.devices.length === 0 && !problem && (
            <div className="flex flex-wrap items-center gap-2">
              <p role="alert" className="text-[13px] text-rose-700 dark:text-rose-300">{text.noDevice}</p>
              <SecondaryButton onClick={() => void readMic()}>{text.checkAgain}</SecondaryButton>
            </div>
          )}
        </div>
      )}

      {source === 'file' && (
        <div
          onDragOver={event => { event.preventDefault(); setOver(true); }} onDragLeave={() => setOver(false)}
          onDrop={event => { event.preventDefault(); setOver(false); const file = event.dataTransfer.files[0]; if (file) void upload(file); }}
          className={`flex flex-wrap items-center gap-3 rounded-lg border border-dashed px-3 py-3 ${over ? 'border-pink-500 bg-pink-500/5' : 'border-zinc-300 dark:border-white/15'}`}>
          {uploading ? (
            <p role="status" className="flex items-center gap-2 text-[13px] text-zinc-600 dark:text-zinc-400"><Loader2 size={13} className="animate-spin" />{text.uploading(uploading)}</p>
          ) : (
            <>
              <SecondaryButton disabled={disabled} onClick={() => picker.current?.click()}><Upload size={13} aria-hidden />{text.chooseFile}</SecondaryButton>
              <span className="text-[13px] text-zinc-600 dark:text-zinc-400">{over ? text.dropNow : text.dropHere} · {text.formats}</span>
            </>
          )}
          <input ref={picker} type="file" className="hidden" accept="audio/*,.wav,.flac,.mp3,.m4a,.ogg,.aiff" aria-label={label}
            onChange={event => { const file = event.target.files?.[0]; if (file) void upload(file); event.target.value = ''; }} />
        </div>
      )}

      {problemText && (
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <p role="alert" className="text-[13px] leading-[18px] text-rose-700 dark:text-rose-300">{problemText}</p>
          {(problem?.code === 'silent' || problem?.code === 'too_short' || problem?.code === 'in_use') && source === 'mic' && (
            <SecondaryButton {...PERSON_ONLY} onClick={record}>{text.recordAgain}</SecondaryButton>
          )}
          {problem?.code === 'no_device' && <SecondaryButton onClick={() => { setProblem(null); void readMic(); }}>{text.checkAgain}</SecondaryButton>}
          {problem?.code === 'upload' && <SecondaryButton onClick={() => picker.current?.click()}>{text.retry}</SecondaryButton>}
        </div>
      )}

      {value && value.origin !== 'library' && phase === 'idle' && (
        <div className="mt-2 flex min-w-0 items-center gap-2 rounded-lg border border-zinc-200 px-2 py-1.5 dark:border-white/10">
          <button type="button" onClick={listen} aria-label={playing ? text.stopListening : text.listen(value.title)}
            className="grid h-9 w-9 shrink-0 place-items-center rounded-full text-zinc-700 hover:bg-zinc-100 dark:text-zinc-200 dark:hover:bg-white/10">
            {playing ? <Pause size={14} /> : <Play size={14} />}
          </button>
          <FileAudio size={14} className="shrink-0 text-zinc-500" aria-hidden />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white" title={value.title}>{value.title}</p>
            <p className="text-[13px] text-zinc-600 dark:text-zinc-400">{value.seconds ? `${time(value.seconds)} · ` : ''}{origin}</p>
          </div>
          {value.origin !== 'mic' && <SecondaryButton disabled={disabled} onClick={() => onChange(null)}>{text.replace}</SecondaryButton>}
        </div>
      )}
    </div>
  );
}
