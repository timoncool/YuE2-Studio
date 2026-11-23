import React, { useEffect, useRef, useState } from 'react';
import { Download, Loader2, Play, Sparkles, Square } from 'lucide-react';
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

const megabytes = (bytes: number) => `${Math.max(1, Math.round(bytes / 1024 / 1024))} MB`;

export const RestoreTool: React.FC<{
  songId: string;
  songTitle: string;
  /** The track an earlier restoration of the chosen one made, which restoring again replaces. */
  restoredId: string | null;
  /** The chosen track is itself a restoration. */
  chosenIsRestored: boolean;
  card: string;
  /** A run ended: the library has a new track. */
  onDone: () => void;
}> = ({ songId, songTitle, restoredId, chosenIsRestored, card, onDone }) => {
  const { t } = useI18n();
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
  const canStart = Boolean(status?.ready) && !running && !busy && (from === 'song' ? Boolean(songId) && !chosenIsRestored : Boolean(file));
  const resultUrl = run?.library_song ? apiUrl(`/v1/library/media/${encodeURIComponent(run.library_song)}`) : null;

  return (
    <section className={card}>
      <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
        <Sparkles size={17} className="text-pink-500" /> {t('restoreTitle')}
      </div>
      <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('restoreHint')}</p>

      <div className="mt-3 inline-flex">
        <CatalogSegments<'song' | 'file'> label={t('restoreFrom')} value={from} onChange={setFrom}
          options={[{ value: 'song', label: t('restoreFromSong') }, { value: 'file', label: t('restoreFromFile') }]} />
      </div>
      {from === 'song' ? (
        <p className="mt-2 truncate text-xs text-zinc-500">{songTitle || t('restoreNoSong')}</p>
      ) : (
        <div className="mt-2">
          <button
            type="button"
            onClick={() => picker.current?.click()}
            className={`w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-left text-sm dark:border-white/10 dark:bg-black/30 ${file ? 'text-zinc-900 dark:text-white' : 'text-zinc-400'}`}
          >
            {file?.name ?? t('chooseAudioFile')}
          </button>
          <input
            ref={picker}
            type="file"
            accept="audio/*"
            className="hidden"
            onChange={event => { setFile(event.target.files?.[0] ?? null); event.target.value = ''; }}
          />
        </div>
      )}
      {from === 'song' && chosenIsRestored && <p className="mt-2 text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('restoreOfRestored')}</p>}

      {status && !status.ready && (
        <div className="mt-3 rounded-lg border border-zinc-200 p-3 dark:border-white/10">
          {status.download?.error && <>
            <p role="alert" className="text-xs text-rose-600">{status.download.error}</p>
            <ManualRuntimeArchive archive={status.download.manual_archive} retryLabel={t('restoreInstall').replace('{size}', megabytes(status.missing_bytes))} />
          </>}
          {download ? (
            <div className="mt-2 flex items-center gap-2">
              <div className="min-w-0 flex-1">
                <WorkProgress label={`${t('download')} · ${megabytes(download.downloaded_bytes)} / ${megabytes(download.total_bytes)}`}
                  percent={Math.round((download.downloaded_bytes / Math.max(1, download.total_bytes)) * 100)} />
              </div>
              <SecondaryButton onClick={() => void post('/v1/restore/cancel', {})}><Square size={11} />{t('cancelDownload')}</SecondaryButton>
            </div>
          ) : (
            <SecondaryButton className="mt-2" onClick={() => void post('/v1/restore/install', {})} disabled={busy}>
              <Download size={13} /> {t('restoreInstall').replace('{size}', megabytes(status.missing_bytes))}
            </SecondaryButton>
          )}
        </div>
      )}

      <div className="mt-3 flex flex-wrap gap-2">
        <PrimaryButton onClick={() => (replaces ? setConfirmReplace(true) : void start())} disabled={!canStart}>
          {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
          {replaces ? t('restoreAgain') : t('restoreStart')}
        </PrimaryButton>
        {running && (
          <SecondaryButton onClick={() => void post('/v1/restore/cancel', {})}>
            <Square size={12} /> {t('restoreStop')}
          </SecondaryButton>
        )}
      </div>
      <ConfirmDialog
        isOpen={confirmReplace}
        title={t('restoreReplaceTitle')}
        message={t('restoreReplaceMessage')}
        confirmLabel={t('restoreReplaceConfirm')}
        onConfirm={() => { setConfirmReplace(false); void start(); }}
        onCancel={() => setConfirmReplace(false)}
      />

      {running && run && <WorkProgress label={t('restoreRunning').replace('{title}', run.title)} percent={null} elapsedMs={Math.max(0, Date.now() - run.started_ms)} />}

      {!running && run?.stage === 'done' && resultUrl && (
        <div className="mt-3 flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <StemPlayer src={resultUrl} label={t('restoredLabel').replace('{title}', run.title)} />
          </div>
          <button
            type="button"
            onClick={() => void saveFile(`${run.title} - restored.wav`, { url: resultUrl })}
            className="shrink-0 text-zinc-400 hover:text-pink-500"
            title={t('download')}
          >
            <Download size={15} />
          </button>
        </div>
      )}
      {!running && run?.stage === 'stopped' && !error && <p className="mt-3 text-xs text-zinc-500">{t('restoreStopped')}</p>}
      {(run?.error || error || status?.error) && (
        <p role="alert" className="mt-3 select-text rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 wrap-anywhere dark:text-rose-300">{run?.error || error || status?.error}</p>
      )}
      <p className="mt-3 text-[11px] leading-4 text-zinc-400">{t('restoreLicense')}</p>
    </section>
  );
};
