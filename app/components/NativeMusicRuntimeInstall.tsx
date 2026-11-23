import React, { useEffect, useRef, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { signedUpdateStrings } from '../i18n/signedUpdates';
import { apiUrl } from '../services/apiBase';
import { Download, RefreshCw } from 'lucide-react';
import { SecondaryButton } from './kit/Buttons';
import { WorkProgress } from './WorkProgress';

type Engine = 'acestep-cpp' | 'minimaxmusic-cpp';
type Status = {
  supported: boolean;
  installed?: boolean;
  bytes?: number;
  download?: { done: boolean; error?: string; downloaded_bytes: number; total_bytes: number };
};

export function NativeMusicRuntimeInstall({ engine, update, onInstalled }: {
  engine: Engine;
  update?: boolean;
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
  const size = status.bytes ? new Intl.NumberFormat(language, { style: 'unit', unit: 'gigabyte', maximumFractionDigits: 1 }).format(status.bytes / 2 ** 30) : '';
  const action = update ? signedUpdateStrings[language].update : t('download');
  return <div className="space-y-2 text-xs">
    {busy ? <>
      <WorkProgress label={t('downloading')} percent={download?.total_bytes ? Math.min(100, percent) : null} />
      {download && !download.done && <SecondaryButton onClick={() => {
        void fetch(url, { method: 'DELETE' }).catch(problem => setError(String(problem)));
      }}>{t('cancelDownload')}</SecondaryButton>}
    </> : <SecondaryButton onClick={() => void install()}>
      {update ? <RefreshCw size={13} aria-hidden /> : <Download size={13} aria-hidden />}{size ? `${action} · ${size}` : action}
    </SecondaryButton>}
    {!busy && (error || download?.error) && <p role="alert" className="text-rose-600 dark:text-rose-300">{error || download?.error}</p>}
  </div>;
}
