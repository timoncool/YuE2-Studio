import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { libraryChanged, useActivity, useAssistantStatus, useSetupStatus, type ActivityEntry } from '../services/studioQueries';
import { hasSungLines, karaokeReason } from '../services/karaoke';
import {
  AlertTriangle, AudioLines, ChevronDown, CircleAlert, Dices, Ear, Eye, EyeOff, FileMusic, FolderOpen, Loader2,
  Grid2x2, ListChecks, Music2, Pause, Piano, Play, RotateCcw, Save, Sparkles, Square, Tags, Upload, Wand2, Settings2, X,
} from 'lucide-react';
import { AudioWaveform } from './AudioWaveform';
import type { Playlist, Song, YueCot, YueHarmony, YueOutputFormat, YueRequest, YueSampling } from '../types';
import { useI18n } from '../context/I18nContext';
import { saveFile } from '../services/saveFile';
import { useBridgeCommand } from '../services/mcpBridge';
import { EXAMPLES, randomExample } from '../services/examples';
import { ScoreView } from './ScoreView';
import { MidiEditor } from './midi/MidiEditor';
import { MidiImportDialog } from './MidiImportDialog';
import { isMidiFile } from '../services/midiFiles';
import { trackMidiBase64 } from '../services/midiEditor';
import { composePlans, composeScore, transcribe, type ScorePlan } from '../services/transcription';
import { chordBed, failed, matchSections, type SectionProposal } from '../services/scoreApi';
import { profileLabel as setLabel } from '../services/modelCatalog';
import { REQUEST_FILE_ACCEPT, parseRequestFile, requestFileTitle, serializeRequest, type RequestFileFormat } from '../services/requestFile';
import { AdapterPicker } from './AdapterPicker';
import { CreatePlaylistModal } from './PlaylistModals';
import { SlideToEnable } from './SlideToEnable';
import { usesFromSettings, type AdapterUse } from '../services/adapters';
import { engineParityLabels, planLabels } from '../i18n/engineParity';
import { midiImportOptions } from '../i18n/midiImportOptions';
import { SongTextParts } from './SongTextParts';
import { writerInstruction } from '../services/songWriting';
import { songWritingStrings } from '../i18n/songWriting';

/**
 * The YuE2 request form.
 *
 * Grouped the way the engine works, stage by stage: the prompt (style and
 * lyrics), the score the autoregressive half plans before it writes codes, the
 * semantic stage, and the acoustic side (flow matching and output). A field
 * left empty is the engine's own default, shown as the placeholder, so the
 * request stays sparse exactly like the engine's reference client sends it.
 */

/** Something another page sends to the form: a library track to cover (its
 * recording transcribed, or its MIDI read), or a score to sing. It lives in
 * the app's state, not in a window event, so it still arrives when the form
 * was hidden at the moment it was sent. */
export type CreateRequest =
  | { id: number; kind: 'transcribe'; song: Song; melodyOnly: boolean }
  | { id: number; kind: 'midi'; song: Song }
  | { id: number; kind: 'score'; abc: string; cot?: YueCot; lyrics?: string; title?: string };

interface CreatePanelProps {
  onGenerate: (request: YueRequest) => void;
  isGenerating: boolean;
  activeJobCount?: number;
  initialData?: { song: Song; timestamp: number } | null;
  request?: CreateRequest | null;
  playlists: Playlist[];
  onCreatePlaylist: (name: string, description: string) => Promise<Playlist | null>;
  /** The playlist new songs go into, which the list beside the form shows; empty for none. */
  playlistId: string;
  onChoosePlaylist: (id: string) => void;
}

type EngineDefaults = Partial<Record<string, unknown>> & {
  abc_sampling?: YueSampling;
  semantic_sampling?: YueSampling;
};

type ProfileFiles = { backbone: string; vae: string; transcriber?: string | null };

type SetupStatus = {
  ready?: boolean;
  profile_files?: ProfileFiles | null;
  engine_ready?: boolean;
  selected_profile_id?: string | null;
  selected_component_ids?: string[] | null;
  effective_max_batch?: number;
  hardware?: { gpuName?: string; totalVramGb?: number; recommended?: string };
};

type EngineCatalog = {
  defaults?: EngineDefaults;
  transcriber?: string | null;
  max_batch?: number;
  version?: string;
};

type SamplingText = Record<keyof YueSampling, string>;

/** The select entry that opens the new-playlist dialog instead of choosing. */
const NEW_PLAYLIST = '__new__';

/** The style as it is sent: a trigger's comma with nothing after it goes. */
const finishedStyle = (text: string) => text.trim().replace(/,$/, '').trimEnd();
const SEMANTIC_CODES_PER_SECOND = 25;
/** 9000 semantic frames at 25 per second, the stage's own budget. */
const MAX_DURATION_SECONDS = 600;
/** A new prompt's ceiling: 2:10. Examples and prompt files keep their own. */
const DEFAULT_DURATION_SECONDS = 130;
const SAMPLING_KEYS: (keyof YueSampling)[] = ['temperature', 'top_p', 'top_k', 'repetition_penalty', 'penalty_window', 'min_tokens', 'max_tokens'];
/** A quoted chord symbol in ABC: "Am", "F/C", "G7". */
const CHORD_SYMBOL = /"[^"\n]+"/;
const CHORD_SYMBOLS = /"[^"\n]+"/g;
const BED_KEYS = ['C', 'G', 'D', 'A', 'E', 'B', 'F#', 'Db', 'Ab', 'Eb', 'Bb', 'F', 'Am', 'Em', 'Bm', 'F#m', 'C#m', 'G#m', 'Ebm', 'Bbm', 'Fm', 'Cm', 'Gm', 'Dm'];
const BED_METERS = ['4/4', '3/4', '6/8', '2/4'];
const SECTION_TAGS = ['[Intro]', '[Verse 1]', '[Pre-Chorus]', '[Chorus]', '[Verse 2]', '[Bridge]', '[Instrumental Break]', '[Outro]'];

const PROFILE_LABEL: Record<string, string> = {
  native: 'Full Native · BF16',
  'quality-q8': 'Quality · Q8_0',
  balanced: 'Balanced · Q6_K',
  light: 'Light · Q5_K_M',
};

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 outline-hidden transition-colors focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/25 dark:text-white';
const LABEL = 'mb-1.5 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400';
const ICON =
  'rounded-md p-1.5 text-zinc-400 transition-colors hover:bg-zinc-200 hover:text-black dark:hover:bg-white/10 dark:hover:text-white disabled:opacity-40';
const CHIP =
  'rounded-md border border-zinc-200 px-2 py-0.5 font-mono text-[10px] text-zinc-500 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-400';

const emptySampling = (): SamplingText => ({ temperature: '', top_p: '', top_k: '', repetition_penalty: '', penalty_window: '', min_tokens: '', max_tokens: '' });

const numberOrUndefined = (value: string): number | undefined => {
  const trimmed = value.trim();
  if (trimmed === '') return undefined;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : undefined;
};

const asText = (value: unknown) => (typeof value === 'number' || typeof value === 'string' ? String(value) : '');

const samplingFrom = (text: SamplingText): YueSampling | undefined => {
  const preset: YueSampling = {};
  for (const key of SAMPLING_KEYS) {
    const value = numberOrUndefined(text[key]);
    if (value !== undefined) preset[key] = value;
  }
  return Object.keys(preset).length ? preset : undefined;
};

const samplingText = (value: unknown): SamplingText => {
  const text = emptySampling();
  if (value && typeof value === 'object') {
    for (const key of SAMPLING_KEYS) text[key] = asText((value as Record<string, unknown>)[key]);
  }
  return text;
};

type HarmonyText = { identity: '' | 'root' | 'spelling'; strength: string; window: string; hold_limit: string; outside_bonus: string; section_strength: string; follow_lyrics: boolean };
const HARMONY_NUMBERS = ['strength', 'window', 'hold_limit', 'outside_bonus', 'section_strength'] as const;
const emptyHarmony = (): HarmonyText => ({ identity: '', strength: '', window: '', hold_limit: '', outside_bonus: '', section_strength: '', follow_lyrics: false });

/** The engine's harmony field, or nothing when every control is at rest. */
const harmonyFrom = (text: HarmonyText): YueHarmony | undefined => {
  const harmony: YueHarmony = {};
  for (const key of HARMONY_NUMBERS) {
    const value = numberOrUndefined(text[key]);
    if (value !== undefined) harmony[key] = value;
  }
  if (text.identity) harmony.identity = text.identity;
  if (text.follow_lyrics) harmony.follow_lyrics = true;
  const active = (harmony.strength ?? 0) > 0 || (harmony.outside_bonus ?? 0) > 0 || (harmony.section_strength ?? 0) > 0 || harmony.follow_lyrics === true;
  return active ? harmony : undefined;
};

const harmonyText = (value: unknown): HarmonyText => {
  const text = emptyHarmony();
  if (!value || typeof value !== 'object') return text;
  const stored = value as Record<string, unknown>;
  for (const key of HARMONY_NUMBERS) text[key] = asText(stored[key]);
  if (stored.identity === 'root' || stored.identity === 'spelling') text.identity = stored.identity;
  text.follow_lyrics = stored.follow_lyrics === true;
  return text;
};

const Field: React.FC<{ label: string; hint?: string; children: React.ReactNode }> = ({ label, hint, children }) => (
  <label className="block">
    <span className={LABEL}>{label}</span>
    {children}
    {hint && <span className="mt-1 block text-[11px] leading-4 text-zinc-500">{hint}</span>}
  </label>
);

