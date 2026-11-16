import React, { useCallback, useEffect, useState } from 'react';
import { FolderCog, Loader2, RotateCcw, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import { apiUrl } from '../services/apiBase';

interface FolderUse { name: string; bytes: number; elsewhere: boolean }
interface Storage {
  directory: string;
  chosen: string | null;
  default: string;
  free_bytes: number;
  total_bytes: number;
  used_bytes: number;
  folders: FolderUse[];
}
interface Moved { name: string; bytes: number; error: string | null }
interface Move {
  running: boolean;
  folder: string | null;
  done_bytes: number;
  total_bytes: number;
  moved: Moved[];
  engine_error: string | null;
}
interface Picked { path: string; directory: string; free_bytes: number; copy_bytes: number }

const OUTLINE =
  'inline-flex items-center gap-1.5 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/15 dark:text-zinc-200';
const PRIMARY =
  'inline-flex items-center gap-1.5 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-3 py-1.5 text-xs font-bold text-white disabled:cursor-not-allowed disabled:opacity-50';

const size = (bytes: number) => (bytes < 1024 ** 3 ? `${Math.round(bytes / 1024 ** 2)} MB` : `${(bytes / 1024 ** 3).toFixed(1)} GB`);

async function answer(response: Response): Promise<Record<string, unknown>> {
  const body = (await response.json().catch(() => null)) as Record<string, unknown> | null;
  if (!response.ok) throw new Error(typeof body?.error === 'string' ? body.error : `HTTP ${response.status}`);
  return body ?? {};
}

/**
 * Where the studio's downloads are kept and how much room that disk has, with
 * the move to another disk: the folders go there and a link stays where they
 * were, so nothing else in the studio changes.
 */
export const ModelStorageCard: React.FC = () => {
  const { t } = useI18n();
  const [storage, setStorage] = useState<Storage | null>(null);
  const [move, setMove] = useState<Move | null>(null);
  const [picked, setPicked] = useState<Picked | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);

  const load = useCallback(async () => {
    const body = await answer(await fetch(apiUrl('/v1/storage')));
    setStorage(body.storage as Storage);
    setMove(body.move as Move);
  }, []);

  useEffect(() => {
    void load().catch(() => undefined);
  }, [load]);

  const running = move?.running === true;
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => void load().catch(() => undefined), 700);
    return () => window.clearInterval(timer);
  }, [running, load]);

  const guarded = async (work: () => Promise<void>) => {
    setError(null);
    try {
      await work();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const choose = () => guarded(async () => {
    setAsking(true);
    try {
      const body = await answer(await fetch(apiUrl('/v1/storage/pick'), { method: 'POST' }));
      if (body.picked === true) setPicked(body as unknown as Picked);
    } finally {
      setAsking(false);
    }
  });

  const start = (directory: string | null) => guarded(async () => {
    const body = await answer(await fetch(apiUrl('/v1/storage'), { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ directory }) }));
    setPicked(null);
    setMove(body.move as Move);
    if ((body.move as Move).running !== true) await load();
  });

  const stop = () => guarded(async () => {
    await answer(await fetch(apiUrl('/v1/storage/cancel'), { method: 'POST' }));
  });

  if (!storage) return null;

  const folderName = (name: string) => t(`storageFolder_${name.replace('/', '_')}` as TranslationKey);
  const tight = picked !== null && picked.free_bytes > 0 && picked.copy_bytes > picked.free_bytes;
  const went = move?.moved.filter(folder => !folder.error) ?? [];
  const stayed = move?.moved.filter(folder => folder.error) ?? [];

  return (
    <section className="mt-2 rounded-lg border border-zinc-200 p-3 text-xs dark:border-white/10" data-testid="model-storage">
      <h3 className="flex items-center gap-1.5 text-sm font-semibold text-zinc-900 dark:text-white"><FolderCog size={15} className="text-pink-500" />{t('storageTitle')}</h3>
      <p className="mt-1 break-all font-mono text-[11px] text-zinc-700 dark:text-zinc-200">{storage.directory}</p>
      <p className="mt-0.5 text-zinc-500">
        {t('storageSpace').replace('{free}', size(storage.free_bytes)).replace('{total}', size(storage.total_bytes)).replace('{used}', size(storage.used_bytes))}
      </p>
      <p className="mt-1.5 leading-5 text-zinc-500">{t('storageHint')}</p>

      {running && move && (
        <div className="mt-2">
          <div className="flex items-center justify-between gap-2 text-zinc-700 dark:text-zinc-200">
            <span className="flex min-w-0 items-center gap-1.5">
              <Loader2 size={13} className="shrink-0 animate-spin" />
              <span className="truncate">
                {t('storageMoving').replace('{folder}', move.folder ? folderName(move.folder) : '…').replace('{done}', size(move.done_bytes)).replace('{total}', size(move.total_bytes))}
              </span>
            </span>
            <button type="button" onClick={() => void stop()} className="shrink-0 font-semibold text-rose-600 hover:text-rose-500">{t('cancel')}</button>
          </div>
          <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
            <div className="h-full bg-pink-500 transition-all" style={{ width: `${move.total_bytes > 0 ? Math.round((move.done_bytes / move.total_bytes) * 100) : 0}%` }} />
          </div>
          <p className="mt-1 text-[11px] text-zinc-500">{t('storageMovingNote')}</p>
        </div>
      )}

      {!running && picked && (
        <div className="mt-2 rounded-lg border border-pink-500/30 bg-pink-500/5 p-2">
          <p className="break-all leading-5 text-zinc-700 dark:text-zinc-200">
            {picked.copy_bytes > 0
              ? t('storagePicked').replace('{folder}', picked.directory).replace('{copy}', size(picked.copy_bytes)).replace('{free}', size(picked.free_bytes))
              : t('storagePickedSame').replace('{folder}', picked.directory)}
          </p>
          {tight && <p className="mt-1 text-rose-600 dark:text-rose-300">{t('storageTight')}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <button type="button" onClick={() => setPicked(null)} className={OUTLINE}><X size={13} />{t('cancel')}</button>
            <button type="button" disabled={tight} onClick={() => void start(picked.path)} className={PRIMARY}>{t('storageMove')}</button>
          </div>
        </div>
      )}

      {!running && !picked && (
        <div className="mt-2 flex flex-wrap gap-2">
          <button type="button" disabled={asking} onClick={() => void choose()} className={OUTLINE}>
            {asking ? <Loader2 size={13} className="animate-spin" /> : <FolderCog size={13} />}{t('storageChange')}
          </button>
          {storage.chosen && (
            <button type="button" onClick={() => void start(null)} className={OUTLINE}><RotateCcw size={13} />{t('storageBack')}</button>
          )}
        </div>
      )}

      {!running && went.length > 0 && <p className="mt-2 text-emerald-700 dark:text-emerald-300">{t('storageDone').replace('{folders}', went.map(folder => folderName(folder.name)).join(', '))}</p>}
      {!running && stayed.map(folder => (
        <p key={folder.name} className="mt-1 break-words text-amber-700 dark:text-amber-300">{t('storageStayed').replace('{folder}', folderName(folder.name)).replace('{error}', folder.error ?? '')}</p>
      ))}
      {!running && move?.engine_error && <p className="mt-1 break-words text-rose-600 dark:text-rose-300">{t('storageEngine').replace('{error}', move.engine_error)}</p>}
      {error && <p className="mt-2 break-words text-rose-600 dark:text-rose-300">{error}</p>}
    </section>
  );
};
