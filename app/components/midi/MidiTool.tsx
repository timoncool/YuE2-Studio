import React, { useCallback, useEffect, useRef, useState } from 'react';
import { SELF_READ } from '../../services/studioQueries';
import { Download, Loader2, PencilLine, Piano, Play, Square, Trash2 } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { saveFile } from '../../services/saveFile';
import { apiUrl } from '../../services/apiBase';
import { MidiEditor } from './MidiEditor';
import { MidiPlayer, type HeardNote } from './MidiPlayer';
import { WorkProgress } from '../WorkProgress';
import { ManualRuntimeArchive, type RuntimeArchive } from '../ManualDownloads';
import { ConfirmDialog } from '../ConfirmDialog';
import { PrimaryButton, SecondaryButton } from '../kit/Buttons';
import { ActionReason, ToolCard, ToolState } from '../kit/ToolCard';
import { FieldLabel } from '../kit/FieldLabel';
import { ToolModelPicker } from '../ToolModelPicker';
import { toolsPageStrings } from '../../i18n/toolsPage';
import { formatSize } from '../../services/fileSize';

/**
 * Audio to MIDI on the tools page, for the track chosen there: a song, a
 * stem, a processed take. The transcriber and its weights are optional; the
 * first transcription fetches them, or the user does beforehand.
 */

interface MidiSize {
  id: 'small' | 'medium' | 'large' | 'vocal';
  params: string;
  bytes: number;
  installed: boolean;
  missing_bytes: number;
}

interface MidiRun {
  song_id: string | null;
  size: string;
  stage: 'preparing' | 'downloading' | 'waiting_for_runtime' | 'reading' | 'transcribing' | 'done' | 'stopped';
  chunks_done: number;
  chunks_total: number;
  notes: number;
  started_ms: number;
  /** How long the run has been going by the service's clock. */
  elapsed_ms?: number;
  done: boolean;
  error: string | null;
  file: string | null;
  progress: number;
  title: string;
  /** The set this track belongs to, when several were asked for at once. */
  batch: { done: number; total: number; queued: string[]; failed: string[] } | null;
}


interface MidiStatus {
  runtime_build?: string | null;
  tool_installed: boolean;
  sizes: MidiSize[];
  default_size: MidiSize['id'];
  download: { downloaded_bytes: number; total_bytes: number; done: boolean; error: string | null; manual_archive?: RuntimeArchive | null } | null;
  run: MidiRun | null;
  license: string;
}

interface SongMidi {
  file: string;
  size: string | null;
  instruments: string[];
  notes: HeardNote[];
}

