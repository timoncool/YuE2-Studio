import React, { useCallback, useEffect, useState } from 'react';
import { ExternalLink, FolderOpen, Loader2, RotateCw, Square, Trash2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { poll } from '../services/poll';

export type CommunityModel = {
  id: string;
  name: Record<string, string>;
  author: string;
  page: string;
  license: string;
  files: { quant: string; file: string; bytes: number; installed: boolean }[];
};

export type OwnModelStatus = {
  folder: string;
  files: { name: string; bytes: number }[];
  selected: string | null;
  missing: boolean;
  community?: CommunityModel[];
  card_quant?: string | null;
  download?: { asset_id: string; downloaded_bytes: number; total_bytes: number; done: boolean; error: string | null } | null;
};

const gigabytes = (bytes: number) => `${(bytes / 1024 ** 3).toFixed(2)} GB`;
/** A choice of the list: an own file by name, or a community model's quantisation. */
const COMMUNITY = 'community:';

async function answer(response: Response): Promise<OwnModelStatus> {
  if (!response.ok) {
    const body = await response.json().catch(() => null) as { error?: string } | null;
    throw new Error(body?.error || `${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<OwnModelStatus>;
}

/** The value of the list for a selected file: the community entry it came from, when it is one. */
export function choiceOf(status: OwnModelStatus | null): string {
  const selected = status?.selected;
  if (!selected) return '';
  for (const model of status?.community ?? []) {
    const file = model.files.find((entry) => entry.file === selected);
    if (file) return `${COMMUNITY}${model.id}:${file.quant}`;
  }
  return selected;
}

/** Settings - Models: a YuE2 fine-tune of the person's own, or of the community, run in place of the set's backbone. */
export const OwnModelPanel: React.FC = () => {
  const { t, language } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [status, setStatus] = useState<OwnModelStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setStatus(await answer(await fetch('/setup/own-model')));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, []);

  useEffect(() => { void load(); }, [load]);
  const downloading = Boolean(status?.download && !status.download.done);
  useEffect(() => {
    if (!downloading) return;
    return poll(load, 1000);
  }, [downloading, load]);

  const send = async (url: string, method: string, body?: unknown) => {
    setBusy(true);
    try {
      setStatus(await answer(await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
      })));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const choose = (value: string) => {
    if (value.startsWith(COMMUNITY)) {
      const [id, quant] = value.slice(COMMUNITY.length).split(':');
      return send('/setup/own-model', 'PUT', { community: id, quant });
    }
    return send('/setup/own-model', 'PUT', { file: value });
  };

  const cancel = async () => {
    try {
      await fetch('/setup/own-model/cancel', { method: 'POST' });
      await load();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
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

  const communityFiles = new Set((status?.community ?? []).flatMap((model) => model.files.map((file) => file.file)));
  const ownFiles = (status?.files ?? []).filter((file) => !communityFiles.has(file.name));
  const percent = status?.download && status.download.total_bytes > 0 ? Math.floor((status.download.downloaded_bytes / status.download.total_bytes) * 100) : 0;
  const removable = status?.files.find((file) => file.name === status.selected);

  return (
    <div className="mt-4 rounded-lg border border-zinc-200 p-3 dark:border-white/10">
      <p className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{tt('ownModelTitle')}</p>
      <p className="mt-1 text-[11px] leading-5 text-zinc-500 dark:text-zinc-400">{tt('ownModelHint')}</p>
      <select
        value={choiceOf(status)}
        onChange={(event) => void choose(event.target.value)}
        disabled={busy || downloading || !status}
        aria-label={tt('ownModelTitle')}
        className="mt-2 w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 focus:border-pink-500 focus:outline-hidden disabled:opacity-50 dark:border-white/10 dark:bg-black/20 dark:text-white"
      >
        <option value="">{tt('ownModelNone')}</option>
        {status?.missing && status.selected && !communityFiles.has(status.selected) && <option value={status.selected}>{status.selected}</option>}
        {ownFiles.length > 0 && (
          <optgroup label={tt('ownModelFilesGroup')}>
            {ownFiles.map((file) => (
              <option key={file.name} value={file.name}>{file.name} — {gigabytes(file.bytes)}</option>
            ))}
          </optgroup>
        )}
        {(status?.community?.length ?? 0) > 0 && (
          <optgroup label={tt('ownModelCommunityGroup')}>
            {status!.community!.flatMap((model) => model.files.map((file) => (
              <option key={file.file} value={`${COMMUNITY}${model.id}:${file.quant}`}>
                {model.name[language] ?? model.name.en} · {file.quant} — {gigabytes(file.bytes)}{file.installed ? '' : ` · ${tt('ownModelToDownload')}`}
              </option>
            )))}
          </optgroup>
        )}
      </select>
      {downloading && status?.download && (
        <div className="mt-2">
          <div className="flex items-center justify-between text-[11px] text-zinc-600 dark:text-zinc-300">
            <span className="flex items-center gap-1"><Loader2 size={12} className="animate-spin text-pink-500" />{tt('ownModelDownloading').replace('{file}', status.download.asset_id)}</span>
            <span className="tabular-nums">{percent}%</span>
          </div>
          <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
            <div className="h-full bg-linear-to-r from-orange-500 to-pink-500 transition-[width]" style={{ width: `${percent}%` }} />
          </div>
          <button type="button" onClick={() => void cancel()} className="mt-2 inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:border-rose-400 hover:text-rose-600 dark:border-white/15 dark:text-zinc-300">
            <Square size={12} />{t('cancelDownload')}
          </button>
        </div>
      )}
      {status?.download?.error && !downloading && <p role="alert" className="mt-1 select-text text-[11px] leading-4 text-rose-600 dark:text-rose-300">{status.download.error}</p>}
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
        {removable && (
          <button
            type="button"
            onClick={() => void send('/setup/own-model/remove', 'POST', { file: removable.name })}
            disabled={busy || downloading}
            className="inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:border-rose-400 hover:text-rose-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-300"
          >
            <Trash2 size={13} />
            {t('remove')}
          </button>
        )}
        {(status?.community?.length ?? 0) > 0 && (
          <a
            href={status!.community![0].page}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
          >
            <ExternalLink size={13} />
            {tt('ownModelCommunityCard')}
          </a>
        )}
      </div>
      {error && <p role="alert" className="mt-2 select-text text-[11px] leading-4 text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
};
