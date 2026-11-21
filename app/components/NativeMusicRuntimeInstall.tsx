import React, { useEffect, useRef, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';

type Engine = 'acestep-cpp' | 'minimaxmusic-cpp';
type Status = {
  supported: boolean;
  installed?: boolean;
  bytes?: number;
  download?: { done: boolean; error?: string; downloaded_bytes: number; total_bytes: number };
};

export function NativeMusicRuntimeInstall({ engine, onInstalled }: {
  engine: Engine;
  onInstalled: () => void;
}) {
  const { t, language } = useI18n();
  const [status, setStatus] = useState<Status>();
  const [waiting, setWaiting] = useState(false);
  const [error, setError] = useState('');
  const notifyInstalled = useRef(onInstalled);
  useEffect(() => { notifyInstalled.current = onInstalled; }, [onInstalled]);
  useEffect(() => {
    if (status?.installed) notifyInstalled.current();
  }, [status?.installed]);
  useEffect(() => {
    if (status?.download?.done && status.download.error) setWaiting(false);
  }, [status?.download?.done, status?.download?.error]);
  const url = apiUrl(`/v1/music/native/${engine}/runtime`);

  useEffect(() => {
    const abort = new AbortController();
    const poll = async () => {
      try {
        const response = await fetch(url, { signal: abort.signal });
        if (!response.ok) return;
        const data: Status = await response.json();
        if (!abort.signal.aborted) setStatus(data);
      } catch { /* The profile picker remains usable while the service restarts. */ }
    };
    void poll();
    const timer = window.setInterval(poll, 1_000);
    return () => { abort.abort(); window.clearInterval(timer); };
  }, [url]);

  async function install() {
    setWaiting(true);
    setError('');
    try {
      const response = await fetch(url, { method: 'POST' });
      const result = await response.json();
      if (!response.ok || result.started !== true) throw new Error(result.error || `HTTP ${response.status}`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
      setWaiting(false);
    }
  }

  if (!status?.supported || status.installed) return null;
  const download = status.download;
  const busy = waiting || Boolean(download && !download.done);
  const percent = download?.total_bytes ? Math.floor(100 * download.downloaded_bytes / download.total_bytes) : 0;
  const size = status.bytes && new Intl.NumberFormat(language, { maximumFractionDigits: 2 }).format(status.bytes / 2 ** 30);
  return <div className="space-y-1 text-xs">
    <button type="button" disabled={busy} onClick={() => void install()} className="text-pink-600 underline disabled:opacity-50 dark:text-pink-300">
      {busy ? `${t('downloading')} ${Math.min(100, percent)}%` : `${t('download')}${size ? ` · ${size} GiB` : ''}`}
    </button>
    {busy && download && !download.done && <button type="button" className="ml-2 underline" onClick={() => {
      void fetch(url, { method: 'DELETE' }).catch(problem => setError(String(problem)));
    }}>{t('cancelDownload')}</button>}
    {!busy && (error || download?.error) && <p role="alert">{error || download?.error}</p>}
  </div>;
}
