import React, { useEffect, useRef, useState } from 'react';
import { Download, FileAudio, Loader2, Play, Sparkles, Square } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { saveFile } from '../services/saveFile';
import { SELF_READ } from '../services/studioQueries';
import { audioFormatFor, base64For } from '../services/nativeOpenRouter';
import { ConfirmDialog } from './ConfirmDialog';
import { ManualRuntimeArchive, type RuntimeArchive } from './ManualDownloads';
import { StemPlayer } from './StemPlayer';
import { WorkProgress } from './WorkProgress';
import { PrimaryButton, SecondaryButton } from './kit/Buttons';
import { CatalogSegments } from './kit/Catalog';
import { ActionReason, ToolCard, ToolState } from './kit/ToolCard';
import { ToolDevice } from './ToolDevice';
import { toolsPageStrings } from '../i18n/toolsPage';
import { formatSize } from '../services/fileSize';

/**
 * Restore after compression on the tools page: Apollo on the track chosen there or on a
 * file from this computer. The result is a new library track beside the source.
 */

export interface RestoreRun {
  song_id: string | null;
  title: string;
  stage: 'restoring' | 'done' | 'stopped';
  started_ms: number;
  done: boolean;
  error: string | null;
  used_gpu: boolean | null;
  library_song: string | null;
}

export interface RestoreStatus {
  model: { id: string; label: string; bytes: number; note: string; installed: boolean };
  runtime_installed: boolean;
  ready: boolean;
  missing_bytes: number;
  error: string | null;
  download: { downloaded_bytes: number; total_bytes: number; done: boolean; error: string | null; manual_archive?: RuntimeArchive | null } | null;
  run: RestoreRun | null;
}

