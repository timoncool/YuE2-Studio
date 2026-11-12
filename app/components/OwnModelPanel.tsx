import React, { useCallback, useEffect, useState } from 'react';
import { FolderOpen, Loader2, RotateCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

export type OwnModelStatus = {
  folder: string;
  files: { name: string; bytes: number }[];
  selected: string | null;
  missing: boolean;
};

const gigabytes = (bytes: number) => `${(bytes / 1024 ** 3).toFixed(2)} GB`;

async function answer(response: Response): Promise<OwnModelStatus> {
  if (!response.ok) {
    const body = await response.json().catch(() => null) as { error?: string } | null;
    throw new Error(body?.error || `${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<OwnModelStatus>;
}

/** Settings - Models: a YuE2 fine-tune of the person's own, run in place of the set's backbone. */
export const OwnModelPanel: React.FC = () => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [status, setStatus] = useState<OwnModelStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    try {
      setStatus(await answer(await fetch('/setup/own-model')));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const choose = async (file: string) => {
    setBusy(true);
    try {
      setStatus(await answer(await fetch('/setup/own-model', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ file }),
      })));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const reveal = async () => {
    try {
      const response = await fetch('/setup/own-model/reveal', { method: 'POST' });
      if (!response.ok) setError((await response.json().catch(() => null) as { error?: string } | null)?.error || `${response.status} ${response.statusText}`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  return (
    <div className="mt-4 rounded-lg border border-zinc-200 p-3 dark:border-white/10">
      <p className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{tt('ownModelTitle')}</p>
      <p className="mt-1 text-[11px] leading-5 text-zinc-500 dark:text-zinc-400">{tt('ownModelHint')}</p>
      <select
        value={status?.selected ?? ''}
        onChange={(event) => void choose(event.target.value)}
        disabled={busy || !status}
        aria-label={tt('ownModelTitle')}
        className="mt-2 w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 focus:border-pink-500 focus:outline-hidden disabled:opacity-50 dark:border-white/10 dark:bg-black/20 dark:text-white"
      >
        <option value="">{tt('ownModelNone')}</option>
        {status?.missing && status.selected && <option value={status.selected}>{status.selected}</option>}
        {status?.files.map((file) => (
          <option key={file.name} value={file.name}>{file.name} — {gigabytes(file.bytes)}</option>
        ))}
      </select>
      {status && status.files.length === 0 && <p className="mt-1 text-[11px] leading-4 text-zinc-500 dark:text-zinc-400">{tt('ownModelEmpty')}</p>}
      {status?.missing && status.selected && (
        <p role="alert" className="mt-1 text-[11px] leading-4 text-amber-600 dark:text-amber-300">{tt('ownModelMissing').replace('{file}', status.selected)}</p>
      )}
      {status?.folder && <p className="mt-1 break-all font-mono text-[11px] text-zinc-600 dark:text-zinc-300">{status.folder}</p>}
      <div className="mt-2 flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => void reveal()}
          className="inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
        >
          <FolderOpen size={13} />
          {t('openFolder')}
        </button>
        <button
          type="button"
          onClick={() => void load()}
          disabled={busy}
          className="inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-300"
        >
          {busy ? <Loader2 size={13} className="animate-spin" /> : <RotateCw size={13} />}
          {tt('ownModelRefresh')}
        </button>
      </div>
      {error && <p role="alert" className="mt-2 select-text text-[11px] leading-4 text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
};