export const MidiTool: React.FC<{ songId: string; songTitle: string; recorded?: boolean }> = ({ songId, songTitle, recorded }) => {
  const { t, language } = useI18n();
  const page = toolsPageStrings[language];
  const [confirmDelete, setConfirmDelete] = useState(false);
  const tt = t as unknown as (key: string) => string;
  const [status, setStatus] = useState<MidiStatus | null>(null);
  const [size, setSize] = useState<MidiSize['id'] | null>(null);
  const [songMidi, setSongMidi] = useState<SongMidi | null>(null);
  const [liveNotes, setLiveNotes] = useState<HeardNote[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const cardRef = useRef<HTMLDivElement | null>(null);

  const run = status?.run?.song_id === songId ? status.run : null;
  const running = Boolean(run && !run.done);
  const batch = status?.run?.batch ?? null;
  const batchActive = Boolean(batch && (batch.queued.length > 0 || !status?.run?.done));
  // the track's MIDI was made with one of the models: that one stays chosen until the person picks another
  const made = status?.sizes.find(entry => entry.id === songMidi?.size)?.id;
  const chosen = size ?? made ?? status?.default_size ?? 'medium';
  const chosenSize = status?.sizes.find(entry => entry.id === chosen);

  const loadSongMidi = useCallback(async () => {
    if (!songId) return setSongMidi(null);
    const response = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/midi`);
    setSongMidi(response.ok ? await response.json() : null);
  }, [songId]);

  useEffect(() => {
    void loadSongMidi().catch(() => setSongMidi(null));
  }, [loadSongMidi]);

  // the MIDI editor saved this track's MIDI
  useEffect(() => {
    const changed = (event: Event) => {
      if ((event as CustomEvent<{ songId: string }>).detail?.songId === songId) void loadSongMidi().catch(() => setSongMidi(null));
    };
    window.addEventListener('yue:midi-changed', changed);
    return () => window.removeEventListener('yue:midi-changed', changed);
  }, [songId, loadSongMidi]);

  // the state, and while a run of this track is at work, the notes it has heard
  useEffect(() => {
    let alive = true;
    let wasRunning = false;
    const read = async () => {
      const next: MidiStatus = await fetch('/v1/midi', { headers: SELF_READ }).then(response => response.json());
      if (!alive) return;
      setStatus(next);
      const mine = next.run?.song_id === songId ? next.run : null;
      if (mine && !mine.done && mine.stage === 'transcribing') {
        const body = await fetch('/v1/midi/notes', { headers: SELF_READ }).then(response => response.json());
        if (alive) setLiveNotes(body.notes ?? []);
      }
      if (wasRunning && mine?.done) void loadSongMidi();
      wasRunning = Boolean(mine && !mine.done);
    };
    void read().catch(() => undefined);
    const timer = window.setInterval(() => void read().catch(() => undefined), 1000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [songId, loadSongMidi]);

  const post = async (path: string, body: object) => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      if (!response.ok) {
        const problem = await response.json().catch(() => null);
        throw new Error(problem?.error || `HTTP ${response.status}`);
      }
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const transcribe = () => {
    setLiveNotes([]);
    void post('/v1/midi/transcribe', { song_id: songId, size: chosen });
  };

  const remove = async () => {
    const response = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/midi`, { method: 'DELETE' });
    if (response.ok) setSongMidi(null);
  };

  const download = status?.download && !status.download.done ? status.download : null;
  const percent = run?.stage === 'downloading' && download
    ? Math.round((download.downloaded_bytes / Math.max(1, download.total_bytes)) * 100)
    : run && run.chunks_total > 0
      ? Math.round(run.progress * 100)
      : null;
  const audioUrl = songId ? apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`) : undefined;
  const missing = chosenSize && (!status?.tool_installed || !chosenSize.installed) ? chosenSize.missing_bytes : 0;
  const sizeName = tt(`midiSize_${chosen}`);
  const sung = chosen === 'vocal';
  const modelName = sung ? 'ROSVOT' : `MuScriptor (${sizeName.toLowerCase()})`;
  const bytesText = (bytes: number) => formatSize(bytes, language);
  const reason = !songId ? page.chooseTrack : null;
  const failure = run?.error || error || (missing > 0 ? status?.download?.error : null);

  const action = missing > 0 && !running ? (download ? (
    <div className="flex items-end gap-2">
      <div className="min-w-0 flex-1">
        <WorkProgress label={page.downloading(modelName, bytesText(download.downloaded_bytes), bytesText(download.total_bytes))}
          percent={Math.round((download.downloaded_bytes / Math.max(1, download.total_bytes)) * 100)} />
      </div>
      <SecondaryButton disabled={busy} onClick={() => void post('/v1/midi/cancel', {})}>{t('cancelDownload')}</SecondaryButton>
    </div>
  ) : (
    <PrimaryButton disabled={busy} onClick={() => void post('/v1/midi/install', { size: chosen })}>
      <Download size={14} />{page.download(modelName, bytesText(missing))}
    </PrimaryButton>
  )) : (
    <>
      <div className="flex flex-wrap gap-2">
        <PrimaryButton onClick={transcribe} disabled={Boolean(reason) || running || busy || batchActive}>
          {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
          {songMidi ? t('midiAgain') : t('midiStart')}
        </PrimaryButton>
        {running && <SecondaryButton onClick={() => void post('/v1/midi/cancel', {})}><Square size={12} /> {t('midiStop')}</SecondaryButton>}
      </div>
      {reason && <ActionReason>{reason}</ActionReason>}
      {running && run && <WorkProgress label={tt(`midiStage_${run.stage}`)} percent={percent} elapsedMs={run.elapsed_ms} />}
    </>
  );

  return (
    <div ref={cardRef}>
      <ToolCard icon={<Piano size={17} />} title={t('midiTitle')}
        state={status && (missing > 0 ? (download ? <ToolState tone="busy">{page.downloading(modelName, bytesText(download.downloaded_bytes), bytesText(download.total_bytes))}</ToolState>
          : <ToolState tone="missing">{page.needsDownload(bytesText(missing))}</ToolState>) : <ToolState tone="ready">{page.ready}</ToolState>)}
        description={sung ? tt('midiHint_vocal') : t('midiHint')}
        action={action}
        result={<>
          {batch && (
            <div className="rounded-lg border border-zinc-200 px-3 py-2 text-xs text-zinc-600 dark:border-white/10 dark:text-zinc-300">
              <div className="flex min-w-0 items-center gap-2">
                {batchActive && <Loader2 size={13} className="shrink-0 animate-spin text-pink-500" />}
                <span className="shrink-0 font-semibold tabular-nums">{t('midiBatchProgress').replace('{done}', String(batch.done)).replace('{total}', String(batch.total))}</span>
                {batchActive && status?.run?.title && <span className="min-w-0 truncate text-zinc-500 dark:text-zinc-400">· {status.run.title}</span>}
                {batchActive && <SecondaryButton className="ml-auto" onClick={() => void post('/v1/midi/cancel', {})}><Square size={10} /> {t('midiStop')}</SecondaryButton>}
              </div>
              {batch.queued.includes(songId) && <p className="mt-1 text-zinc-500 dark:text-zinc-400">{t('midiBatchQueuedHere')}</p>}
              {batch.failed.length > 0 && (
                <ul className="mt-1 space-y-0.5 text-rose-600 wrap-anywhere dark:text-rose-300">
                  {batch.failed.map(line => <li key={line}>{line}</li>)}
                </ul>
              )}
            </div>
          )}
          {running && run?.stage === 'transcribing' && (
            <MidiPlayer key={`live-${songId}`} notes={liveNotes} sourceAudioUrl={audioUrl} live chunks={{ done: run.chunks_done, total: run.chunks_total }} />
          )}
          {!running && songMidi && (
            <div className="grid gap-2">
              <p className="text-xs text-zinc-500 dark:text-zinc-400">
                {songMidi.size ? `${tt(`midiSize_${songMidi.size}`)} · ` : ''}{songMidi.instruments.length} {t('midiInstruments')}
              </p>
              <MidiPlayer key={`done-${songId}-${songMidi.notes.length}`} notes={songMidi.notes} sourceAudioUrl={audioUrl} live={false} />
              <div className="flex flex-wrap gap-2">
                <SecondaryButton onClick={() => setEditing(true)}><PencilLine size={13} /> {tt('midiEdEdit')}</SecondaryButton>
                <SecondaryButton onClick={() => void saveFile(`${songTitle || 'track'}.mid`, { url: apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/midi/file`) })}>
                  <Download size={13} /> {t('midiSave')}
                </SecondaryButton>
                <SecondaryButton onClick={() => setConfirmDelete(true)}><Trash2 size={13} /> {page.midiDelete}</SecondaryButton>
              </div>
            </div>
          )}
          {!running && run?.stage === 'stopped' && !error && <p className="text-xs text-zinc-500 dark:text-zinc-400">{t('midiStopped')}</p>}
        </>}
        error={failure && <>
          <p role="alert" className="select-text rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 wrap-anywhere dark:text-rose-300">{failure}</p>
          {missing > 0 && status?.download?.error && <ManualRuntimeArchive archive={status.download.manual_archive} retryLabel={page.download(modelName, bytesText(missing))} />}
        </>}
        footer={sung ? <>{page.model}: ROSVOT (Ruiqi Li), RMVPE · {page.license} MIT, Apache 2.0</>
          : <>{page.model}: MuScriptor (Kyutai, Mirelo) · {page.license} CC BY-NC 4.0 — <span className="text-zinc-700 dark:text-zinc-200">{page.nonCommercial}</span></>}>
        <ToolModelPicker<MidiSize['id']> tool="midi" value={chosen} onChange={setSize} disabled={running}
          models={(status?.sizes ?? []).map(entry => ({ id: entry.id, name: tt(`midiSize_${entry.id}`), hint: tt(entry.id === 'vocal' ? 'midiHint_vocal' : 'midiHint_all'), bytes: entry.bytes, installed: entry.installed }))}
          note={recorded && chosen !== 'vocal' && status?.sizes.some(entry => entry.id === 'vocal') && (
          <p className="flex flex-wrap items-center gap-2 text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">
            {tt('midiPickVocal')}
            <SecondaryButton disabled={running} onClick={() => setSize('vocal')}>{tt('midiPickVocalAction')}</SecondaryButton>
          </p>
        )} />
      </ToolCard>
      <ConfirmDialog isOpen={confirmDelete} title={page.midiDeleteTitle} message={page.midiDeleteMessage} confirmLabel={page.midiDelete}
        onConfirm={() => { setConfirmDelete(false); void remove(); }} onCancel={() => setConfirmDelete(false)} />
      {editing && <MidiEditor source={{ kind: 'track', songId, title: songTitle }} onClose={() => setEditing(false)} />}
    </div>
  );
};