export const RestoreTool: React.FC<{
  songId: string;
  songTitle: string;
  /** The track an earlier restoration of the chosen one made, which restoring again replaces. */
  restoredId: string | null;
  /** The chosen track is itself a restoration. */
  chosenIsRestored: boolean;
  /** A run ended: the library has a new track. */
  onDone: () => void;
  /** The page shows the track picker only while the chosen track is the source. */
  onSourceChange?: (from: 'song' | 'file') => void;
}> = ({ songId, songTitle, restoredId, chosenIsRestored, onDone, onSourceChange }) => {
  const { t, language } = useI18n();
  const page = toolsPageStrings[language];
  const size = (bytes: number) => formatSize(bytes, language);
  const [status, setStatus] = useState<RestoreStatus | null>(null);
  const [from, setFrom] = useState<'song' | 'file'>('song');
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmReplace, setConfirmReplace] = useState(false);
  const picker = useRef<HTMLInputElement | null>(null);
  const finished = useRef(onDone);
  finished.current = onDone;

  useEffect(() => {
    let alive = true;
    let wasRunning = false;
    const read = async () => {
      const next: RestoreStatus = await fetch('/v1/restore', { headers: SELF_READ }).then(response => response.json());
      if (!alive) return;
      setStatus(next);
      const running = Boolean(next.run && !next.run.done);
      if (wasRunning && !running) finished.current();
      wasRunning = running;
    };
    void read().catch(() => undefined);
    const timer = window.setInterval(() => void read().catch(() => undefined), 1000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

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

  const start = async () => {
    if (from === 'song') return post('/v1/restore/start', { song_id: songId });
    if (!file) return;
    setBusy(true);
    let encoded: string;
    try {
      encoded = await base64For(file);
    } catch (problem) {
      setBusy(false);
      setError(problem instanceof Error ? problem.message : String(problem));
      return;
    }
    await post('/v1/restore/start', { audio_base64: encoded, audio_format: audioFormatFor(file), audio_name: file.name });
  };

  const run = status?.run ?? null;
  const running = Boolean(run && !run.done);
  const download = status?.download && !status.download.done ? status.download : null;
  const replaces = from === 'song' && Boolean(restoredId);
  const reason = from === 'song' ? (!songId ? page.chooseTrack : chosenIsRestored ? t('restoreOfRestored') : null) : !file ? page.chooseFile : null;
  const canStart = Boolean(status?.ready) && !running && !busy && !reason;
  const resultUrl = run?.library_song ? apiUrl(`/v1/library/media/${encodeURIComponent(run.library_song)}`) : null;
  const failure = run?.error || error || status?.error || status?.download?.error;

  return (
    <ToolCard icon={<Sparkles size={17} />} title={t('restoreTitle')}
      state={status && (status.ready ? <ToolState tone="ready">{page.ready}</ToolState>
        : download ? <ToolState tone="busy">{page.downloading('Apollo', size(download.downloaded_bytes), size(download.total_bytes))}</ToolState>
          : <ToolState tone="missing">{page.needsDownload(size(status.missing_bytes))}</ToolState>)}
      description={t('restoreHint')}
      action={status && !status.ready ? (download ? (
        <div className="flex items-end gap-2">
          <div className="min-w-0 flex-1">
            <WorkProgress label={page.downloading('Apollo', size(download.downloaded_bytes), size(download.total_bytes))}
              percent={Math.round((download.downloaded_bytes / Math.max(1, download.total_bytes)) * 100)} />
          </div>
          <SecondaryButton onClick={() => void post('/v1/restore/cancel', {})}><Square size={11} />{t('cancelDownload')}</SecondaryButton>
        </div>
      ) : (
        <PrimaryButton onClick={() => void post('/v1/restore/install', {})} disabled={busy}>
          <Download size={14} />{page.download('Apollo', size(status.missing_bytes))}
        </PrimaryButton>
      )) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <PrimaryButton onClick={() => (replaces ? setConfirmReplace(true) : void start())} disabled={!canStart}>
              {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
              {replaces ? t('restoreAgain') : t('restoreStart')}
            </PrimaryButton>
            {running && <SecondaryButton onClick={() => void post('/v1/restore/cancel', {})}><Square size={12} /> {t('restoreStop')}</SecondaryButton>}
          </div>
          {reason && !running && <ActionReason>{reason}</ActionReason>}
          {running && run && <WorkProgress label={t('restoreRunning').replace('{title}', run.title)} percent={null} elapsedMs={Math.max(0, Date.now() - run.started_ms)} />}
          <ConfirmDialog
            isOpen={confirmReplace}
            title={t('restoreReplaceTitle')}
            message={t('restoreReplaceMessage')}
            confirmLabel={t('restoreReplaceConfirm')}
            onConfirm={() => { setConfirmReplace(false); void start(); }}
            onCancel={() => setConfirmReplace(false)}
          />
        </>
      )}
      result={<>
        {!running && run?.stage === 'done' && resultUrl && (
          <div className="grid gap-2">
            <StemPlayer src={resultUrl} label={`${run.title} · ${page.restored}`} />
            <SecondaryButton className="justify-self-start" onClick={() => void saveFile(`${run.title} - restored.wav`, { url: resultUrl })}>
              <Download size={13} />{page.saveWav}
            </SecondaryButton>
          </div>
        )}
        {!running && run?.stage === 'stopped' && !error && <p className="text-xs text-zinc-500 dark:text-zinc-400">{t('restoreStopped')}</p>}
      </>}
      error={failure && <>
        <p role="alert" className="select-text rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 wrap-anywhere dark:text-rose-300">{failure}</p>
        {status?.download?.error && <ManualRuntimeArchive archive={status.download.manual_archive} retryLabel={page.download('Apollo', size(status.missing_bytes))} />}
      </>}
      footer={<>{page.model}: Apollo (JusperLee) · {page.license} CC BY-SA 4.0</>}>
      <CatalogSegments<'song' | 'file'> fill label={t('restoreFrom')} value={from} onChange={next => { setFrom(next); onSourceChange?.(next); }}
        options={[{ value: 'song', label: page.fromTrack }, { value: 'file', label: page.fromFile }]} />
      {from === 'file' && (
        <div>
          <SecondaryButton aria-label={`${page.fromFile}: ${file?.name ?? t('chooseAudioFile')}`} onClick={() => picker.current?.click()}>
            <FileAudio size={13} aria-hidden />{file?.name ?? t('chooseAudioFile')}
          </SecondaryButton>
          <input ref={picker} type="file" accept="audio/*" className="hidden"
            onChange={event => { setFile(event.target.files?.[0] ?? null); event.target.value = ''; }} />
        </div>
      )}
      <ToolDevice tool="restore" disabled={running} />
    </ToolCard>
  );
};