const Switch: React.FC<{ checked: boolean; onChange: (value: boolean) => void; label: string; hint?: string; disabled?: boolean }> = ({ checked, onChange, label, hint, disabled }) => (
  <div className={`flex items-center justify-between gap-3 ${disabled ? 'opacity-50' : ''}`}>
    <div className="min-w-0">
      <span className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</span>
      {hint && <p className="mt-0.5 text-[11px] leading-4 text-zinc-500">{hint}</p>}
    </div>
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-10 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed ${checked ? 'bg-pink-500' : 'bg-zinc-300 dark:bg-zinc-600'}`}
    >
      <span className={`absolute top-[2px] h-4 w-4 rounded-full bg-white shadow-xs transition-all ${checked ? 'left-[22px]' : 'left-[2px]'}`} />
    </button>
  </div>
);

/** A number you drag. Empty means "engine default", shown until touched. */
const SliderRow: React.FC<{
  label: string;
  value: string;
  fallback: number;
  min: number;
  max: number;
  step: number;
  suffix?: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  format?: (value: number) => string;
}> = ({ label, value, fallback, min, max, step, suffix, onChange, disabled, format }) => {
  const current = value.trim() === '' ? fallback : Number(value);
  const shown = Number.isFinite(current) ? current : fallback;
  const decimals = step < 1 ? String(step).split('.')[1]?.length ?? 1 : 0;
  return (
    <div className={disabled ? 'opacity-50' : undefined}>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</span>
        <span className="text-[11px] tabular-nums text-zinc-600 dark:text-zinc-300">
          {format ? format(shown) : `${shown.toFixed(decimals)}${suffix ?? ''}`}
          {value.trim() === '' && <span className="ml-1 text-zinc-400" title="engine default">·</span>}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={shown}
        disabled={disabled}
        aria-label={label}
        onChange={event => onChange(event.target.value)}
        className="mt-1.5 h-1 w-full cursor-pointer accent-pink-500"
      />
    </div>
  );
};

const Card: React.FC<{ title: string; icon?: React.ReactNode; actions?: React.ReactNode; children: React.ReactNode }> = ({ title, icon, actions, children }) => (
  <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-white/5 dark:bg-suno-card">
    <div className="flex items-center justify-between gap-2 border-b border-zinc-100 bg-zinc-50 px-3 py-2 dark:border-white/5 dark:bg-white/5">
      <span className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{icon}{title}</span>
      {actions && <div className="flex items-center gap-1">{actions}</div>}
    </div>
    <div className="p-3">{children}</div>
  </div>
);

const Stage: React.FC<{ title: string; hint: string; children: React.ReactNode }> = ({ title, hint, children }) => (
  <section>
    <h4 className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{title}</h4>
    <p className="mb-3 mt-0.5 text-[11px] leading-4 text-zinc-500">{hint}</p>
    {children}
  </section>
);

/** Grows with its content up to `maxRows`, then scrolls inside itself. */
const AutoTextarea: React.FC<React.TextareaHTMLAttributes<HTMLTextAreaElement> & { minRows?: number; maxRows?: number }> = ({ minRows = 3, maxRows = 24, value, ...rest }) => {
  const node = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    const element = node.current;
    if (!element) return;
    element.style.height = 'auto';
    element.style.height = `${Math.min(Math.max(element.scrollHeight, minRows * 20), maxRows * 20)}px`;
  }, [value, minRows, maxRows]);
  return <textarea ref={node} value={value} rows={minRows} {...rest} />;
};

/** Seven knobs of one autoregressive stage, each defaulting to the checkpoint. */
const SamplingGrid: React.FC<{ value: SamplingText; defaults?: YueSampling; onChange: (value: SamplingText) => void; t: (key: never) => string; maxTokensCap?: number }> = ({ value, defaults, onChange, t, maxTokensCap }) => {
  const label: Record<keyof YueSampling, string> = {
    temperature: t('samplingTemperature' as never),
    top_p: 'Top P',
    top_k: 'Top K',
    repetition_penalty: t('samplingRepetitionPenalty' as never),
    penalty_window: t('samplingPenaltyWindow' as never),
    min_tokens: t('samplingMinTokens' as never),
    max_tokens: t('samplingMaxTokens' as never),
  };
  return (
    <div className="grid grid-cols-2 gap-2">
      {SAMPLING_KEYS.map(key => (
        <Field key={key} label={label[key]}>
          <input
            value={value[key]}
            onChange={event => onChange({ ...value, [key]: event.target.value })}
            placeholder={key === 'max_tokens' && maxTokensCap !== undefined ? asText(Math.min(Number(defaults?.max_tokens ?? maxTokensCap), maxTokensCap)) : asText(defaults?.[key])}
            inputMode="decimal"
            className={CONTROL}
          />
        </Field>
      ))}
    </div>
  );
};

const NO_ACTIVITY: ActivityEntry[] = [];

export const CreatePanel: React.FC<CreatePanelProps> = ({ onGenerate, isGenerating, activeJobCount = 0, initialData, request, playlists, onCreatePlaylist, playlistId, onChoosePlaylist }) => {
  const { t, language } = useI18n();
  const writingText = songWritingStrings[language];
  const tt = t as unknown as (key: string) => string;

  const [name, setName] = useState('');
  const [style, setStyle] = useState('');
  const [lyrics, setLyrics] = useState('');
  const [abc, setAbc] = useState('');
  const [cot, setCot] = useState<YueCot | ''>('');
  const [showNotation, setShowNotation] = useState(true);

  // Strings, so an empty field can mean "engine default".
  // The ceiling last set by hand, kept between sessions like the playlist:
  // a song made longer once is not cut at 2:10 after the next start.
  const [duration, setDuration] = useState(() => {
    try {
      const kept = Number(window.localStorage.getItem('studio.createDuration'));
      return kept >= 10 && kept <= MAX_DURATION_SECONDS ? String(kept) : String(DEFAULT_DURATION_SECONDS);
    } catch { return String(DEFAULT_DURATION_SECONDS); }
  });
  const chooseDuration = (value: string) => {
    setDuration(value);
    try { window.localStorage.setItem('studio.createDuration', value); } catch { /* kept until a reload */ }
  };
  const [lmBatch, setLmBatch] = useState('');
  const [synthBatch, setSynthBatch] = useState('');
  const [steps, setSteps] = useState('');
  const [solver, setSolver] = useState<'' | 'midpoint' | 'ab2'>('');
  const [cfgScale, setCfgScale] = useState('');
  const [randomizeSeed, setRandomizeSeed] = useState(true);
  const [lmSeed, setLmSeed] = useState('');
  const [seed, setSeed] = useState('');
  const [semanticTokens, setSemanticTokens] = useState('');
  const [saveMenuOpen, setSaveMenuOpen] = useState(false);
  const [takeChoiceOpen, setTakeChoiceOpen] = useState(false);
  const [abcSampling, setAbcSampling] = useState<SamplingText>(emptySampling);
  const [semanticSampling, setSemanticSampling] = useState<SamplingText>(emptySampling);
  const [harmony, setHarmony] = useState<HarmonyText>(emptyHarmony);
  const harmonyField = (key: keyof HarmonyText) => (value: string) => setHarmony(current => ({ ...current, [key]: key === 'follow_lyrics' ? value === 'true' : value }));
  const [transpose, setTranspose] = useState('0');
  const [vocalsOnly, setVocalsOnly] = useState(false);
  const [lyricTiming, setLyricTiming] = useState(true);
  // the decoder companion: auto leaves it to the service (on only under a LoRA trained in the studio)
  const [realaudio, setRealaudio] = useState<'auto' | 'on' | 'off'>('auto');
  // The engine default of 128 kbps throws away what the VAE produced.
  const [mp3Bitrate, setMp3Bitrate] = useState('320');
  const [format, setFormat] = useState<YueOutputFormat>('flac');

  const setupQuery = useSetupStatus<SetupStatus>();
  const setup = setupQuery.isError ? null : setupQuery.data ?? null;
  const serviceDown = setupQuery.isError;
  const [catalog, setCatalog] = useState<EngineCatalog | null>(null);
  const assistantQuery = useAssistantStatus<{ available?: boolean }>();
  const assistantReady = !assistantQuery.isError && assistantQuery.data?.available === true;
  const [assisting, setAssisting] = useState<'all' | 'lyrics' | 'style' | 'score' | 'sections' | null>(null);
  const [assistStage, setAssistStage] = useState<string | null>(null);
  const [assistModel, setAssistModel] = useState<string | null>(null);
  const [assistDraft, setAssistDraft] = useState('');
  const [assistSeconds, setAssistSeconds] = useState(0);
  const [coverPrompt, setCoverPrompt] = useState('');
  const [adapters, setAdapters] = useState<AdapterUse[]>([]);

  // A picked LoRA's trigger word leads the style; removing the LoRA takes it out again.
  const applyTrigger = useCallback((word: string, present: boolean) => {
    setStyle(current => {
      const parts = current.split(',').map(part => part.trim()).filter(Boolean);
      const has = parts.some(part => part.toLowerCase() === word.toLowerCase());
      // into an empty style the word comes with its comma, so what is typed next stays apart
      if (present) return has ? current : parts.length ? [word, ...parts].join(', ') : `${word}, `;
      return has ? parts.filter(part => part.toLowerCase() !== word.toLowerCase()).join(', ') : current;
    });
  }, []);
  const activityQuery = useActivity();
  const activity = activityQuery.data ?? NO_ACTIVITY;
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [mode, setMode] = useState<'studio' | 'simple' | 'cover'>('studio');
  const [assistInstruction, setAssistInstruction] = useState('');
  const [writerLanguage, setWriterLanguage] = useState('');
  const [writerLines, setWriterLines] = useState('');
  const [writerExtra, setWriterExtra] = useState('');
  const [scoreInstruction, setScoreInstruction] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [examplesOpen, setExamplesOpen] = useState(false);
  const [transcribing, setTranscribing] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);
  const [planCount, setPlanCount] = useState('1');
  const [plans, setPlans] = useState<ScorePlan[]>([]);
  const [selectedPlan, setSelectedPlan] = useState(0);
  const composeRun = useRef<AbortController | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [continueScore, setContinueScore] = useState(false);
  const [bedOpen, setBedOpen] = useState(false);
  const [bedBpm, setBedBpm] = useState('100');
  const [bedKey, setBedKey] = useState('C');
  const [bedMeter, setBedMeter] = useState('4/4');
  const [bedBusy, setBedBusy] = useState(false);
  const [bedWritten, setBedWritten] = useState<string[][] | null>(null);
  const [coverSource, setCoverSource] = useState<string>('');
  // the library song a cover's melody was taken from, which the new song names
  const [coverSongId, setCoverSongId] = useState<string | null>(null);
  // The recording being covered, so it can be listened to next to its score.
  const [coverAudio, setCoverAudio] = useState<string | null>(null);
  const [coverFile, setCoverFile] = useState<File | null>(null);
  const [sectionProposal, setSectionProposal] = useState<SectionProposal | null>(null);
  const [matchingSections, setMatchingSections] = useState(false);
  // A proposal is for the lyrics, score and recording it was made from; any change makes it stale, a run still going too.
  const sectionRun = useRef(0);
  useEffect(() => { sectionRun.current += 1; setSectionProposal(null); }, [abc, lyrics, coverSongId, coverFile]);
  const coverPlayer = useRef<HTMLAudioElement | null>(null);
  const [coverPlaying, setCoverPlaying] = useState(false);
  const [coverTime, setCoverTime] = useState(0);
  const [coverDuration, setCoverDuration] = useState(0);
  const [coverMelodyOnly, setCoverMelodyOnly] = useState(true);
  // a track's MIDI sent to a cover, waiting in the MIDI import for its parts to be chosen
  const [midiCover, setMidiCover] = useState<{ name: string; data: string } | null>(null);
  const promptFile = useRef<HTMLInputElement | null>(null);
  const scoreFile = useRef<HTMLInputElement | null>(null);
  const audioFile = useRef<HTMLInputElement | null>(null);
  const lyricsBox = useRef<HTMLTextAreaElement | null>(null);

  const ready = setup?.ready === true && setup?.engine_ready === true;
  const defaults: EngineDefaults = catalog?.defaults ?? {};
  const placeholder = (key: string) => asText(defaults[key]);
  const maxBatch = Math.max(1, catalog?.max_batch ?? setup?.effective_max_batch ?? 1);
  const transcriberReady = Boolean(catalog?.transcriber);
  const effectiveCot: YueCot = cot || (defaults.cot as YueCot) || 'full';

  const profileLabel = useMemo(() => {
    if (setup?.selected_component_ids?.length) return t('customSet');
    const id = setup?.selected_profile_id;
    return id ? PROFILE_LABEL[id] ?? id : '—';
  }, [setup, t]);

  // A cover or a lyric timing finished: the library is read again for it.
  // What was finished before the page opened is in the library already.
  const finishedWork = useRef<string | null>(null);
  // Failures already on the server when the page opened are old news; only
  // one that happens while the studio is open is told, once, as a toast.
  const reportedFailures = useRef<Set<string> | null>(null);
  useEffect(() => {
    const entries = activityQuery.data;
    if (!entries) return;
    const done = entries.filter(entry => entry.state === 'done').map(entry => `${entry.song_id}:${entry.kind}`).join(',');
    if (finishedWork.current !== null && done !== finishedWork.current) libraryChanged();
    finishedWork.current = done;
    const failed = entries.filter(entry => entry.state === 'failed');
    const keyOf = (entry: ActivityEntry) => `${entry.song_id}:${entry.kind}:${entry.detail ?? ''}`;
    if (reportedFailures.current === null) {
      reportedFailures.current = new Set(failed.map(keyOf));
      return;
    }
    for (const entry of failed) {
      const key = keyOf(entry);
      if (reportedFailures.current.has(key)) continue;
      reportedFailures.current.add(key);
      const what = entry.kind === 'cover' ? t('coverArt') : t('karaokeSection');
      const why = karaokeReason(tt, entry.detail) ?? '';
      window.dispatchEvent(new CustomEvent('yue:toast', { detail: { message: `${what} · ${entry.title}: ${why}`, type: 'info' } }));
    }
  }, [activityQuery.data, t, tt]);

  useEffect(() => {
    if (!assisting) return;
    setAssistSeconds(0);
    const started = Date.now();
    const timer = window.setInterval(() => setAssistSeconds(Math.round((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, [assisting]);


  useEffect(() => {
    void fetch('/v1/local-models/music')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((body: { catalog?: EngineCatalog }) => setCatalog(body.catalog ?? null))
      .catch(() => setCatalog(null));
  }, [setup?.engine_ready, setup?.selected_profile_id]);


  /** Fills the form from a stored request: a reused track, a file, an example. */
  const applyRequest = useCallback((request: Record<string, unknown>, title?: string) => {
    if (title !== undefined) setName(title);
    if (typeof request.style === 'string') setStyle(request.style);
    if (typeof request.lyrics === 'string') setLyrics(request.lyrics);
    setAbc(typeof request.abc === 'string' ? request.abc.trimEnd() : '');
    setPlans([]); setSelectedPlan(0);
    setCot(request.cot === 'full' || request.cot === 'melody' || request.cot === 'off' ? request.cot : '');
    setDuration(asText(request.duration ?? request.duration_seconds));
    setLmBatch('');
    setSynthBatch('');
    setSteps(asText(request.steps));
    setSolver(request.solver === 'ab2' || request.solver === 'midpoint' ? request.solver : '');
    setCfgScale(typeof request.cfg_scale === 'number' && request.cfg_scale >= 0 ? String(request.cfg_scale) : '');
    const safeSeed = (value: unknown) => (typeof value === 'number' && Number.isSafeInteger(value) ? String(value) : '');
    const storedLmSeed = safeSeed(request.lm_seed);
    const storedSeed = safeSeed(request.seed);
    setLmSeed(storedLmSeed === '-1' ? '' : storedLmSeed);
    setSeed(storedSeed === '-1' ? '' : storedSeed);
    setRandomizeSeed(!(storedLmSeed && storedLmSeed !== '-1'));
    setSemanticTokens(typeof request.semantic_tokens === 'string' ? request.semantic_tokens : '');
    setAbcSampling(samplingText(request.abc_sampling));
    setSemanticSampling(samplingText(request.semantic_sampling));
    setHarmony(harmonyText(request.harmony));
    setContinueScore(request.abc_continue === true);
    setTranspose(asText(request.transpose) || '0');
    setVocalsOnly(request.vocals_only === true);
    setLyricTiming(request.lyric_timing !== false);
    setRealaudio(request.companion_scale === undefined ? 'auto' : request.companion_scale === 0 ? 'off' : 'on');
    if (request.mp3_bitrate !== undefined) setMp3Bitrate(asText(request.mp3_bitrate));
    if (request.output_format === 'mp3' || request.output_format === 'flac') setFormat(request.output_format);
    if (Array.isArray(request.adapters)) setAdapters(usesFromSettings(request));
    setError(null);
  }, []);

  useEffect(() => {
    if (!initialData?.song) return;
    const song = initialData.song;
    const settings = (song.generationParams ?? {}) as Record<string, unknown>;
    applyRequest({ style: song.style, lyrics: song.lyrics, ...settings }, song.title || '');
    // A song made without LoRA reuses without it, whatever was picked before.
    setAdapters(usesFromSettings(settings));
    setMode('studio');
  }, [initialData, applyRequest]);

  useEffect(() => {
    setCoverPlaying(false);
    setCoverTime(0);
    setCoverDuration(0);
    return () => { if (coverAudio?.startsWith('blob:')) URL.revokeObjectURL(coverAudio); };
  }, [coverAudio]);

  // A request from another page is applied once, by its id: the form's effects
  // run again each time it is shown, and must not repeat what they already did.
  const appliedRequest = useRef<number | null>(null);
  useEffect(() => {
    if (!request || appliedRequest.current === request.id) return;
    appliedRequest.current = request.id;
    if (request.kind === 'transcribe') {
      // a library track to cover: its recording becomes the score, and its own
      // words start the lyric sheet when there is nothing there yet
      const { song, melodyOnly } = request;
      setCoverSource(song.title);
      setCoverSongId(song.id);
      setCoverFile(null);
      setCoverAudio(song.audioUrl ?? null);
      if (song.lyrics?.trim()) setLyrics(current => (current.trim() ? current : song.lyrics));
      setMode('cover');
      void runTranscription({ songId: song.id }, melodyOnly);
    } else if (request.kind === 'midi') {
      // a track made or edited in the MIDI editor: its MIDI becomes the score,
      // through the MIDI import, where the voice's part is chosen
      const { song } = request;
      setCoverSource(song.title);
      setCoverSongId(song.id);
      setCoverFile(null);
      setCoverAudio(song.audioUrl ?? null);
      if (song.lyrics?.trim()) setLyrics(current => (current.trim() ? current : song.lyrics));
      setMode('cover');
      setError(null);
      trackMidiBase64(song.id)
        .then(data => setMidiCover({ name: `${song.title}.mid`, data }))
        .catch(reason => setError(reason instanceof Error ? reason.message : String(reason)));
    } else {
      // a score from elsewhere: a transcribed library track, an edited plan
      setAbc(request.abc.trimEnd());
      if (request.cot) setCot(request.cot);
      if (request.lyrics) setLyrics(current => (current.trim() ? current : request.lyrics ?? ''));
      if (request.title) setName(current => (current.trim() ? current : request.title ?? ''));
      setSemanticTokens('');
      setMode('studio');
    }
    // runTranscription is the one from the render that received the request
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request]);

  const reset = () => {
    setName(''); setStyle(''); setLyrics(''); setAbc(''); setCot(''); setContinueScore(false);
    setPlans([]); setSelectedPlan(0);
    resetParameters();
    setCoverPrompt('');
    setAdapters([]);
    setError(null);
  };

  const resetParameters = () => {
    chooseDuration(String(DEFAULT_DURATION_SECONDS)); setLmBatch(''); setSynthBatch(''); setSteps(''); setSolver(''); setCfgScale('');
    setRandomizeSeed(true); setLmSeed(''); setSeed(''); setSemanticTokens('');
    setAbcSampling(emptySampling()); setSemanticSampling(emptySampling()); setHarmony(emptyHarmony());
    setMp3Bitrate('320'); setFormat('flac');
    setTranspose('0'); setVocalsOnly(false); setLyricTiming(true); setRealaudio('auto');
  };

  const loadExample = (id?: string) => {
    const example = (id && EXAMPLES.find(entry => entry.id === id)) || randomExample();
    applyRequest({ style: example.style, lyrics: example.lyrics, cot: example.cot, abc: example.abc }, example.title);
    setExamplesOpen(false);
  };

  // A saved prompt keeps the seeds only when they were pinned: with the random
  // switch on, the file stays a prompt rather than one particular take.
  const buildRequest = (forFile = false): YueRequest => {
    const request: YueRequest = {
      style: finishedStyle(style),
      lyrics: lyrics.replace(/\r\n?/g, '\n').trim(),
      output_format: format,
    };
    if (abc.trim() && effectiveCot !== 'off') request.abc = abc.trim();
    if (abc.trim() && effectiveCot !== 'off' && !semanticTokens.trim() && Number(transpose) !== 0) request.transpose = Number(transpose);
    if (abc.trim() && effectiveCot !== 'off' && !lyricTiming) request.lyric_timing = false;
    if (realaudio !== 'auto') request.companion_scale = realaudio === 'on' ? 1 : 0;
    if (vocalsOnly) request.vocals_only = true;
    if (cot) request.cot = cot;
    const durationValue = numberOrUndefined(duration);
    if (durationValue !== undefined) request.duration_seconds = Math.min(Math.max(durationValue, 1), MAX_DURATION_SECONDS);
    const lmBatchValue = numberOrUndefined(lmBatch);
    if (lmBatchValue !== undefined) request.lm_batch_size = lmBatchValue;
    const synthBatchValue = numberOrUndefined(synthBatch);
    if (synthBatchValue !== undefined) request.synth_batch_size = synthBatchValue;
    const stepsValue = numberOrUndefined(steps);
    if (stepsValue !== undefined) request.steps = stepsValue;
    if (solver) request.solver = solver;
    const cfgValue = numberOrUndefined(cfgScale);
    if (cfgValue !== undefined) request.cfg_scale = cfgValue;
    // Seeds are drawn here, within 32 bits, so the stored request replays the
    // exact track: the engine's own random draw is 64-bit and does not survive
    // a JavaScript number.
    const randomSeed = () => Math.floor(Math.random() * 0x100000000);
    const lmSeedValue = randomizeSeed ? undefined : numberOrUndefined(lmSeed);
    const seedValue = randomizeSeed ? undefined : numberOrUndefined(seed);
    const pinned = (value: number | undefined) => value !== undefined && value >= 0;
    if (!forFile || pinned(lmSeedValue)) request.lm_seed = pinned(lmSeedValue) ? lmSeedValue : randomSeed();
    if (!forFile || pinned(seedValue)) request.seed = pinned(seedValue) ? seedValue : randomSeed();
    if (semanticTokens.trim()) request.semantic_tokens = semanticTokens.trim();
    if (!forFile && mode === 'cover' && coverSongId && request.abc) request.cover_of = coverSongId;
    const abcPreset = samplingFrom(abcSampling);
    if (abcPreset) request.abc_sampling = abcPreset;
    const semanticPreset = samplingFrom(semanticSampling);
    if (semanticPreset) request.semantic_sampling = semanticPreset;
    const harmonyValue = harmonyFrom(harmony);
    if (harmonyValue) request.harmony = harmonyValue;
    if (continueScore && request.abc) request.abc_continue = true;
    const bitrate = numberOrUndefined(mp3Bitrate);
    if (bitrate !== undefined && format === 'mp3') request.mp3_bitrate = bitrate;
    if (name.trim()) request.title = name.trim();
    if (coverPrompt.trim()) request.cover_prompt = coverPrompt.trim();
    if (adapters.length > 0) request.adapters = adapters.filter(row => !row.disabled).map(({ id, scales }) => ({ id, scales }));
    if (!forFile && chosenPlaylist) request.playlist_id = chosenPlaylist;
    return request;
  };

  const download = (filename: string, text: string, type: string) => void saveFile(filename, { blob: new Blob([text], { type }) });

  const safeName = () => (name.trim() || 'request').replace(/[\\/:*?"<>|]/g, '');

  const savePrompt = (format: RequestFileFormat) => {
    setSaveMenuOpen(false);
    download(`${safeName()}.${format}`, serializeRequest(buildRequest(true), format), format === 'json' ? 'application/json' : 'application/x-yaml');
  };

  const openPrompt = async (file: File) => {
    try {
      const parsed = parseRequestFile(file.name, await file.text());
      applyRequest(parsed, requestFileTitle(file.name, parsed));
      setError(null);
    } catch {
      setError(t('promptFileInvalid'));
    }
  };

  const openScore = async (file: File) => {
    const text = await file.text();
    if (!/K:/.test(text)) { setError(tt('scoreFileInvalid')); return; }
    setAbc(text.trimEnd());
    setError(null);
  };

  const insertTag = (tag: string) => {
    const box = lyricsBox.current;
    const at = box?.selectionStart ?? lyrics.length;
    const before = lyrics.slice(0, at);
    const after = lyrics.slice(at);
    const prefix = before && !before.endsWith('\n\n') ? (before.endsWith('\n') ? '\n' : '\n\n') : '';
    setLyrics(`${before}${prefix}${tag}\n${after}`);
    // the caret follows the tag, so tags pressed in turn stay in that order
    const caret = before.length + prefix.length + tag.length + 1;
    window.requestAnimationFrame(() => {
      box?.focus();
      box?.setSelectionRange(caret, caret);
    });
  };

  /** Reads a recording into a score with SheetSage2, for a cover. */
  const runTranscription = async (source: { file?: File; songId?: string }, melodyOnly: boolean) => {
    setError(null);
    setTranscribing(source.file?.name ?? source.songId ?? '');
    try {
      const score = await transcribe(source, melodyOnly);
      setAbc(score.trimEnd());
      // The model card: covers take a melody-only score in melody mode.
      setCot(melodyOnly ? 'melody' : 'full');
      setSemanticTokens('');
      setShowNotation(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setTranscribing(null);
    }
  };

  /** The score the model writes from the form's style and lyrics, nothing sung. */
  const writeScoreAlone = (signal: AbortSignal) => {
    if (!ready) throw new Error(t('downloadProfileFirst'));
    if (!style.trim() && !lyrics.trim()) throw new Error(tt('styleOrLyricsRequired'));
    const pinned = randomizeSeed ? undefined : numberOrUndefined(lmSeed);
    return composeScore({
      style: finishedStyle(style),
      lyrics: lyrics.replace(/\r\n?/g, '\n').trim(),
      cot: effectiveCot,
      lmSeed: pinned !== undefined && pinned >= 0 ? pinned : undefined,
      abcSampling: samplingFrom(abcSampling),
      harmony: harmonyFrom(harmony),
      opening: continueScore && abc.trim() ? abc : undefined,
    }, signal);
  };

  /** Writes the score alone, so it can be read and edited before a song is sung from it. */
  const runComposition = async () => {
    if (!ready) { setError(t('downloadProfileFirst')); return; }
    if (!style.trim() && !lyrics.trim()) { setError(tt('styleOrLyricsRequired')); return; }
    setError(null);
    setComposing(true);
    const controller = new AbortController();
    composeRun.current = controller;
    try {
      const pinned = randomizeSeed ? undefined : numberOrUndefined(lmSeed);
      const choices = await composePlans({ style: finishedStyle(style), lyrics: lyrics.replace(/\r\n?/g, '\n').trim(), cot: effectiveCot, lmSeed: pinned, abcSampling: samplingFrom(abcSampling), harmony: harmonyFrom(harmony), opening: continueScore && abc.trim() ? abc : undefined, count: Math.max(1, Math.min(Number(planCount), maxBatch, 9)) }, controller.signal);
      setPlans(choices); setSelectedPlan(0);
      setAbc(choices[0].abc);
      setContinueScore(false);
      setSemanticTokens('');
      setShowNotation(true);
    } catch (reason) {
      if (!(reason instanceof DOMException && reason.name === 'AbortError')) {
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      composeRun.current = null;
      setComposing(false);
    }
  };

  // Without an assistant the wands open its settings: hidden, they left no
  // sign that the studio can write a style or lyrics at all.
  const openAssistantSetup = () => window.dispatchEvent(new CustomEvent('yue:open-settings', { detail: 'models:assistant' }));

  const assistRun = useRef<AbortController | null>(null);
  const stopAssistant = () => {
    assistRun.current?.abort();
    assistRun.current = null;
    setAssisting(null);
    setAssistStage(null);
    setAssistDraft('');
  };

  const askAssistant = async (target: 'all' | 'lyrics' | 'style' | 'score') => {
    if (!assistantReady || assisting) return;
    const run = new AbortController();
    assistRun.current = run;
    setAssisting(target);
    setError(null);
    // An idea in the simple mode is a new song: nothing left in the form from
    // the last one - its words, its sound, its score - is carried into it.
    const freshSong = mode === 'simple' && target === 'all';
    if (freshSong) {
      setAbc('');
      setSemanticTokens('');
    }
    try {
      const payload = JSON.stringify({
        target,
        description: freshSong ? '' : name.trim(),
        instruction: target === 'score' ? scoreInstruction.trim() : freshSong ? writerInstruction(assistInstruction, writerLanguage, writerLines, writerExtra) : assistInstruction.trim(),
        lyrics: freshSong ? '' : lyrics.trim(),
        style: freshSong ? '' : finishedStyle(style),
        abc: freshSong ? '' : abc.trim(),
        duration_seconds: numberOrUndefined(duration) ?? 120,
      });
      setAssistStage('preparing');
      setAssistDraft('');
      let streamed = '';
      const live = await fetch('/v1/assistant/write/stream', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: payload,
        signal: run.signal,
      });
      if (!live.ok || !live.body) {
        const refused = await live.json().catch(() => null);
        throw new Error(refused?.error || String(live.status));
      }
      let body: Record<string, unknown> | null = null;
      {
        const reader = live.body.getReader();
        const decoder = new TextDecoder();
        let carry = '';
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          carry += decoder.decode(value, { stream: true });
          let split = carry.indexOf('\n\n');
          while (split !== -1) {
            const frame = carry.slice(0, split).trim();
            carry = carry.slice(split + 2);
            split = carry.indexOf('\n\n');
            if (!frame.startsWith('data:')) continue;
            let event: { stage?: string; delta?: string; text?: string; error?: string; model?: string; draft?: Record<string, unknown> };
            try {
              event = JSON.parse(frame.slice(5).trim());
            } catch {
              continue;
            }
            if (event.error) throw new Error(event.error);
            if (event.stage) setAssistStage(event.stage);
            if (event.draft) body = event.draft;
            if (event.model) setAssistModel(event.model);
            if (event.delta) {
              streamed += event.delta;
              setAssistDraft(streamed);
            }
          }
        }
      }
      if (!body) throw new Error(t('assistantNoAnswer'));
      if (typeof body?.lyrics === 'string') setLyrics(body.lyrics);
      if (typeof body?.style === 'string') setStyle(body.style);
      if (typeof body?.abc === 'string') setAbc(body.abc.trimEnd());
      if (typeof body?.title === 'string' && body.title.trim() && target !== 'score') setName(body.title.trim());
      if (typeof body?.cover_prompt === 'string' && body.cover_prompt.trim()) setCoverPrompt(body.cover_prompt.trim());
      if (typeof body?.duration_seconds === 'number' && body.duration_seconds >= 10 && target === 'all') {
        setDuration(String(Math.min(MAX_DURATION_SECONDS, Math.round(body.duration_seconds))));
      }
    } catch (reason) {
      const cancelled = reason instanceof DOMException && reason.name === 'AbortError';
      if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      assistRun.current = null;
      setAssisting(null);
      setAssistStage(null);
      setAssistDraft('');
    }
  };

  // Tags the lyrics already in the form: the assistant marks where each
  // section starts, the words stay exactly as written.
  const layOutLyrics = async () => {
    if (!assistantReady || assisting || !lyrics.trim()) return;
    const run = new AbortController();
    assistRun.current = run;
    setAssisting('sections');
    setError(null);
    try {
      const response = await fetch('/v1/assistant/sections', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ lyrics }),
        signal: run.signal,
      });
      const body = await response.json().catch(() => null);
      if (!response.ok || typeof body?.lyrics !== 'string') throw new Error(body?.error || String(response.status));
      setLyrics(body.lyrics);
    } catch (reason) {
      const cancelled = reason instanceof DOMException && reason.name === 'AbortError';
      if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      assistRun.current = null;
      setAssisting(null);
    }
  };

  // A cover of a recording that came without a style: MOSS-Music hears the
  // source and the style is written from that.
  const [describing, setDescribing] = useState(false);
  const describeByEar = async () => {
    if (!coverSongId) return;
    setDescribing(true);
    setError(null);
    try {
      const response = await fetch(`/v1/library/songs/${encodeURIComponent(coverSongId)}/describe-style`, { method: 'POST' });
      const body = await response.json().catch(() => null);
      if (!response.ok || typeof body?.style !== 'string') throw new Error(body?.error || `HTTP ${response.status}`);
      setStyle(body.style);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setDescribing(false);
    }
  };

  const [newPlaylistOpen, setNewPlaylistOpen] = useState(false);
  const choosePlaylist = onChoosePlaylist;
  // a playlist deleted elsewhere is no longer offered or sent
  const chosenPlaylist = playlists.some(entry => entry.id === playlistId) ? playlistId : '';

  // Without stopping: the request as it was when the button was pressed goes
  // again with new seeds whenever the queue runs low, until switched off. A
  // snapshot, so editing the form meanwhile does not change the next songs.
  const [forever, setForever] = useState(false);
  const foreverRequest = useRef<YueRequest | null>(null);
  useEffect(() => {
    const stop = () => setForever(false);
    window.addEventListener('yue:cancel-all', stop);
    return () => window.removeEventListener('yue:cancel-all', stop);
  }, []);
  const generate = (request: YueRequest) => {
    if (forever) foreverRequest.current = { ...request };
    onGenerate(request);
  };
  useEffect(() => {
    if (!forever) { foreverRequest.current = null; return; }
    const snapshot = foreverRequest.current;
    if (!snapshot || activeJobCount >= 2) return;
    onGenerate({ ...snapshot, lm_seed: Math.floor(Math.random() * 0x100000000), seed: Math.floor(Math.random() * 0x100000000) });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [forever, activeJobCount]);

  const submit = () => {
    if (!ready) { setError(t('downloadProfileFirst')); return; }
    if (!style.trim() && !lyrics.trim() && !semanticTokens.trim()) { setError(tt('styleOrLyricsRequired')); return; }
    setError(null);
    // Audio codes hold a performance already sung: ask whether to render that
    // take again or perform the prompt anew, as the engine's own WebUI does.
    if (semanticTokens.trim()) { setTakeChoiceOpen(true); return; }
    generate(buildRequest());
  };

  const renderTake = (fresh: boolean) => {
    setTakeChoiceOpen(false);
    const request = buildRequest();
    if (fresh) {
      delete request.semantic_tokens;
      request.lm_seed = Math.floor(Math.random() * 0x100000000);
      request.seed = Math.floor(Math.random() * 0x100000000);
      setSemanticTokens('');
    }
    generate(request);
  };

  // An agent connected over MCP reads and fills this form as the user sees it
  const formFields: Record<string, [unknown, (value: string) => void]> = {
    title: [name, setName],
    style: [style, setStyle],
    lyrics: [lyrics, setLyrics],
    abc: [abc, setAbc],
    cot: [cot, (value) => setCot(value as YueCot | '')],
    duration_seconds: [duration, setDuration],
    lm_batch_size: [lmBatch, setLmBatch],
    synth_batch_size: [synthBatch, setSynthBatch],
    steps: [steps, setSteps],
    solver: [solver, value => setSolver(value === 'ab2' || value === 'midpoint' ? value : '')],
    cfg_scale: [cfgScale, setCfgScale],
    lm_seed: [lmSeed, setLmSeed],
    seed: [seed, setSeed],
    cover_prompt: [coverPrompt, setCoverPrompt],
    output_format: [format, (value) => setFormat(value as YueOutputFormat)],
    mp3_bitrate: [mp3Bitrate, setMp3Bitrate],
    transpose: [transpose, setTranspose],
    vocals_only: [vocalsOnly, value => setVocalsOnly(value === 'true')],
    lyric_timing: [lyricTiming, value => setLyricTiming(value !== 'false')],
    chord_strength: [harmony.strength, harmonyField('strength')],
    chord_identity: [harmony.identity, harmonyField('identity')],
    chord_window: [harmony.window, harmonyField('window')],
    chord_hold_limit: [harmony.hold_limit, harmonyField('hold_limit')],
    outside_bonus: [harmony.outside_bonus, harmonyField('outside_bonus')],
    section_strength: [harmony.section_strength, harmonyField('section_strength')],
    follow_sections: [harmony.follow_lyrics, harmonyField('follow_lyrics')],
    continue_score: [continueScore, value => setContinueScore(value === 'true')],
    companion_scale: [realaudio === 'auto' ? '' : realaudio === 'on' ? '1' : '0', value => setRealaudio(value === '' ? 'auto' : Number(value) !== 0 ? 'on' : 'off')],
  };
  useBridgeCommand('create_get', () => ({
    mode,
    fields: { ...Object.fromEntries(Object.entries(formFields).map(([key, [value]]) => [key, value])), randomize_seed: randomizeSeed, adapters },
    request: buildRequest(),
    ready,
    error,
    assistant_writing: assisting,
  }));
  useBridgeCommand('create_set', (args) => {
    const fields = (args.fields && typeof args.fields === 'object' ? args.fields : args) as Record<string, unknown>;
    // every field is checked before any changes, so a refused call leaves the form as it was
    const choices: Record<string, string[]> = { mode: ['studio', 'simple', 'cover'], cot: ['full', 'melody', 'off', ''], output_format: ['flac', 'mp3'] };
    const unknown = Object.keys(fields).filter(key => !formFields[key] && !['mode', 'randomize_seed', 'adapters'].includes(key));
    if (unknown.length) throw new Error(`Unknown fields: ${unknown.join(', ')}. The form has: ${[...Object.keys(formFields), 'mode', 'randomize_seed', 'adapters'].join(', ')}.`);
    for (const [key, allowed] of Object.entries(choices)) {
      if (key in fields && !allowed.includes(String(fields[key] ?? ''))) throw new Error(`${key} is one of: ${allowed.filter(Boolean).join(', ')}.`);
    }
    if ('adapters' in fields && !Array.isArray(fields.adapters)) throw new Error('adapters is a list of {id, scales}.');
    for (const [key, value] of Object.entries(fields)) {
      if (key === 'mode') setMode(value as 'studio' | 'simple' | 'cover');
      else if (key === 'randomize_seed') setRandomizeSeed(Boolean(value));
      else if (key === 'adapters') setAdapters(value as AdapterUse[]);
      else formFields[key][1](value == null ? '' : String(value));
    }
    return { text: 'Filled in; create_form_get shows the form, ui_screenshot shows it on screen.' };
  });
  useBridgeCommand('create_submit', () => {
    submit();
    return { text: 'Pressed Create. studio_status shows the new job; if the form refused, create_form_get says why under error.' };
  });

  const songs = numberOrUndefined(lmBatch) ?? 1;
  const variations = numberOrUndefined(synthBatch) ?? 1;
  const totalTracks = songs * variations;
  const durationFallback = Number(defaults.duration ?? MAX_DURATION_SECONDS);
  const formatDuration = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.round(seconds % 60)).padStart(2, '0')}`;
  // The engine caps the audio-code stage at duration x 25 codes: what a
  // larger max tokens would ask for is never drawn.
  const effectiveDuration = Math.min(Math.max(numberOrUndefined(duration) ?? durationFallback, 1), MAX_DURATION_SECONDS);
  const semanticBudget = Math.round(effectiveDuration * SEMANTIC_CODES_PER_SECOND);
  const semanticMaxTokens = numberOrUndefined(semanticSampling.max_tokens);
  const budgetText = (key: string) => tt(key).replace('{frames}', String(semanticBudget)).replace('{time}', formatDuration(effectiveDuration));
  const scoreDisabled = effectiveCot === 'off';
  // The model card: melody mode does not strip chord symbols by itself.
  const melodyWithChords = effectiveCot === 'melody' && CHORD_SYMBOL.test(abc);

  return (
    <section className="flex h-full min-h-0 w-full flex-col overflow-hidden bg-zinc-50 text-zinc-900 dark:bg-suno-panel dark:text-white">
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain custom-scrollbar">
        <div className="space-y-3 p-4 pb-6">
          <div className="flex items-center justify-between gap-3">
            <div className="min-w-0">
              <h1 className="truncate text-base font-bold">{t('createMusic')}</h1>
              <p className="mt-0.5 truncate text-[11px] text-zinc-500 dark:text-zinc-400">{tt('localInferenceYue')}</p>
            </div>
            <span className={`shrink-0 rounded-full px-2.5 py-1 text-[10px] font-semibold ${ready ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300' : 'bg-amber-500/10 text-amber-700 dark:text-amber-300'}`}>
              <span className={`mr-1 inline-block h-1.5 w-1.5 rounded-full ${ready ? 'bg-emerald-500' : 'bg-amber-500'}`} />
              {serviceDown ? t('serviceUnavailable') : ready ? t('engineReady') : t('profileRequired')}
            </span>
          </div>

          {serviceDown ? (
            <div className="flex gap-2 rounded-xl border border-rose-500/30 bg-rose-500/10 p-3 text-xs leading-5 text-rose-700 dark:text-rose-200">
              <CircleAlert className="mt-0.5 shrink-0" size={15} />
              <div><b>{t('serviceUnavailable')}</b><br />{t('serviceUnavailableHint')}</div>
            </div>
          ) : !ready && (
            <div className="flex gap-2 rounded-xl border border-amber-500/25 bg-amber-500/10 p-3 text-xs leading-5 text-amber-800 dark:text-amber-200">
              <CircleAlert className="mt-0.5 shrink-0" size={15} />
              <div><b>{t('localGenerationUnavailable')}</b><br />{t('downloadProfileFirst')}</div>
            </div>
          )}

          <div className="flex items-center rounded-lg border border-zinc-300 bg-zinc-200 p-1 dark:border-white/5 dark:bg-black/40" role="tablist">
            {(['studio', 'cover', 'simple'] as const).map(value => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-selected={mode === value}
                onClick={() => setMode(value)}
                className={`flex-1 rounded-md py-1.5 text-xs font-semibold transition-all ${mode === value ? 'bg-white text-black shadow-xs dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}
              >
                {value === 'studio' ? t('studioMode') : value === 'cover' ? tt('coverMode') : t('simpleMode')}
              </button>
            ))}
          </div>

          {mode === 'simple' && !assistantReady && (
            <Card title={t('songIdea')}>
              <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('assistantNeedsModel')}</p>
              <p className="mt-2 text-[11px] leading-4 text-zinc-500">{t('assistantHint')}</p>
              <button
                type="button"
                onClick={() => window.dispatchEvent(new CustomEvent('yue:open-settings', { detail: 'models:assistant' }))}
                className="mt-3 inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
              >
                <Settings2 size={13} />
                {t('setUpAssistant')}
              </button>
            </Card>
          )}

          {mode === 'simple' && assistantReady && (
            <Card title={t('songIdea')}>
              <AutoTextarea
                value={assistInstruction}
                minRows={3}
                onChange={event => setAssistInstruction(event.target.value)}
                placeholder={t('songIdeaPlaceholder')}
                className={`${CONTROL} resize-none`}
              />
              <p className="mt-2 text-[11px] leading-4 text-zinc-500">{t('songIdeaHint')}</p>
              <div className="mt-3 grid grid-cols-2 gap-2">
                <Field label={writingText.language}><input className={CONTROL} value={writerLanguage} onChange={event => setWriterLanguage(event.target.value)} placeholder={writingText.auto} /></Field>
                <Field label={writingText.lines}><select className={CONTROL} value={writerLines} onChange={event => setWriterLines(event.target.value)}><option value="">{writingText.auto}</option>{[8, 16, 24, 32].map(lines => <option key={lines} value={lines}>{lines}</option>)}</select></Field>
              </div>
              <div className="mt-3"><Field label={writingText.instructions}><AutoTextarea className={`${CONTROL} resize-none`} minRows={2} value={writerExtra} onChange={event => setWriterExtra(event.target.value)} /></Field></div>
              <button
                type="button"
                onClick={() => void askAssistant('all')}
                disabled={assisting !== null || !assistInstruction.trim()}
                className="mt-3 inline-flex w-full items-center justify-center gap-2 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 py-2.5 text-xs font-bold text-white transition hover:brightness-110 disabled:opacity-50"
              >
                {assisting === 'all' ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} />}
                {assisting === 'all' ? `${t('assistantWriting')} · ${assistSeconds} ${t('secondsShort')}` : t('writeEverything')}
              </button>
              {assisting === 'all' && (
                <button
                  type="button"
                  onClick={stopAssistant}
                  className="mt-2 inline-flex w-full items-center justify-center gap-2 rounded-lg border border-zinc-300 py-2 text-xs font-semibold text-zinc-600 transition-colors hover:border-rose-400 hover:text-rose-600 dark:border-white/15 dark:text-zinc-300"
                >
                  <Square size={13} />
                  {t('cancelDownload')}
                </button>
              )}
            </Card>
          )}

          {mode === 'cover' && (
            <Card title={tt('coverTitle')} icon={<AudioLines size={13} />}>
              <p className="text-xs leading-5 text-zinc-600 dark:text-zinc-300">{tt('coverIntro')}</p>
              <ol className="mt-3 space-y-1.5 text-[11px] leading-4 text-zinc-500">
                <li><b className="text-zinc-700 dark:text-zinc-200">1.</b> {tt('coverStep1')}</li>
                <li><b className="text-zinc-700 dark:text-zinc-200">2.</b> {tt('coverStep2')}</li>
                <li><b className="text-zinc-700 dark:text-zinc-200">3.</b> {tt('coverStep3')}</li>
              </ol>
              <div className="mt-3 border-t border-zinc-100 pt-3 dark:border-white/5">
                <Switch checked={coverMelodyOnly} onChange={setCoverMelodyOnly} label={tt('coverMelodyOnly')} hint={tt('coverMelodyOnlyHint')} />
              </div>
              {!transcriberReady && (
                <p className="mt-3 flex gap-1.5 rounded-lg bg-amber-500/10 p-2 text-[11px] leading-4 text-amber-700 dark:text-amber-300">
                  <AlertTriangle size={13} className="mt-0.5 shrink-0" />
                  {tt('transcriberMissing')}
                </p>
              )}
              <input
                ref={audioFile}
                type="file"
                accept="audio/*,.mp3,.wav,.flac,.ogg,.m4a,.aiff,.aif"
                className="hidden"
                onChange={event => {
                  const file = event.target.files?.[0];
                  event.target.value = '';
                  if (file && isMidiFile(file.name)) {
                    void file.arrayBuffer().then(buffer => {
                      const bytes = new Uint8Array(buffer);
                      let raw = '';
                      for (let index = 0; index < bytes.length; index += 0x8000) raw += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
                      setMidiCover({ name: file.name, data: btoa(raw) });
                    });
                  } else if (file) {
                    setCoverSource(file.name);
                    setCoverSongId(null);
                    setCoverFile(file);
                    setCoverAudio(URL.createObjectURL(file));
                    void runTranscription({ file }, coverMelodyOnly);
                  }
                }}
              />
              <button
                type="button"
                onClick={() => audioFile.current?.click()}
                disabled={!transcriberReady || transcribing !== null}
                className="mt-3 inline-flex w-full items-center justify-center gap-2 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 py-2.5 text-xs font-bold text-white transition hover:brightness-110 disabled:opacity-50"
              >
                {transcribing !== null ? <Loader2 size={14} className="animate-spin" /> : <Upload size={14} />}
                {transcribing !== null ? `${tt('transcribing')} · ${transcribing}` : tt('coverPickRecording')}
              </button>
              <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt('coverFromLibraryHint')}</p>
              {coverAudio && (
                <div className="mt-3 flex items-center gap-3 rounded-lg border border-zinc-100 bg-zinc-50 p-2 dark:border-white/5 dark:bg-white/3">
                  <audio
                    key={coverAudio}
                    ref={coverPlayer}
                    src={coverAudio}
                    preload="metadata"
                    onLoadedMetadata={event => setCoverDuration(Number.isFinite(event.currentTarget.duration) ? event.currentTarget.duration : 0)}
                    onTimeUpdate={event => setCoverTime(event.currentTarget.currentTime)}
                    onPlay={() => setCoverPlaying(true)}
                    onPause={() => setCoverPlaying(false)}
                    onEnded={() => setCoverPlaying(false)}
                  />
                  <button
                    type="button"
                    onClick={() => { const player = coverPlayer.current; if (player) void (player.paused ? player.play() : player.pause()); }}
                    title={tt('coverListen')}
                    className="relative flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-linear-to-br from-orange-500 to-pink-600 text-white shadow-lg shadow-pink-500/20 transition-transform hover:scale-105"
                  >
                    {coverPlaying ? <Pause size={16} fill="currentColor" /> : <Play size={16} fill="currentColor" className="ml-0.5" />}
                  </button>
                  <div className="min-w-0 flex-1">
                    <div className="mb-1 flex items-center justify-between gap-2">
                      <span className="truncate text-xs font-medium text-zinc-800 dark:text-zinc-200">{coverSource}</span>
                      <span className="shrink-0 text-[10px] tabular-nums text-zinc-400">{formatDuration(coverTime)} / {formatDuration(coverDuration)}</span>
                    </div>
                    <AudioWaveform
                      url={coverAudio}
                      currentTime={coverTime}
                      duration={coverDuration}
                      onSeek={fraction => { const player = coverPlayer.current; if (player && coverDuration > 0) player.currentTime = fraction * coverDuration; }}
                    />
                  </div>
                </div>
              )}
              {coverSource && !transcribing && abc && (
                <p className="mt-2 text-[11px] text-emerald-600 dark:text-emerald-300">{tt('coverScoreReady')} · {coverSource}</p>
              )}
              {abc && (coverSongId || coverFile) && (
                <div className="mt-3 border-t border-zinc-100 pt-3 dark:border-white/5">
                  <button
                    type="button"
                    disabled={matchingSections || !hasSungLines(lyrics)}
                    onClick={() => {
                      setError(null);
                      setMatchingSections(true);
                      const run = sectionRun.current;
                      void matchSections({ abc, lyrics: lyrics.replace(/\r\n?/g, '\n'), songId: coverSongId, file: coverFile })
                        .then(answer => {
                          if (run !== sectionRun.current) return;
                          if (failed(answer)) setError(karaokeReason(tt, answer.error) ?? answer.error);
                          else setSectionProposal(answer.proposal);
                        })
                        .finally(() => setMatchingSections(false));
                    }}
                    title={tt('coverMatchSectionsHint')}
                    className="inline-flex w-full items-center justify-center gap-2 rounded-lg border border-zinc-200 py-2 text-xs font-semibold text-zinc-700 transition hover:border-pink-300 disabled:opacity-50 dark:border-white/10 dark:text-zinc-200"
                  >
                    {matchingSections ? <Loader2 size={13} className="animate-spin" /> : <ListChecks size={13} />}
                    {matchingSections ? tt('coverMatchingSections') : tt('coverMatchSections')}
                  </button>
                  <p className="mt-1.5 text-[11px] leading-4 text-zinc-500">{tt('coverMatchSectionsHint')}</p>
                  {sectionProposal && (
                    <div className="mt-3 rounded-lg border border-pink-300/40 bg-pink-50/60 p-3 dark:border-pink-500/20 dark:bg-pink-500/5">
                      {sectionProposal.unchanged ? (
                        <p className="text-[11px] text-zinc-600 dark:text-zinc-300">{tt('coverSectionsUnchanged')}</p>
                      ) : (
                        <>
                          <p className="text-[11px] leading-4 text-zinc-700 dark:text-zinc-200">
                            {tt('coverSectionsSummary')
                              .replace('{renamed}', String(sectionProposal.blocks.filter(block => block.status === 'renamed').length))
                              .replace('{copied}', String(sectionProposal.filled.filter(fill => fill.copied_from !== null).length))
                              .replace('{unsure}', String(sectionProposal.blocks.filter(block => block.status === 'unsure').length))}
                          </p>
                          {sectionProposal.blocks.some(block => block.status === 'unsure') && (
                            <p className="mt-1 flex gap-1.5 text-[11px] leading-4 text-amber-700 dark:text-amber-300">
                              <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                              {tt('coverSectionsUnsure').replace('{blocks}', sectionProposal.blocks.filter(block => block.status === 'unsure').map(block => block.tag ?? `#${block.index}`).join(', '))}
                            </p>
                          )}
                          <div className="mt-2 grid grid-cols-2 gap-2">
                            <pre className="max-h-48 overflow-auto rounded-md bg-white/70 p-2 text-[10px] leading-4 text-zinc-500 dark:bg-black/20">{lyrics}</pre>
                            <pre className="max-h-48 overflow-auto rounded-md bg-white/70 p-2 text-[10px] leading-4 text-zinc-800 dark:bg-black/20 dark:text-zinc-100">{sectionProposal.lyrics}</pre>
                          </div>
                        </>
                      )}
                      <div className="mt-2 flex justify-end gap-2">
                        <button type="button" onClick={() => setSectionProposal(null)} className="rounded-md px-3 py-1.5 text-[11px] font-semibold text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-white/5">
                          {tt('cancel')}
                        </button>
                        {!sectionProposal.unchanged && (
                          <button
                            type="button"
                            onClick={() => { setLyrics(sectionProposal.lyrics); setSectionProposal(null); }}
                            className="rounded-md bg-pink-600 px-3 py-1.5 text-[11px] font-bold text-white hover:brightness-110"
                          >
                            {tt('coverSectionsApply')}
                          </button>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              )}
            </Card>
          )}

          {activity.filter(entry => entry.state === 'running').slice(-3).map(entry => (
            <div key={`${entry.song_id}-${entry.kind}`} className="rounded-xl border border-zinc-200 bg-white px-3 py-2 text-[11px] dark:border-white/10 dark:bg-suno-card">
              <div className="flex items-center gap-2">
                <Loader2 size={12} className="animate-spin text-pink-500" />
                <span className="font-semibold text-zinc-700 dark:text-zinc-200">
                  {entry.kind === 'cover' ? t('activityCover') : t('activityKaraoke')}
                </span>
                <span className="min-w-0 flex-1 truncate text-zinc-500">{entry.title}</span>
              </div>
            </div>
          ))}

          {assisting !== null && (
            <div className="rounded-xl border border-zinc-200 bg-white p-3 dark:border-white/10 dark:bg-suno-card">
              <div className="flex items-center justify-between gap-2 text-[11px] font-semibold uppercase tracking-wide">
                <span className="flex items-center gap-1.5 text-pink-600 dark:text-pink-300">
                  <Loader2 size={12} className="animate-spin" />
                  {assistStage === 'sent' ? t('assistStageSent') : assistStage === 'writing' ? t('assistStageWriting') : assistStage === 'done' ? t('assistStageDone') : t('assistStagePreparing')}
                </span>
                <span className="flex items-center gap-2">
                  <span className="tabular-nums text-zinc-400">{assistSeconds} {t('secondsShort')}</span>
                  <button type="button" onClick={stopAssistant} className={ICON} title={t('cancelDownload')}><X size={13} /></button>
                </span>
              </div>
              {assistModel && <p className="mt-1 truncate text-[11px] text-zinc-500">{assistModel}</p>}
              {assistDraft && (
                <pre className="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap wrap-break-word rounded-lg bg-zinc-50 p-2 font-mono text-[11px] leading-4 text-zinc-600 dark:bg-black/30 dark:text-zinc-300">
                  {assistDraft.slice(-1200)}
                </pre>
              )}
            </div>
          )}

          <Card
            title={tt('styleCardTitle')}
            icon={<Music2 size={13} />}
            actions={
              <>
                <button type="button" onClick={() => (assistantReady ? void askAssistant('style') : openAssistantSetup())} disabled={assistantReady && (assisting !== null)} className={ICON} title={assistantReady ? tt('writeStyle') : t('setUpAssistant')}>
                  {assisting === 'style' ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} className="text-pink-500" />}
                </button>
                <button type="button" onClick={() => setExamplesOpen(open => !open)} className={ICON} title={t('examplePrompt')} aria-expanded={examplesOpen}><Dices size={14} /></button>
                <button type="button" onClick={() => promptFile.current?.click()} className={ICON} title={t('openPrompt')}><FolderOpen size={14} /></button>
                <span className="relative">
                  <button type="button" onClick={() => setSaveMenuOpen(open => !open)} className={ICON} title={t('savePrompt')} aria-expanded={saveMenuOpen}><Save size={14} /></button>
                  {saveMenuOpen && (
                    <span className="absolute right-0 top-full z-20 mt-1 flex overflow-hidden rounded-lg border border-zinc-200 bg-white text-[11px] font-semibold shadow-lg dark:border-white/10 dark:bg-zinc-900">
                      {(['json', 'yaml'] as const).map(format => (
                        <button key={format} type="button" onClick={() => savePrompt(format)} className="px-3 py-1.5 uppercase text-zinc-700 hover:bg-pink-500/10 hover:text-pink-600 dark:text-zinc-200">{format}</button>
                      ))}
                    </span>
                  )}
                </span>
                <button type="button" onClick={reset} className={ICON} title={t('resetPrompt')}><RotateCcw size={14} /></button>
                <input
                  ref={promptFile}
                  type="file"
                  accept={REQUEST_FILE_ACCEPT}
                  className="hidden"
                  onChange={event => { const file = event.target.files?.[0]; if (file) void openPrompt(file); event.target.value = ''; }}
                />
              </>
            }
          >
            {examplesOpen && (
              <div className="mb-3 rounded-lg border border-zinc-200 bg-zinc-50 p-2 dark:border-white/10 dark:bg-black/25">
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-[10px] font-semibold uppercase tracking-wide text-zinc-500">{tt('officialExamples')} · {EXAMPLES.length}</span>
                  <button type="button" onClick={() => loadExample()} className="inline-flex items-center gap-1 text-[11px] font-semibold text-pink-600 hover:text-pink-500 dark:text-pink-300"><Dices size={12} />{tt('randomExample')}</button>
                </div>
                <div className="grid max-h-52 grid-cols-2 gap-1 overflow-y-auto custom-scrollbar">
                  {EXAMPLES.map(example => (
                    <button
                      key={example.id}
                      type="button"
                      onClick={() => loadExample(example.id)}
                      className="truncate rounded-md px-2 py-1 text-left text-[11px] text-zinc-600 transition-colors hover:bg-white hover:text-black dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-white"
                      title={example.style}
                    >
                      {example.cover && <span className="mr-1 rounded-sm bg-pink-500/15 px-1 text-[9px] font-bold uppercase text-pink-600 dark:text-pink-300">{tt('coverBadge')}</span>}
                      {example.title}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <input
              value={name}
              onChange={event => setName(event.target.value)}
              placeholder={t('untitled')}
              aria-label={t('untitled')}
              className="w-full border-0 bg-transparent p-0 text-lg font-bold text-zinc-900 outline-hidden placeholder:text-zinc-300 dark:text-white dark:placeholder:text-zinc-600"
            />
            <AutoTextarea
              value={style}
              minRows={3}
              maxRows={8}
              onChange={event => setStyle(event.target.value)}
              placeholder={tt('stylePlaceholder')}
              aria-label={tt('styleCardTitle')}
              className={`${CONTROL} mt-3 resize-none overflow-y-auto leading-5 custom-scrollbar`}
            />
            <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt('styleHint')}</p>
            <SongTextParts kind="style" value={style} onChange={setStyle} />
            {mode === 'cover' && coverSongId && (
              <button type="button" onClick={() => void describeByEar()} disabled={describing} className="mt-2 inline-flex items-center gap-1.5 rounded-md border border-zinc-200 px-2 py-1 text-[11px] font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/10 dark:text-zinc-300" title={tt('describeByEarHint')}>
                {describing ? <Loader2 size={12} className="animate-spin" /> : <Ear size={12} />}{tt('describeByEar')}
              </button>
            )}
          </Card>

          <Card
            title={t('lyrics')}
            actions={
              <>
                <button type="button" onClick={() => (assistantReady ? void layOutLyrics() : openAssistantSetup())} disabled={assistantReady && (assisting !== null || !lyrics.trim())} className={ICON} title={assistantReady ? t('formatLyrics') : t('setUpAssistant')}>
                  {assisting === 'sections' ? <Loader2 size={14} className="animate-spin" /> : <Tags size={14} />}
                </button>
                <button type="button" onClick={() => (assistantReady ? void askAssistant('lyrics') : openAssistantSetup())} disabled={assistantReady && (assisting !== null)} className={ICON} title={assistantReady ? t('writeLyrics') : t('setUpAssistant')}>
                  {assisting === 'lyrics' ? <Loader2 size={14} className="animate-spin" /> : <Wand2 size={14} className="text-pink-500" />}
                </button>
                <button type="button" onClick={() => setLyrics('')} className={ICON} title={t('resetPrompt')}><RotateCcw size={14} /></button>
              </>
            }
          >
            <div className="mb-2 flex flex-wrap gap-1">
              {SECTION_TAGS.map(tag => (
                <button key={tag} type="button" onClick={() => insertTag(tag)} className={CHIP}>{tag}</button>
              ))}
            </div>
            <AutoTextarea
              value={lyrics}
              minRows={10}
              maxRows={22}
              onChange={event => setLyrics(event.target.value)}
              onFocus={event => { lyricsBox.current = event.currentTarget; }}
              placeholder={'[Verse 1]\n…\n\n[Chorus]\n…'}
              aria-label={t('lyrics')}
              className={`${CONTROL} resize-none overflow-y-auto font-mono text-xs leading-5 custom-scrollbar`}
            />
            <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt('lyricsHintYue')}</p>
            <SongTextParts kind="lyrics" value={lyrics} onChange={setLyrics} style={style} cot={effectiveCot} />
          </Card>

          <Card
            title={tt('scoreCardTitle')}
            icon={<FileMusic size={13} />}
            actions={
              <>
                <button type="button" onClick={() => setShowNotation(value => !value)} className={ICON} title={showNotation ? tt('hideNotation') : tt('showNotation')}>
                  {showNotation ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
                <button type="button" onClick={() => scoreFile.current?.click()} className={ICON} title={tt('openScore')}><FolderOpen size={14} /></button>
                <button type="button" onClick={() => abc.trim() && download(`${safeName()}.abc`, `${abc.trim()}\n`, 'text/vnd.abc')} disabled={!abc.trim()} className={ICON} title={tt('saveScore')}><Save size={14} /></button>
                <button type="button" onClick={() => { setAbc(''); setContinueScore(false); }} disabled={!abc} className={ICON} title={tt('clearScore')}><X size={14} /></button>
                <input
                  ref={scoreFile}
                  type="file"
                  accept=".abc,text/plain"
                  className="hidden"
                  onChange={event => { const file = event.target.files?.[0]; if (file) void openScore(file); event.target.value = ''; }}
                />
              </>
            }
          >
            <div className="grid grid-cols-3 gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-black/30" role="radiogroup" aria-label={tt('cotMode')}>
              {(['full', 'melody', 'off'] as const).map(value => (
                <button
                  key={value}
                  type="button"
                  role="radio"
                  aria-checked={effectiveCot === value}
                  onClick={() => setCot(value)}
                  className={`rounded-md px-2 py-1.5 text-[11px] font-semibold transition-all ${effectiveCot === value ? 'bg-white text-black shadow-xs dark:bg-zinc-700 dark:text-white' : 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200'}`}
                >
                  {tt(value === 'full' ? 'cotFull' : value === 'melody' ? 'cotMelody' : 'cotOff')}
                </button>
              ))}
            </div>
            <p className="mt-2 text-[11px] leading-4 text-zinc-500">
              {tt(effectiveCot === 'full' ? 'cotFullHint' : effectiveCot === 'melody' ? 'cotMelodyHint' : 'cotOffHint')}
            </p>
            {!scoreDisabled && (
              <>
                {showNotation && abc.trim() && (
                  <div className="mt-3 max-h-80 overflow-auto rounded-lg border border-zinc-200 bg-white p-2 dark:border-white/10 custom-scrollbar">
                    <ScoreView abc={abc} onChange={setAbc} />
                  </div>
                )}
                <AutoTextarea
                  value={abc}
                  minRows={5}
                  maxRows={14}
                  onChange={event => setAbc(event.target.value)}
                  placeholder={tt('scorePlaceholder')}
                  aria-label={tt('scoreCardTitle')}
                  spellCheck={false}
                  className={`${CONTROL} mt-3 resize-none overflow-y-auto font-mono text-[11px] leading-4 custom-scrollbar`}
                />
                <p className="mt-2 text-[11px] leading-4 text-zinc-500">{tt('scoreHint')}</p>
                {/%yue2-words [a-f0-9]{16}/.test(abc) && <label className="mt-2 flex items-start gap-2 text-xs text-zinc-500" title={midiImportOptions[language].keepHint}>
                  <input type="checkbox" className="mt-0.5 accent-pink-500" checked={/%yue2-words [a-f0-9]{16} keep/.test(abc)} onChange={event => setAbc(current => current.replace(/(%yue2-words [a-f0-9]{16})(?: keep)?/g, `$1${event.target.checked ? ' keep' : ''}`))} />
                  {midiImportOptions[language].keep}
                </label>}
                {abc.trim() && (
                  <div className="mt-2">
                    <Switch checked={continueScore} onChange={setContinueScore} label={tt('continueScore')} hint={tt('continueScoreHint')} disabled={Boolean(semanticTokens.trim())} />
                  </div>
                )}
                <div className="mt-2 flex flex-wrap items-center gap-2">
                  <select className={CONTROL + ' max-w-24'} value={planCount} aria-label={planLabels[language]} onChange={event => setPlanCount(event.target.value)} disabled={composing}>
                    {Array.from({ length: Math.min(maxBatch, 9) }, (_, index) => index + 1).map(count => <option key={count} value={count}>{count}</option>)}
                  </select>
                  <button
                    type="button"
                    onClick={() => (composing ? composeRun.current?.abort() : void runComposition())}
                    disabled={!composing && !ready}
                    className="inline-flex items-center gap-1.5 rounded-lg border border-pink-500/40 px-3 py-1.5 text-xs font-semibold text-pink-600 transition hover:bg-pink-500/10 disabled:opacity-50 dark:text-pink-300"
                  >
                    {composing ? <Loader2 size={13} className="animate-spin" /> : <FileMusic size={13} />}
                    {composing ? tt('composingScore') : continueScore && abc.trim() ? tt('composeContinue') : abc.trim() ? tt('composeScoreAgain') : tt('composeScore')}
                  </button>
                  <button
                    type="button"
                    onClick={() => setEditorOpen(true)}
                    disabled={composing}
                    title={tt('scoreEditorHint')}
                    className="inline-flex items-center gap-1.5 rounded-lg border border-pink-500/40 px-3 py-1.5 text-xs font-semibold text-pink-600 transition hover:bg-pink-500/10 disabled:opacity-50 dark:text-pink-300"
                  >
                    <Piano size={13} />
                    {tt('scoreEditor')}
                  </button>
                  <button
                    type="button"
                    onClick={() => setBedOpen(open => !open)}
                    disabled={composing}
                    aria-expanded={bedOpen}
                    title={tt('chordBedHint')}
                    className="inline-flex items-center gap-1.5 rounded-lg border border-pink-500/40 px-3 py-1.5 text-xs font-semibold text-pink-600 transition hover:bg-pink-500/10 disabled:opacity-50 dark:text-pink-300"
                  >
                    <Grid2x2 size={13} />
                    {tt('chordBed')}
                  </button>
                  <span className="text-[11px] leading-4 text-zinc-500">{composing ? tt('composeScoreCancel') : tt('composeScoreHint')}</span>
                </div>
                {bedOpen && (
                  <div className="mt-2 rounded-lg border border-zinc-100 p-2 dark:border-white/5">
                    <p className="mb-2 text-[11px] leading-4 text-zinc-500">{tt('chordBedHint')}</p>
                    <div className="grid grid-cols-3 gap-2">
                      <Field label={tt('chordBedTempo')}>
                        <input value={bedBpm} onChange={event => setBedBpm(event.target.value)} inputMode="numeric" className={CONTROL} />
                      </Field>
                      <Field label={tt('chordBedKey')}>
                        <select value={bedKey} onChange={event => setBedKey(event.target.value)} className={CONTROL}>
                          {BED_KEYS.map(key => <option key={key} value={key}>{key}</option>)}
                        </select>
                      </Field>
                      <Field label={tt('chordBedMeter')}>
                        <select value={bedMeter} onChange={event => setBedMeter(event.target.value)} className={CONTROL}>
                          {BED_METERS.map(meter => <option key={meter} value={meter}>{meter}</option>)}
                        </select>
                      </Field>
                    </div>
                    <button
                      type="button"
                      disabled={bedBusy}
                      onClick={() => {
                        setError(null);
                        setBedBusy(true);
                        void chordBed({ bpm: Math.max(0, Math.round(Number(bedBpm))) || 0, key: bedKey, meter: bedMeter, lyrics: lyrics.replace(/\r\n?/g, '\n') })
                          .then(answer => {
                            if (failed(answer)) { setError(answer.error); return; }
                            setAbc(answer.abc.trimEnd());
                            setPlans([]); setSelectedPlan(0); setSemanticTokens(''); setContinueScore(false);
                            setBedWritten(answer.progressions);
                          })
                          .finally(() => setBedBusy(false));
                      }}
                      className="mt-2 inline-flex items-center gap-1.5 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-3 py-1.5 text-xs font-bold text-white transition hover:brightness-110 disabled:opacity-50"
                    >
                      {bedBusy ? <Loader2 size={13} className="animate-spin" /> : <Grid2x2 size={13} />}
                      {abc.trim() ? tt('chordBedWriteAgain') : tt('chordBedWrite')}
                    </button>
                    {bedWritten && (
                      <p className="mt-2 text-[11px] leading-4 text-zinc-500">
                        {tt('chordBedWritten').replace('{verse}', bedWritten[0].join(' ')).replace('{chorus}', bedWritten[1].join(' ')).replace('{bridge}', bedWritten[2].join(' '))}
                      </p>
                    )}
                  </div>
                )}
                {plans.length > 1 && <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
                  <span className="text-zinc-500">{planLabels[language]}</span>
                  {plans.map((plan, index) => <button key={index} type="button" aria-pressed={index === selectedPlan} className={`${CHIP} ${index === selectedPlan ? 'border-pink-500 text-pink-500' : ''}`} onClick={() => { setSelectedPlan(index); setAbc(plan.abc); setSemanticTokens(''); }}>
                    {index + 1}{plan.lm_seed !== undefined && plan.lm_seed !== null ? ` · ${plan.lm_seed}` : ''}
                  </button>)}
                </div>}
                {melodyWithChords && (
                  <div className="mt-2 flex items-start gap-2 rounded-lg bg-amber-500/10 p-2 text-[11px] leading-4 text-amber-700 dark:text-amber-300">
                    <AlertTriangle size={13} className="mt-0.5 shrink-0" />
                    <span className="flex-1">{tt('melodyScoreHasChords')}</span>
                    <button type="button" onClick={() => setAbc(current => current.replace(CHORD_SYMBOLS, ''))} className="shrink-0 font-semibold underline decoration-dotted underline-offset-2">{tt('stripChords')}</button>
                  </div>
                )}
                {assistantReady && abc.trim() && (
                  <div className="mt-3 rounded-lg border border-pink-500/20 bg-pink-500/5 p-2">
                    <span className={LABEL}>{tt('scoreEditTitle')}</span>
                    <div className="flex gap-2">
                      <input
                        value={scoreInstruction}
                        onChange={event => setScoreInstruction(event.target.value)}
                        onKeyDown={event => { if (event.key === 'Enter' && scoreInstruction.trim()) void askAssistant('score'); }}
                        placeholder={tt('scoreEditPlaceholder')}
                        className={CONTROL}
                      />
                      <button
                        type="button"
                        onClick={() => void askAssistant('score')}
                        disabled={assisting !== null || !scoreInstruction.trim()}
                        className="inline-flex shrink-0 items-center gap-1.5 rounded-lg bg-pink-600 px-3 text-xs font-bold text-white transition hover:brightness-110 disabled:opacity-50"
                      >
                        {assisting === 'score' ? <Loader2 size={13} className="animate-spin" /> : <Wand2 size={13} />}
                        {tt('scoreEditApply')}
                      </button>
                    </div>
                  </div>
                )}
              </>
            )}
          </Card>

          <AdapterPicker
            value={adapters}
            onChange={setAdapters}
            onTrigger={applyTrigger}
            iconClass={ICON}
            frame={(title, icon, actions, body) => <Card title={title} icon={icon} actions={actions}>{body}</Card>}
          />

          <Card
            title={t('quality')}
            actions={
              <button type="button" onClick={resetParameters} className="rounded-md px-2 py-1 text-[10px] font-semibold text-zinc-500 transition-colors hover:bg-zinc-200 hover:text-black dark:hover:bg-white/10 dark:hover:text-white">
                {t('resetToDefaults')}
              </button>
            }
          >
            <div className="space-y-3">
              <SliderRow
                label={t('maxDuration')}
                value={duration}
                fallback={durationFallback}
                min={10}
                max={MAX_DURATION_SECONDS}
                step={5}
                onChange={chooseDuration}
                format={formatDuration}
              />
              <p className="text-[11px] leading-4 text-zinc-500">{tt('maxDurationHintYue')}</p>
              <SliderRow
                label={tt('flowSteps')}
                value={steps}
                fallback={Number(defaults.steps ?? 32)}
                min={1}
                max={100}
                step={1}
                onChange={setSteps}
              />
              <Field label={tt('solver')} hint={tt(solver === 'ab2' ? 'solverAb2Hint' : 'solverMidpointHint')}>
                <select value={solver || 'midpoint'} onChange={event => setSolver(event.target.value as 'midpoint' | 'ab2')} className={CONTROL}>
                  <option value="midpoint">{tt('solverMidpoint')}</option>
                  <option value="ab2">{tt('solverAb2')}</option>
                </select>
              </Field>
            </div>
            <div className="mt-4 space-y-3 border-t border-zinc-100 pt-4 dark:border-white/5">
              <SliderRow
                label={tt('songsPerRequest')}
                value={lmBatch}
                fallback={1}
                min={1}
                max={Math.max(maxBatch, 1)}
                step={1}
                onChange={setLmBatch}
                disabled={maxBatch <= 1}
              />
              <p className="text-[11px] leading-4 text-zinc-500">{tt('songsPerRequestExplain')}{maxBatch <= 1 && ` ${tt('songsPerRequestHint')}`}</p>
              <SliderRow
                label={t('variationsBatch')}
                value={synthBatch}
                fallback={1}
                min={1}
                max={9}
                step={1}
                onChange={setSynthBatch}
              />
              <p className="text-[11px] leading-4 text-zinc-500">{tt('variationsExplain')}</p>
              <Switch checked={randomizeSeed} onChange={setRandomizeSeed} label={t('randomizeSeed')} />
              {!randomizeSeed && (
                <div className="grid grid-cols-2 gap-2">
                  <Field label={tt('lmSeedYue')}>
                    <input value={lmSeed} onChange={event => setLmSeed(event.target.value)} placeholder="-1" inputMode="numeric" className={CONTROL} />
                  </Field>
                  <Field label={tt('noiseSeed')}>
                    <input value={seed} onChange={event => setSeed(event.target.value)} placeholder="-1" inputMode="numeric" className={CONTROL} />
                  </Field>
                </div>
              )}
              {totalTracks > 1 && (
                <p className="text-[11px] text-zinc-500">{t('renderCountPrefix')} <b className="text-zinc-700 dark:text-zinc-200">{totalTracks}</b></p>
              )}
            </div>
          </Card>

          <div className="overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-white/5 dark:bg-suno-card">
            <button
              type="button"
              onClick={() => setShowAdvanced(current => !current)}
              className="flex w-full items-center justify-between gap-2 px-3 py-2 text-[11px] font-bold uppercase tracking-wide text-zinc-500 transition-colors hover:text-black dark:text-zinc-400 dark:hover:text-white"
              aria-expanded={showAdvanced}
            >
              {t('advanced')}
              <ChevronDown size={15} className={showAdvanced ? 'rotate-180 transition-transform' : 'transition-transform'} />
            </button>
            {showAdvanced && (
              <div className="space-y-4 border-t border-zinc-100 p-3 dark:border-white/5">
                <Stage title={tt('stageScoreSampling')} hint={tt('stageScoreSamplingHint')}>
                  <div className="mb-3">
                    <SliderRow label={engineParityLabels[language].transpose} value={transpose} fallback={0} min={-24} max={24} step={1}
                      disabled={!abc.trim() || effectiveCot === 'off' || Boolean(semanticTokens.trim())} onChange={setTranspose} />
                    <p className="mt-1 text-[11px] leading-4 text-zinc-500">{engineParityLabels[language].transposeHint}</p>
                  </div>
                  <div className="mb-3"><Switch checked={lyricTiming} disabled={!abc.trim() || effectiveCot === 'off' || Boolean(semanticTokens.trim()) || (cfgScale.trim() !== '' && Number(cfgScale) !== 1)} onChange={setLyricTiming} label={engineParityLabels[language].lyricTiming} hint={engineParityLabels[language].lyricTimingHint} /></div>
                  <SamplingGrid value={abcSampling} defaults={defaults.abc_sampling} onChange={setAbcSampling} t={t as never} />
                  <div className={`mt-4 space-y-3 border-t border-zinc-100 pt-3 dark:border-white/5 ${effectiveCot === 'off' ? 'opacity-50' : ''}`}>
                    <div>
                      <span className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{tt('harmonyTitle')}</span>
                      <p className="mt-0.5 text-[11px] leading-4 text-zinc-500">{tt('harmonyHint')}</p>
                    </div>
                    <SliderRow label={tt('chordVariety')} value={harmony.strength} fallback={0} min={0} max={16} step={0.5} disabled={effectiveCot === 'off'} onChange={harmonyField('strength')} />
                    {(numberOrUndefined(harmony.strength) ?? 0) > 0 && (
                      <>
                        <Field label={tt('chordIdentity')} hint={tt(harmony.identity === 'spelling' ? 'chordIdentitySpellingHint' : 'chordIdentityRootHint')}>
                          <select value={harmony.identity || 'root'} onChange={event => harmonyField('identity')(event.target.value)} className={CONTROL}>
                            <option value="root">{tt('chordIdentityRoot')}</option>
                            <option value="spelling">{tt('chordIdentitySpelling')}</option>
                          </select>
                        </Field>
                        <div className="grid grid-cols-2 gap-2">
                          <SliderRow label={tt('chordWindow')} value={harmony.window} fallback={16} min={4} max={64} step={1} onChange={harmonyField('window')} />
                          <SliderRow label={tt('chordHold')} value={harmony.hold_limit} fallback={8} min={0} max={16} step={1} format={value => (value === 0 ? '∞' : String(value))} onChange={harmonyField('hold_limit')} />
                        </div>
                      </>
                    )}
                    <div>
                      <SliderRow label={tt('outsideKey')} value={harmony.outside_bonus} fallback={0} min={0} max={8} step={0.5} disabled={effectiveCot === 'off' || harmony.identity === 'spelling'} onChange={harmonyField('outside_bonus')} />
                      <p className="mt-1 text-[11px] leading-4 text-zinc-500">{tt('outsideKeyHint')}</p>
                    </div>
                    <div>
                      <SliderRow label={tt('sectionVariety')} value={harmony.section_strength} fallback={0} min={0} max={16} step={0.5} disabled={effectiveCot === 'off'} onChange={harmonyField('section_strength')} />
                      <p className="mt-1 text-[11px] leading-4 text-zinc-500">{tt('sectionVarietyHint')}</p>
                    </div>
                    <Switch checked={harmony.follow_lyrics} disabled={effectiveCot === 'off'} onChange={value => harmonyField('follow_lyrics')(String(value))} label={tt('followSections')} hint={tt('followSectionsHint')} />
                  </div>
                </Stage>

                <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
                  <Stage title={tt('stageSemantic')} hint={tt('stageSemanticHint')}>
                    <Field label={tt('guidanceScale')} hint={tt('guidanceScaleHint')}>
                      <input value={cfgScale} onChange={event => setCfgScale(event.target.value)} placeholder={tt('guidanceAuto')} inputMode="decimal" className={CONTROL} />
                    </Field>
                    <div className="mt-3">
                      <SamplingGrid value={semanticSampling} defaults={defaults.semantic_sampling} onChange={setSemanticSampling} t={t as never} maxTokensCap={semanticBudget} />
                      <p className={`mt-2 text-[11px] leading-4 ${semanticMaxTokens !== undefined && semanticMaxTokens > semanticBudget ? 'text-amber-600 dark:text-amber-400' : 'text-zinc-500'}`}>
                        {budgetText(semanticMaxTokens !== undefined && semanticMaxTokens > semanticBudget ? 'semanticBudgetOver' : 'semanticBudget')}
                      </p>
                    </div>
                    <div className="mt-3">
                      <Field label={tt('semanticTokens')} hint={tt('semanticTokensHint')}>
                        <AutoTextarea
                          value={semanticTokens}
                          minRows={2}
                          onChange={event => setSemanticTokens(event.target.value)}
                          placeholder="12046,8433,22418,…"
                          spellCheck={false}
                          className={`${CONTROL} resize-none font-mono text-[11px]`}
                        />
                      </Field>
                    </div>
                  </Stage>
                </div>

                <div className="border-t border-zinc-100 pt-4 dark:border-white/5">
                  <Stage title={t('stageOutput')} hint={t('stageOutputHint')}>
                    <div className="mb-3">
                      <Field label={engineParityLabels[language].realaudio} hint={engineParityLabels[language].realaudioHint}>
                        <select value={realaudio} onChange={event => setRealaudio(event.target.value as 'auto' | 'on' | 'off')} className={CONTROL}>
                          <option value="auto">{engineParityLabels[language].realaudioAuto}</option>
                          <option value="on">{engineParityLabels[language].realaudioOn}</option>
                          <option value="off">{engineParityLabels[language].realaudioOff}</option>
                        </select>
                      </Field>
                    </div>
                    <div className="mb-3"><Switch checked={vocalsOnly} onChange={setVocalsOnly} label={engineParityLabels[language].vocals} hint={engineParityLabels[language].vocalsHint} /></div>
                    <div className="mt-3 grid grid-cols-2 gap-2">
                      <Field label={t('mp3Bitrate')}>
                        <select value={mp3Bitrate || String(defaults.mp3_bitrate ?? 128)} onChange={event => setMp3Bitrate(event.target.value)} disabled={format !== 'mp3'} className={CONTROL}>
                          {['128', '192', '256', '320'].map(rate => <option key={rate} value={rate}>{rate} kbps</option>)}
                        </select>
                      </Field>
                      <Field label={t('outputFormat')}>
                        <select value={format} onChange={event => setFormat(event.target.value as YueOutputFormat)} className={CONTROL}>
                          <option value="flac">FLAC</option>
                          <option value="mp3">MP3</option>
                        </select>
                      </Field>
                    </div>
                    <p className="mt-2 text-[11px] leading-4 text-zinc-500">{t('outputRawHint')}</p>
                  </Stage>
                </div>
              </div>
            )}
          </div>

          <div className="flex items-center justify-between px-1 text-[11px] text-zinc-500 dark:text-zinc-400">
            <button
              type="button"
              onClick={() => window.dispatchEvent(new CustomEvent('yue:open-settings', { detail: 'models' }))}
              className="text-left hover:text-pink-500"
              title={t('changeProfileHint')}
            >
              {t('profile')}: <b className="text-zinc-700 underline decoration-dotted underline-offset-2 dark:text-zinc-200">{profileLabel}</b>
            </button>
            {catalog?.version && <span className="font-mono text-[10px] text-zinc-400">yue2.cpp {catalog.version.split(' ')[0]}</span>}
          </div>
          {setup?.hardware?.recommended && (
            <p className="px-1 text-[10px] text-zinc-400">
              {setup.hardware.gpuName}{setup.hardware.totalVramGb ? `, ${setup.hardware.totalVramGb.toFixed(1)} GB` : ''} · {t('recommendedForMachine')}{' '}
              {setLabel(t, { id: setup.hardware.recommended, label: PROFILE_LABEL[setup.hardware.recommended] ?? setup.hardware.recommended })}
            </p>
          )}
          {error && <div role="alert" className="rounded-xl border border-red-500/30 bg-red-500/10 p-3 text-xs leading-5 text-red-700 dark:text-red-200">{error}</div>}
        </div>
      </div>

      <footer className="shrink-0 border-t border-zinc-200 bg-zinc-50/95 p-4 backdrop-blur-sm dark:border-white/5 dark:bg-suno-panel/95">
        {takeChoiceOpen && (
          <div role="dialog" aria-label={tt('takeTitle')} className="mb-3 rounded-xl border border-pink-300/50 bg-pink-50 p-3 text-xs leading-5 text-zinc-700 dark:border-pink-500/25 dark:bg-pink-500/10 dark:text-zinc-200">
            <p className="font-semibold text-zinc-900 dark:text-white">{tt('takeTitle')}</p>
            <p className="mt-1">{tt('takeBody')}</p>
            <div className="mt-2 flex flex-wrap gap-2">
              <button type="button" onClick={() => renderTake(false)} className="rounded-lg bg-pink-600 px-3 py-1.5 font-semibold text-white hover:brightness-110">{tt('sameTake')}</button>
              <button type="button" onClick={() => renderTake(true)} className="rounded-lg border border-zinc-300 px-3 py-1.5 font-semibold hover:border-pink-400 dark:border-white/15">{tt('newTake')}</button>
              <button type="button" onClick={() => setTakeChoiceOpen(false)} className="ml-auto px-2 py-1.5 text-zinc-500 hover:text-zinc-800 dark:hover:text-white">{t('cancel')}</button>
            </div>
          </div>
        )}
        <label className="mb-2 flex items-center gap-2 text-xs text-zinc-600 dark:text-zinc-300">
          <span className="shrink-0">{tt('createIntoPlaylist')}</span>
          <select value={chosenPlaylist} onChange={event => (event.target.value === NEW_PLAYLIST ? setNewPlaylistOpen(true) : choosePlaylist(event.target.value))} className="min-w-0 flex-1 rounded-md border border-zinc-200 bg-white px-2 py-1 text-xs dark:border-white/10 dark:bg-black/20">
            <option value="">{tt('createIntoNoPlaylist')}</option>
            {playlists.map(entry => <option key={entry.id} value={entry.id}>{entry.name}</option>)}
            <option value={NEW_PLAYLIST}>{tt('createIntoNewPlaylist')}</option>
          </select>
        </label>
        <CreatePlaylistModal
          isOpen={newPlaylistOpen}
          onClose={() => setNewPlaylistOpen(false)}
          onCreate={(name, description) => void onCreatePlaylist(name, description).then(playlist => { if (playlist) choosePlaylist(playlist.id); })}
        />
        {/* songs without end are switched on by a deliberate slide, never by a stray click */}
        <div className="mb-2">
          <SlideToEnable
            on={forever}
            onChange={setForever}
            offLabel={tt('generateForeverSlide')}
            stopLabel={tt('generateForeverStop')}
            title={tt('generateForeverHint')}
          />
        </div>
        <button
          type="button"
          onClick={submit}
          className="flex h-12 w-full items-center justify-center gap-2 rounded-xl bg-linear-to-r from-orange-500 to-pink-600 text-base font-bold text-white shadow-lg transition hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {isGenerating ? <Square size={18} /> : <Sparkles size={18} />}
          {t('create')}
          {activeJobCount > 0 && <span className="rounded-full bg-white/20 px-2 py-0.5 text-xs">{activeJobCount}</span>}
        </button>
      </footer>
      {editorOpen && (
        <MidiEditor
          source={{
            kind: 'score',
            abc,
            title: name,
            chords: effectiveCot === 'full',
            style: finishedStyle(style),
            lyrics: lyrics.trim(),
            cot: effectiveCot,
            onApply: (value, words) => {
              setAbc(value);
              if (words !== null) setLyrics(current => (current.trim() ? current : words));
              setSemanticTokens('');
              setShowNotation(true);
            },
          }}
          onClose={() => setEditorOpen(false)}
        />
      )}
      {midiCover && (
        <MidiImportDialog
          chordsWanted={!coverMelodyOnly}
          initial={midiCover}
          onCancel={() => setMidiCover(null)}
          onOpen={imported => {
            setMidiCover(null);
            setAbc(imported.abc.trimEnd());
            setCot(imported.mode);
            if (imported.lyrics) setLyrics(current => (current.trim() ? current : imported.lyrics ?? ''));
            setSemanticTokens('');
            setShowNotation(true);
          }}
        />
      )}
    </section>
  );
};
