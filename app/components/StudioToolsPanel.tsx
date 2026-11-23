import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { SELF_READ, useKaraokeStatus, useOpenRouterSettings } from '../services/studioQueries';
import { karaokeReason, recogniseLyrics } from '../services/karaoke';
import { Check, ChevronRight, Copy, Download, FileAudio, KeyboardMusic, Loader2, Play, Scissors, SlidersHorizontal, Wand2 } from 'lucide-react';
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
import { FieldLabel } from './kit/FieldLabel';
import { ActionReason, ToolCard, ToolLauncher, ToolState } from './kit/ToolCard';
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

export function StudioToolsPanel({ initialSongId, onGenerateEffect, generationBusy }: {
  initialSongId?: string | null;
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
  const languages = ['ru', 'en', 'zh', 'ja', 'ko', 'pt', 'es', 'de', 'fr', 'it', 'uk'];
  const languageName = (code: string) => { try { return new Intl.DisplayNames([language], { type: 'language' }).of(code) ?? code; } catch { return code; } };
  const asrReason = asrWay === 'karaoke' && !karaokeRecogniser ? t('karaokeNoRecogniser')
    : asrWay === 'openrouter' && !cloudReady ? page.cloudKey
      : asrWay === 'openrouter' && asrModels.length === 0 ? t('noSpeechModel')
      : fromSong ? (!songId ? page.chooseTrack : null) : !asrFile ? page.chooseFile : null;
  const launcherButton = 'inline-flex items-center gap-1.5 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/15 dark:text-zinc-200';

  return (
    <div className="flex-1 overflow-y-auto bg-white px-5 pb-6 dark:bg-suno md:px-8">
      <div className="mx-auto max-w-4xl space-y-6">
        <header className="pt-6">
          <h1 className="text-2xl font-bold text-zinc-950 dark:text-white">{page.title}</h1>
          <p className="mt-2 max-w-3xl text-sm text-zinc-600 dark:text-zinc-400">{page.intro}</p>
        </header>

        <div className="sticky top-0 z-10 -mx-2 rounded-2xl border border-zinc-200 bg-white/95 px-4 py-3 backdrop-blur dark:border-white/10 dark:bg-zinc-950/90">
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

        <section className="space-y-4" aria-labelledby="tools-with-track">
          <div>
            <h2 id="tools-with-track" className="text-base font-semibold text-zinc-900 dark:text-white">{page.withTrack}</h2>
            <p className="text-xs text-zinc-500 dark:text-zinc-400">{page.withTrackHint}</p>
          </div>

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
            <details className="group text-xs">
              <summary className="flex cursor-pointer list-none items-center gap-1 font-semibold text-zinc-600 hover:text-zinc-900 dark:text-zinc-300 dark:hover:text-white [&::-webkit-details-marker]:hidden">
                <ChevronRight size={14} aria-hidden className="transition-transform group-open:rotate-90" />{page.options}
              </summary>
              <div className="mt-2 grid">
                <FieldLabel>{t('separationRuntime')}</FieldLabel>
                <fieldset disabled={busy || running || !status}>
                  <DevicePicker value={settings.runtime} onChange={choice => void saveSettings({ ...settings, runtime: choice })}
                    cardAvailable={Boolean(status?.card && status?.card_runtime_installed)} />
                </fieldset>
                {status?.card && !status?.card_runtime_installed && status?.engine !== 'roformer' && (
                  <button type="button" onClick={() => openSettings('models')} className="mt-2 justify-self-start text-left text-xs text-amber-700 underline underline-offset-2 dark:text-amber-300">
                    {t('separationInstallGpu')}{status?.card_missing_bytes ? ` · ${bytesText(status.card_missing_bytes)}` : ''}
                  </button>
                )}
              </div>
            </details>
            <ConfirmDialog
              isOpen={confirmReplace}
              title={t('replaceStemsTitle')}
              message={t('replaceStemsMessage').replace('{stems}', () => replacing.map(stemLabel).join(', '))}
              confirmLabel={t('replaceStemsConfirm')}
              onConfirm={() => { setConfirmReplace(false); void separate(); }}
              onCancel={() => setConfirmReplace(false)}
            />
          </ToolCard>

          <MidiTool songId={songId} songTitle={chosen?.title ?? ''} />

          <RestoreTool
            songId={songId}
            songTitle={chosen?.title ?? ''}
            restoredId={restoredOf(songs, songId)}
            chosenIsRestored={derivedBy(chosen, 'restore')}
            onDone={() => void loadSongs()}
          />

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
              <p className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">{t(asrWay === 'karaoke' ? 'transcribeWayKaraokeHint' : 'transcribeWayOpenRouterHint')}</p>
            </div>
            {asrWay === 'openrouter' && cloudReady && asrModels.length > 0 && (
              <label className="grid text-xs">
                <FieldLabel>{page.model}</FieldLabel>
                <select value={asrModel} onChange={event => setAsrModel(event.target.value)} className={CONTROL}>
                  {asrModels.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}
                </select>
              </label>
            )}
            {asrWay === 'karaoke' && (
              <CatalogSegments<'song' | 'file'> fill label={t('restoreFrom')} value={asrSource} onChange={setAsrSource}
                options={[{ value: 'song', label: page.fromTrack }, { value: 'file', label: page.fromFile }]} />
            )}
            {!fromSong && (
              <div>
                <SecondaryButton aria-label={`${page.fromFile}: ${asrFile?.name ?? t('chooseAudioFile')}`} onClick={() => filePicker.current?.click()}>
                  <FileAudio size={13} aria-hidden />{asrFile?.name ?? t('chooseAudioFile')}
                </SecondaryButton>
                <input ref={filePicker} type="file" accept="audio/*" className="hidden"
                  onChange={event => { setAsrFile(event.target.files?.[0] ?? null); event.target.value = ''; }} />
              </div>
            )}
            <label className="grid text-xs">
              <FieldLabel>{page.language}</FieldLabel>
              <select value={asrLanguage} onChange={event => setAsrLanguage(event.target.value)} className={`${CONTROL} max-w-xs`}>
                <option value="">{page.autoLanguage}</option>
                {languages.map(code => <option key={code} value={code}>{languageName(code)}</option>)}
              </select>
            </label>
          </ToolCard>

          <div className="rounded-2xl border border-zinc-200 bg-zinc-50 dark:border-white/10 dark:bg-white/3">
            <p className="px-4 pt-3 text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{page.openIn}</p>
            <ToolLauncher icon={<Wand2 size={17} />} title={t('processTitle')} description={t('processHint')}>
              <button type="button" disabled={!songId} className={launcherButton}
                onClick={() => {
                  if (!songId) return;
                  setProcessError(null);
                  void fetch(`/v1/library/songs/${encodeURIComponent(songId)}`)
                    .then(response => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
                    .then(body => window.dispatchEvent(new CustomEvent('yue:process-song', { detail: mapNativeLibrarySong(body) })))
                    .catch(problem => setProcessError(problem instanceof Error ? problem.message : String(problem)));
                }}>
                <Wand2 size={13} /> {t('processMenu')}
              </button>
              {!songId && <ActionReason>{page.chooseTrack}</ActionReason>}
              {processError && <p role="alert" className="mt-1 text-xs text-rose-600 dark:text-rose-300">{processError}</p>}
            </ToolLauncher>
            <div className="border-t border-zinc-200 dark:border-white/10" />
            <ToolLauncher icon={<SlidersHorizontal size={17} />} title={t('audioEditor')} description={t('audioEditorIntro')}>
              <button type="button" disabled={!songId} className={launcherButton}
                onClick={() => { if (songId) void openAudioEditor(apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`)); }}>
                <SlidersHorizontal size={13} /> {t('openTrackInEditor')}
              </button>
              {!songId && <ActionReason>{page.chooseTrack}</ActionReason>}
            </ToolLauncher>
          </div>
        </section>

        <section className="space-y-4" aria-labelledby="tools-create">
          <div>
            <h2 id="tools-create" className="text-base font-semibold text-zinc-900 dark:text-white">{page.createNew}</h2>
            <p className="text-xs text-zinc-500 dark:text-zinc-400">{page.createNewHint}</p>
          </div>
          {onGenerateEffect && <SoundEffectsTool control={CONTROL} busy={Boolean(generationBusy)} onGenerate={onGenerateEffect} />}
          {onGenerateEffect && <ControlFoleyTool control={CONTROL} busy={Boolean(generationBusy)} onGenerate={onGenerateEffect} />}
          <div className="rounded-2xl border border-zinc-200 bg-zinc-50 dark:border-white/10 dark:bg-white/3">
            <ToolLauncher icon={<KeyboardMusic size={17} />} title={t('midiEdTitle')} description={t('midiEdIntro')}>
              <button type="button" onClick={() => setMidiEditorOpen(true)} className={launcherButton}>
                <KeyboardMusic size={13} /> {t('midiEdOpen')}
              </button>
            </ToolLauncher>
          </div>
          {midiEditorOpen && <MidiEditor source={{ kind: 'new' }} onClose={() => setMidiEditorOpen(false)} />}
        </section>
      </div>
    </div>
  );
}
