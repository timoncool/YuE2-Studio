import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { SELF_READ, useKaraokeStatus, useOpenRouterSettings } from '../services/studioQueries';
import { karaokeReason, recogniseLyrics } from '../services/karaoke';
import { AudioWaveform, Check, ChevronLeft, ChevronRight, Clapperboard, Copy, Download, FileAudio, KeyboardMusic, Loader2, Piano, Play, Scissors, SlidersHorizontal, Sparkles, Wand2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { poll } from '../services/poll';
import { saveFile } from '../services/saveFile';
import { tempoAndKey } from '../services/songDownload';
import { DevicePicker } from './DevicePicker';
import { transcribeWithNativeOpenRouter } from '../services/nativeOpenRouter';
import { apiUrl } from '../services/apiBase';
import { openAudioEditor } from '../services/audioEditor';
import { StemMixer } from './StemMixer';
import { mapNativeLibrarySong } from '../services/nativeLibrary';
import type { NativeLibrarySong, YueRequest } from '../types';
import { ControlFoleyTool } from './ControlFoleyTool';
import { SoundEffectsTool } from './SoundEffectsTool';
import { stemOf } from '../services/songStems';
import { derivedBy, libraryFailure, restoredOf, toolsSongOnOpen, type ToolRun } from '../services/toolsSong';
import { RestoreTool } from './RestoreTool';
import { ConfirmDialog } from './ConfirmDialog';
import { MidiTool } from './midi/MidiTool';
import { MidiEditor } from './midi/MidiEditor';
import { WorkProgress } from './WorkProgress';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import { CatalogSegments } from './kit/Catalog';
import { Select } from './kit/Select';
import Combobox from './kit/Combobox';
import { FieldLabel } from './kit/FieldLabel';
import { ActionReason, ToolCard, ToolState } from './kit/ToolCard';
import { LibrarySongPicker } from './LibrarySongPicker';
import { toolsPageStrings } from '../i18n/toolsPage';
import { formatSize } from '../services/fileSize';
import { ManualRuntimeArchive, type RuntimeArchive } from './ManualDownloads';

/**
 * Studio tools.
 *
 * Work done to a finished track rather than to a new one: splitting a song into
 * its parts, and turning a reference recording into text you can paste into a
 * lyric sheet. Both run through the studio's own service - the separator on the
 * ONNX Runtime it already ships, the transcription through whichever recogniser
 * the user connected.
 *
 * The page used to be diagnostics. Those moved to Settings, next to the flags
 * that produce them, which left this page for the tools it is named after.
 */


interface SeparationStatus {
  model: { label: string; bytes: number; installed: boolean; note: string };
  runtime_installed: boolean;
  missing_bytes?: number;
  ready: boolean;
  stems: string[];
  download: { downloaded_bytes: number; total_bytes: number; done: boolean; error?: string | null; manual_archive?: RuntimeArchive | null } | null;
  /** The card audio.cpp separates on: CUDA on an NVIDIA card that runs it, Vulkan on another make, Metal on a Mac. */
  card: 'cuda' | 'vulkan' | 'metal' | null;
  card_runtime_installed: boolean;
  card_missing_bytes: number;
  /** The separator: HT-Demucs for six stems, Mel-Band RoFormer for the vocals and the instrumental. */
  engine?: 'htdemucs' | 'roformer';
  settings: { stems: string[]; runtime: 'auto' | 'cuda' | 'cpu'; engine?: 'htdemucs' | 'roformer' };
  run: { song_id: string; progress: number; started_ms: number; elapsed_ms?: number; done: boolean; error: string | null; stems: string[] } | null;
}

interface CatalogModel {
  id: string;
  name: string;
  capabilities: string[];
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/30 dark:text-white';

export const TOOL_IDS = ['stems', 'midi', 'restore', 'recognise', 'process', 'editor', 'effects', 'foley', 'midi-editor'] as const;
export type ToolId = typeof TOOL_IDS[number];
const TOOL_KEY = 'yue2.tools.tool';
const rememberedTool = (): ToolId => {
  try {
    const saved = localStorage.getItem(TOOL_KEY);
    return (TOOL_IDS as readonly string[]).includes(saved ?? '') ? saved as ToolId : 'stems';
  } catch {
    return 'stems';
  }
};

export function StudioToolsPanel({ initialSongId, initialTool, onGenerateEffect, generationBusy }: {
  initialSongId?: string | null;
  /** The tool a menu or the address asked for; without one the list comes first on a phone. */
  initialTool?: ToolId | null;
  onGenerateEffect?: (request: YueRequest) => void;
  generationBusy?: boolean;
} = {}): React.ReactElement {
  const { t, language } = useI18n();
  const page = toolsPageStrings[language];
  const bytesText = (bytes: number) => formatSize(bytes, language);

  const [songs, setSongs] = useState<NativeLibrarySong[]>([]);
  const [midiEditorOpen, setMidiEditorOpen] = useState(false);
  const [songId, setSongId] = useState('');
  const [status, setStatus] = useState<SeparationStatus | null>(null);
  const [stems, setStems] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const settingsRevision = useRef(0);
  // separating again replaces the stems the song has: asked first
  const [confirmReplace, setConfirmReplace] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [asrError, setAsrError] = useState<string | null>(null);
  const [processError, setProcessError] = useState<string | null>(null);
  const [asrSource, setAsrSource] = useState<'song' | 'file'>('song');
  const [restoreFromFile, setRestoreFromFile] = useState(false);
  const [tool, setTool] = useState<ToolId>(() => initialTool ?? rememberedTool());
  const [listOnly, setListOnly] = useState(!initialTool);
  useEffect(() => { if (initialTool) { setTool(initialTool); setListOnly(false); } }, [initialTool]);
  const choose = (next: ToolId) => {
    setTool(next);
    setListOnly(false);
    try { localStorage.setItem(TOOL_KEY, next); } catch { /* the choice is a convenience */ }
    window.history.replaceState(window.history.state, '', `/tools/${next}`);
  };

  const [asrModels, setAsrModels] = useState<CatalogModel[]>([]);
  const [asrModel, setAsrModel] = useState('');
  const [asrFile, setAsrFile] = useState<File | null>(null);
  const [asrLanguage, setAsrLanguage] = useState('');
  const [asrBusy, setAsrBusy] = useState(false);
  const [asrText, setAsrText] = useState('');
  const { data: karaoke } = useKaraokeStatus();
  const cloudReady = useOpenRouterSettings<{ configured: boolean }>().data?.configured === true;
  const karaokeRecogniser = Boolean(karaoke?.provider && karaoke.provider !== 'none');
  const [chosenWay, setChosenWay] = useState<'karaoke' | 'openrouter' | null>(null);
  // the karaoke recogniser hears the file on this computer when one is chosen; OpenRouter otherwise
  const asrWay = chosenWay ?? (karaokeRecogniser ? 'karaoke' : 'openrouter');
  const [copied, setCopied] = useState(false);
  const filePicker = useRef<HTMLInputElement | null>(null);

  // an empty list before the library has answered is not "no songs"
  const [songsLoaded, setSongsLoaded] = useState(false);
  const [songsError, setSongsError] = useState<string | null>(null);
  const loadSongs = useCallback(async () => {
    try {
      const response = await fetch('/v1/library/songs');
      if (!response.ok) throw new Error(libraryFailure(response.status, await response.text()));
      const body = await response.json();
      const list: NativeLibrarySong[] = Array.isArray(body) ? body : body.songs ?? [];
      const playable = list.filter(song => song.audio_path);
      setSongs(playable);
      setSongsError(null);
      const runOf = (path: string) =>
        fetch(path, { headers: SELF_READ })
          .then(response => response.json() as Promise<{ run?: ToolRun | null }>)
          .then(body => body.run ?? null)
          .catch((error: unknown) => {
            console.error(`[ERROR] the run at ${path} was not read:`, error);
            return null;
          });
      const runs = await Promise.all([runOf('/v1/separation/status'), runOf('/v1/midi')]);
      setSongId(current => current || toolsSongOnOpen(playable.map(song => song.id), runs));
    } catch (failure) {
      setSongsError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setSongsLoaded(true);
    }
  }, []);

  // a song made while the page is open is chosen in the picker before this list knows it
  const reloadedFor = useRef('');
  useEffect(() => {
    if (!songId || !songsLoaded || reloadedFor.current === songId || songs.some(song => song.id === songId)) return;
    reloadedFor.current = songId;
    void loadSongs();
  }, [songId, songsLoaded, songs, loadSongs]);

  // A track handed over by a menu wins over whatever was chosen here before.
  useEffect(() => {
    if (initialSongId) setSongId(initialSongId);
  }, [initialSongId]);

  useEffect(() => {
    void loadSongs();
    void fetch('/v1/openrouter/catalog')
      .then(response => response.json())
      .then((body: { models?: CatalogModel[] }) => {
        const recognisers = (body.models ?? []).filter(model => model.capabilities.includes('speech_to_text'));
        setAsrModels(recognisers);
        setAsrModel(current => current || recognisers[0]?.id || '');
      })
      .catch(() => undefined);
  }, [loadSongs]);

  // The separation state, and which stems the chosen track already has.
  const refresh = useCallback(async () => {
    const revision = settingsRevision.current;
    const response = await fetch('/v1/separation/status', { headers: SELF_READ });
    if (!response.ok) throw new Error(libraryFailure(response.status, await response.text()));
    const separation: SeparationStatus = await response.json();
    if (revision !== settingsRevision.current) return;
    setStatus(separation);
    if (songId) {
      const body = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/stems`, { headers: SELF_READ }).then(response => response.json());
      setStems(body.stems ?? []);
    }
  }, [songId]);

  useEffect(() => {
    return poll(() => refresh().catch(() => undefined), 1000);
  }, [refresh]);

  const saveSettings = async (next: SeparationStatus['settings']) => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch('/v1/separation/settings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(next),
      });
      if (!response.ok) {
        const body = await response.json().catch(() => null);
        throw new Error(body?.error || `${t('error')} (${response.status})`);
      }
      settingsRevision.current += 1;
      setStatus(null);
      await refresh();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const changeStemDownload = async (action: 'install' | 'cancel') => {
    setBusy(true);
    setError(null);
    try {
      const path = action === 'install' ? '/v1/separation/install' : '/v1/separation/runtime/cancel';
      const response = await fetch(path, { method: 'POST' });
      if (!response.ok) {
        const body = await response.json().catch(() => null);
        throw new Error(body?.error || `${t('error')} (${response.status})`);
      }
      await refresh();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  // a stem is a part of a song already: there is nothing in it to separate
  const chosenIsStem = useMemo(() => {
    const chosen = songs.find(song => song.id === songId);
    return chosen ? stemOf(mapNativeLibrarySong(chosen)) !== null : false;
  }, [songs, songId]);

  const separate = async () => {
    if (!songId) return;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/stems`, { method: 'POST' });
      if (!response.ok) {
        const body = await response.json().catch(() => null);
        throw new Error(body?.error || `Separation could not start (${response.status})`);
      }
      await refresh();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const fromSong = asrWay === 'karaoke' && asrSource === 'song';
  const transcribe = async () => {
    if (fromSong ? !songId : !asrFile) return;
    if (asrWay === 'openrouter' && !asrModel) return;
    setAsrBusy(true);
    setAsrError(null);
    setAsrText('');
    try {
      const language = asrLanguage || undefined;
      setAsrText(asrWay === 'karaoke'
        ? await recogniseLyrics(fromSong ? { songId } : { file: asrFile! }, language)
        : await transcribeWithNativeOpenRouter(asrModel, asrFile!, language));
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setAsrError(karaokeReason(t, message) ?? message);
    } finally {
      setAsrBusy(false);
    }
  };

  const copyText = async () => {
    try {
      await navigator.clipboard.writeText(asrText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard permission is not worth an error banner.
    }
  };

  const settings = status?.settings ?? { stems: status?.stems ?? [], runtime: 'auto' as const };
  // what separating again replaces: the stems it makes that the song has already
  const replacing = stems.filter(stem => settings.stems.includes(stem));
  const run = status?.run?.song_id === songId ? status?.run : null;
  const running = Boolean(run && !run.done);
  const percent = run && run.progress > 0 ? Math.round(run.progress * 100) : null;
  const download = status?.download && !status.download.done ? status.download : null;
  const chosen = songs.find(song => song.id === songId);
  const facts = [
    stems.length > 0 && page.hasStems(stems.length),
    chosenIsStem && page.isStem,
    derivedBy(chosen, 'restore') && page.restored,
  ].filter(Boolean).join(' · ');
  const serverMissing = Boolean(status && status.engine !== 'roformer' && status.model.installed && !status.ready);
  const stemsReason = !songId ? page.chooseTrack : chosenIsStem ? t('stemOfStem') : settings.stems.length === 0 ? page.whichStems : null;
  const stemLabel = (stem: string) => t(`stem_${stem}` as never) || stem;
  const openSettings = (detail: string) => window.dispatchEvent(new CustomEvent('yue:open-settings', { detail }));
  const languages = ['ru', 'en', 'uk', 'be', 'kk', 'zh', 'ja', 'ko', 'pt', 'es', 'de', 'fr', 'it', 'pl', 'cs', 'nl', 'sv', 'fi', 'no', 'da', 'tr', 'el', 'he', 'ar', 'fa', 'hi', 'id', 'vi', 'th', 'hu', 'ro', 'bg', 'sr', 'hr', 'lt', 'lv', 'et'];
  const languageName = (code: string) => { try { return new Intl.DisplayNames([language], { type: 'language' }).of(code) ?? code; } catch { return code; } };
  const asrReason = asrWay === 'karaoke' && !karaokeRecogniser ? t('karaokeNoRecogniser')
    : asrWay === 'openrouter' && !cloudReady ? page.cloudKey
      : asrWay === 'openrouter' && asrModels.length === 0 ? t('noSpeechModel')
      : fromSong ? (!songId ? page.chooseTrack : null) : !asrFile ? page.chooseFile : null;

  const tools: { id: ToolId; group: 'track' | 'new'; icon: React.ReactNode; title: string; hint: string }[] = [
    { id: 'stems', group: 'track', icon: <Scissors size={16} />, title: t('stemsTitle'), hint: page.navStems },
    { id: 'midi', group: 'track', icon: <Piano size={16} />, title: t('midiTitle'), hint: page.navMidi },
    { id: 'restore', group: 'track', icon: <Sparkles size={16} />, title: t('restoreTitle'), hint: page.navRestore },
    { id: 'recognise', group: 'track', icon: <FileAudio size={16} />, title: page.recognise, hint: page.navRecognise },
    { id: 'process', group: 'track', icon: <Wand2 size={16} />, title: t('processTitle'), hint: page.navProcess },
    { id: 'editor', group: 'track', icon: <SlidersHorizontal size={16} />, title: t('audioEditor'), hint: page.navEditor },
    ...(onGenerateEffect ? [
      { id: 'effects' as const, group: 'new' as const, icon: <AudioWaveform size={16} />, title: page.effectsTitle, hint: page.navEffects },
      { id: 'foley' as const, group: 'new' as const, icon: <Clapperboard size={16} />, title: page.foleyTitle, hint: page.navFoley },
    ] : []),
    { id: 'midi-editor', group: 'new', icon: <KeyboardMusic size={16} />, title: t('midiEdTitle'), hint: page.navMidiEditor },
  ];
  const current = tools.find(entry => entry.id === tool) ?? tools[0];
  const needsTrack = current.group === 'track' && !(current.id === 'recognise' && !fromSong) && !(current.id === 'restore' && restoreFromFile);

  const trackBar = (
    <div className="sticky top-0 z-10 -mx-1 mb-4 rounded-xl border border-zinc-200 bg-white px-3 py-2.5 dark:border-white/10 dark:bg-zinc-950">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <FieldLabel hidden>{page.track}</FieldLabel>
        <div className="min-w-0 flex-1 basis-64">
          <LibrarySongPicker label={page.track} value={songId} placeholder={t('searchYourSongs')} showLabel={false} controlClassName={CONTROL}
            onChange={id => setSongId(id)} />
        </div>
        {facts && <span className="text-xs text-zinc-600 dark:text-zinc-300">{facts}</span>}
      </div>
      {songsError && <p role="alert" className="mt-2 text-xs text-rose-600 dark:text-rose-300">{t('toolsSongsFailed').replace('{error}', songsError)}
        {' '}<button type="button" className="underline" onClick={() => void loadSongs()}>{t('refresh')}</button></p>}
      {songsLoaded && !songsError && songs.length === 0 && <p className="mt-2 text-xs text-zinc-500 dark:text-zinc-400">{t('noSongsYet')}</p>}
    </div>
  );

  const pane = (() => {
    switch (current.id) {
      case 'stems': return (
          <ToolCard icon={<Scissors size={17} />} title={t('stemsTitle')}
          state={status && (status.ready ? <ToolState tone="ready">{page.ready}</ToolState>
            : download ? <ToolState tone="busy">{page.downloading(status.model.label, bytesText(download.downloaded_bytes), bytesText(download.total_bytes))}</ToolState>
              : serverMissing ? <ToolState tone="blocked">{page.unavailable}</ToolState>
                : <ToolState tone="missing">{page.needsDownload(bytesText(status.missing_bytes ?? status.model.bytes))}</ToolState>)}
          description={status?.engine === 'roformer' ? t('stemsHintRoformer' as never) : t('stemsHint')}
          action={status && !status.ready && !serverMissing ? (download ? (
            <div className="flex items-end gap-2">
              <div className="min-w-0 flex-1">
                <WorkProgress label={page.downloading(status.model.label, bytesText(download.downloaded_bytes), bytesText(download.total_bytes))}
                  percent={Math.round((download.downloaded_bytes / Math.max(1, download.total_bytes)) * 100)} />
              </div>
              <SecondaryButton disabled={busy} onClick={() => void changeStemDownload('cancel')}>{t('cancelDownload')}</SecondaryButton>
            </div>
          ) : (
            <PrimaryButton disabled={busy} onClick={() => void changeStemDownload('install')}>
              <Download size={14} />{page.download(status.model.label, bytesText(status.missing_bytes ?? status.model.bytes))}
            </PrimaryButton>
          )) : (
            <>
              <PrimaryButton onClick={() => (replacing.length > 0 ? setConfirmReplace(true) : void separate())}
                disabled={!status?.ready || Boolean(stemsReason) || running || busy}>
                {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
                {stems.length > 0 ? t('stemsAgain') : t('stemsStart')}
              </PrimaryButton>
              {serverMissing ? <ActionReason>{t('updateWhyAudiocppServer')} ·{' '}
                <button type="button" onClick={() => openSettings('models')} className="underline underline-offset-2">{t('changeProfileHint')}</button></ActionReason>
                : stemsReason && !running && <ActionReason>{stemsReason}</ActionReason>}
              {running && <WorkProgress label={t('stemsRunning')} percent={percent} elapsedMs={run?.elapsed_ms} />}
            </>
          )}
          result={stems.length > 0 && songId && (
            <StemMixer
              // the audio graph is wired once for a set of stems: another set is another mixer
              key={`${songId}:${stems.join(',')}`}
              stems={stems}
              urlFor={stem => apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/stems/${stem}`)}
              mixUrl={apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`)}
              label={stemLabel}
              onDownload={stem => {
                const tempoKey = chosen ? tempoAndKey((chosen.generation_settings as Record<string, unknown> | null | undefined)?.abc) : '';
                void saveFile(`${chosen?.title ?? 'track'} - ${stem}${tempoKey ? ` - ${tempoKey}` : ''}.wav`, { url: apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/stems/${stem}`) });
              }}
              onEdit={stem => void openAudioEditor(apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/stems/${stem}`))}
            />
          )}
          error={(run?.error || error || download?.error || status?.download?.error) && <>
            <p role="alert" className="select-text rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 wrap-anywhere dark:text-rose-300">{run?.error || error || status?.download?.error}</p>
            {status?.download?.error && <ManualRuntimeArchive archive={status.download.manual_archive} />}
          </>}
          footer={status && <>{page.model}: {status.model.label}</>}>
<div role="radiogroup" aria-label={page.separationModel} className="grid">
            <FieldLabel hidden>{page.separationModel}</FieldLabel>
            <div className="grid gap-2 sm:grid-cols-2">
              {([['htdemucs', page.sepSix, page.sepSixHint], ['roformer', page.sepVocal, page.sepVocalHint]] as const).map(([id, name, hint]) => {
                const on = (settings.engine ?? status?.engine ?? 'htdemucs') === id;
                return (
                  <button key={id} type="button" role="radio" aria-checked={on} disabled={busy || running || !status}
                    onClick={() => !on && void saveSettings({ ...settings, engine: id })}
                    className={`rounded-lg border px-3 py-2.5 text-left disabled:opacity-60 ${on ? 'border-pink-400 bg-pink-500/10' : 'border-zinc-200 hover:border-pink-300 dark:border-white/10'}`}>
                    <span className="block text-sm font-semibold text-zinc-900 dark:text-white">{name}</span>
                    <span className="block text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">{hint}</span>
                  </button>
                );
              })}
            </div>
          </div>
          <div className="grid">
            <FieldLabel>{page.whichStems}</FieldLabel>
            <div className="flex flex-wrap gap-2">
              {(status?.stems ?? []).map(stem => {
                const on = settings.stems.includes(stem);
                return (
                  <button key={stem} type="button" role="checkbox" aria-checked={on} disabled={busy || running}
                    onClick={() => void saveSettings({ ...settings, stems: on ? settings.stems.filter(name => name !== stem) : [...settings.stems, stem] })}
                    className={`inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-semibold ${on ? 'border-pink-400 bg-pink-500/10 text-zinc-900 dark:text-white' : 'border-zinc-200 text-zinc-600 dark:border-white/10 dark:text-zinc-300'}`}>
                    <span className={`grid h-3.5 w-3.5 place-items-center rounded-sm ${on ? 'bg-pink-500 text-white' : 'bg-zinc-200 dark:bg-white/10'}`}>
                      {on && <Check size={9} strokeWidth={3} />}
                    </span>
                    {stemLabel(stem)}
                  </button>
                );
              })}
            </div>
          </div>
          <div className="grid text-xs">
            <FieldLabel>{t('separationRuntime')}</FieldLabel>
            <fieldset disabled={busy || running || !status}>
              <DevicePicker value={settings.runtime} onChange={choice => void saveSettings({ ...settings, runtime: choice })}
                cardAvailable={Boolean(status?.card && status?.card_runtime_installed)} />
            </fieldset>
            {status?.card && !status?.card_runtime_installed && status?.engine !== 'roformer' && (
              <button type="button" onClick={() => openSettings('models')} className="mt-2 justify-self-start text-left text-[13px] text-amber-700 underline underline-offset-2 dark:text-amber-300">
                {t('separationInstallGpu')}{status?.card_missing_bytes ? ` · ${bytesText(status.card_missing_bytes)}` : ''}
              </button>
            )}
          </div>
          <ConfirmDialog
            isOpen={confirmReplace}
            title={t('replaceStemsTitle')}
            message={t('replaceStemsMessage').replace('{stems}', () => replacing.map(stemLabel).join(', '))}
            confirmLabel={t('replaceStemsConfirm')}
            onConfirm={() => { setConfirmReplace(false); void separate(); }}
            onCancel={() => setConfirmReplace(false)}
          />
        </ToolCard>
      );
      case 'midi': return <MidiTool songId={songId} songTitle={chosen?.title ?? ''} />;
      case 'restore': return <RestoreTool songId={songId} songTitle={chosen?.title ?? ''} restoredId={restoredOf(songs, songId)}
        chosenIsRestored={derivedBy(chosen, 'restore')} onDone={() => void loadSongs()} onSourceChange={from => setRestoreFromFile(from === 'file')} />;
      case 'recognise': return (
          <ToolCard icon={<FileAudio size={17} />} title={page.recognise}
          description={page.recogniseIntro}
          action={<>
            <PrimaryButton onClick={() => void transcribe()} disabled={Boolean(asrReason) || asrBusy}>
              {asrBusy ? <Loader2 size={14} className="animate-spin" /> : <FileAudio size={14} />} {page.recognise}
            </PrimaryButton>
            {asrReason && <ActionReason>{asrReason}{asrWay === 'openrouter' && !cloudReady && <>{' '}
              <button type="button" onClick={() => openSettings('cloud')} className="underline underline-offset-2">{page.configure}</button></>}
              {asrWay === 'karaoke' && !karaokeRecogniser && <>{' '}
              <button type="button" onClick={() => openSettings('models')} className="underline underline-offset-2">{page.configure}</button></>}</ActionReason>}
          </>}
          result={asrText && (
            <div className="grid gap-2">
              <textarea readOnly value={asrText} rows={8} className={`${CONTROL} resize-y font-mono text-[13px] leading-5`} />
              <div className="flex flex-wrap gap-2">
                <SecondaryButton onClick={() => void copyText()}>{copied ? <Check size={13} /> : <Copy size={13} />} {copied ? t('copied') : t('copy')}</SecondaryButton>
                <SecondaryButton onClick={() => void saveFile(`${(fromSong ? chosen?.title : asrFile?.name?.replace(/\.[^.]+$/, '')) || 'lyrics'}.txt`, { blob: new Blob([asrText], { type: 'text/plain;charset=utf-8' }) })}>
                  <Download size={13} /> {page.saveText}</SecondaryButton>
              </div>
            </div>
          )}
          error={asrError && <p role="alert" className="select-text rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 wrap-anywhere dark:text-rose-300">{asrError}</p>}>
          <div className="grid">
            <FieldLabel hidden>{page.recogniseWay}</FieldLabel>
            <CatalogSegments<'karaoke' | 'openrouter'> fill label={page.recogniseWay} value={asrWay} onChange={setChosenWay}
              options={[{ value: 'karaoke', label: page.wayLocal }, { value: 'openrouter', label: page.wayCloud }]} />
            <p className="mt-1.5 text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">{t(asrWay === 'karaoke' ? 'transcribeWayKaraokeHint' : 'transcribeWayOpenRouterHint')}</p>
          </div>
          {asrWay === 'openrouter' && cloudReady && asrModels.length > 0 && (
            <div className="grid text-xs">
              <FieldLabel hidden>{page.model}</FieldLabel>
              <Select label={page.model} value={asrModel} onChange={setAsrModel} inputClassName={CONTROL}
                options={asrModels.map(model => ({ value: model.id, label: model.name }))} />
            </div>
          )}
          <div className="grid">
          <FieldLabel>{page.source}</FieldLabel>
          {asrWay === 'karaoke' ? (
            <CatalogSegments<'song' | 'file'> fill label={page.source} value={asrSource} onChange={setAsrSource}
              options={[{ value: 'song', label: page.fromTrack }, { value: 'file', label: page.fromFile }]} />
          ) : <p className="text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">{page.cloudFileOnly}</p>}
        </div>
          {!fromSong && (
            <div>
              <SecondaryButton aria-label={`${page.fromFile}: ${asrFile?.name ?? t('chooseAudioFile')}`} onClick={() => filePicker.current?.click()}>
                <FileAudio size={13} aria-hidden />{asrFile?.name ?? t('chooseAudioFile')}
              </SecondaryButton>
              <input ref={filePicker} type="file" accept="audio/*" className="hidden"
                onChange={event => { setAsrFile(event.target.files?.[0] ?? null); event.target.value = ''; }} />
            </div>
          )}
          <div className="grid text-xs">
            <FieldLabel hidden>{page.language}</FieldLabel>
            <Combobox label={page.language} value={asrLanguage} onChange={setAsrLanguage} className="max-w-xs" inputClassName={CONTROL}
              options={[{ value: '', label: page.autoLanguage }, ...languages.map(code => ({ value: code, label: languageName(code) }))]} />
          </div>
        </ToolCard>
      );
      case 'process': return (
        <ToolCard icon={<Wand2 size={17} />} title={t('processTitle')} description={t('processHint')}
          action={<>
            <PrimaryButton disabled={!songId}
              onClick={() => {
                if (!songId) return;
                setProcessError(null);
                void fetch(`/v1/library/songs/${encodeURIComponent(songId)}`)
                  .then(response => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
                  .then(body => window.dispatchEvent(new CustomEvent('yue:process-song', { detail: mapNativeLibrarySong(body) })))
                  .catch(problem => setProcessError(problem instanceof Error ? problem.message : String(problem)));
              }}>
              <Wand2 size={14} /> {t('processMenu')}
            </PrimaryButton>
            {!songId && <ActionReason>{page.chooseTrack}</ActionReason>}
          </>}
          error={processError && <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">{processError}</p>} />
      );
      case 'editor': return (
        <ToolCard icon={<SlidersHorizontal size={17} />} title={t('audioEditor')} description={t('audioEditorIntro')}
          action={<>
            <PrimaryButton disabled={!songId} onClick={() => { if (songId) void openAudioEditor(apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`)); }}>
              <SlidersHorizontal size={14} /> {t('openTrackInEditor')}
            </PrimaryButton>
            {!songId ? <ActionReason>{page.chooseTrack}</ActionReason> : stems.length > 0 && <ActionReason>{page.editorStems}</ActionReason>}
          </>} />
      );
      case 'effects': return onGenerateEffect ? <SoundEffectsTool control={CONTROL} busy={Boolean(generationBusy)} onGenerate={onGenerateEffect} /> : null;
      case 'foley': return onGenerateEffect ? <ControlFoleyTool control={CONTROL} busy={Boolean(generationBusy)} onGenerate={onGenerateEffect} /> : null;
      case 'midi-editor': return (
        <ToolCard icon={<KeyboardMusic size={17} />} title={t('midiEdTitle')} description={t('midiEdIntro')}
          action={<PrimaryButton onClick={() => setMidiEditorOpen(true)}><KeyboardMusic size={14} /> {t('midiEdOpen')}</PrimaryButton>} />
      );
    }
  })();

  const navItem = (entry: typeof tools[number]) => (
    <li key={entry.id}>
      <button type="button" aria-current={entry.id === current.id ? 'page' : undefined} onClick={() => choose(entry.id)}
        className={`flex w-full items-start gap-2.5 rounded-lg px-2.5 py-2 text-left transition-colors ${entry.id === current.id && !listOnly
          ? 'bg-pink-500/10 text-zinc-950 dark:text-white' : 'text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-white/5'}`}>
        <span className={`mt-0.5 shrink-0 ${entry.id === current.id && !listOnly ? 'text-pink-500' : 'text-zinc-400'}`} aria-hidden>{entry.icon}</span>
        <span className="min-w-0">
          <span className={`block text-sm ${entry.id === current.id && !listOnly ? 'font-semibold' : 'font-medium'}`}>{entry.title}</span>
          <span className="block text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">{entry.hint}</span>
        </span>
      </button>
    </li>
  );

  return (
    <div className="flex-1 overflow-y-auto bg-white dark:bg-suno">
      <div className="mx-auto max-w-6xl px-5 py-6 md:px-8">
        <header className={`mb-6 ${listOnly ? '' : 'max-md:hidden'}`}>
          <h1 className="text-2xl leading-8 font-bold text-zinc-950 dark:text-white">{page.title}</h1>
          <p className="mt-1 max-w-3xl text-sm text-zinc-600 dark:text-zinc-400">{page.intro}</p>
        </header>
        <div className="grid gap-6 md:grid-cols-[16rem_1fr]">
        <nav aria-label={page.title} className={`${listOnly ? '' : 'max-md:hidden'} md:sticky md:top-0 md:self-start`}>
          {(['track', 'new'] as const).map(group => (
            <div key={group} className="mb-5 last:mb-0">
              <h2 className="mb-1.5 px-2.5 text-xs font-semibold uppercase tracking-[0.04em] text-zinc-600 dark:text-zinc-400">{group === 'track' ? page.withTrack : page.createNew}</h2>
              <ul className="mt-1 space-y-0.5">{tools.filter(entry => entry.group === group).map(navItem)}</ul>
            </div>
          ))}
        </nav>
        <main aria-label={current.title} className={`min-w-0 ${listOnly ? 'max-md:hidden' : ''}`}>
          <button type="button" onClick={() => setListOnly(true)} className="mb-3 inline-flex items-center gap-1 text-sm font-semibold text-zinc-600 hover:text-pink-600 md:hidden dark:text-zinc-300">
            <ChevronLeft size={16} aria-hidden />{page.title}
          </button>
          {needsTrack && trackBar}
          {pane}
        </main>
        </div>
      </div>
      {midiEditorOpen && <MidiEditor source={{ kind: 'new' }} onClose={() => setMidiEditorOpen(false)} />}
    </div>
  );
}
