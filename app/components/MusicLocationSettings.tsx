import React, { useEffect, useState } from 'react';
import { FolderOpen, Loader2, RotateCcw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';

interface Location {
  directory: string;
  default_directory: string;
  is_default: boolean;
  pending_directory: string | null;
  restart_required: boolean;
  error: string | null;
}

async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(apiUrl(url), options);
  const data = await response.json().catch(() => null);
  if (!response.ok) throw new Error(data?.error || `Music location: HTTP ${response.status}`);
  return data as T;
}

/** Song audio, covers and the SQLite catalogue (including album membership).
 * This is separate from model storage. A change is applied only on restart.
 */
export const MusicLocationSettings: React.FC = () => {
  const { t } = useI18n();
  const [location, setLocation] = useState<Location | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    request<Location>('/v1/library/location')
      .then(info => { if (!cancelled) setLocation(info); })
      .catch(reason => { if (!cancelled) setError(String(reason)); });
    return () => { cancelled = true; };
  }, []);

  const run = async (action: () => Promise<void>) => {
    setWorking(true);
    setError(null);
    try { await action(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setWorking(false); }
  };
  const choose = () => run(async () => {
    const response = await request<{ picked: boolean; directory?: string }>('/v1/library/location/pick', { method: 'POST' });
    if (response.picked && response.directory) setPicked(response.directory);
  });
  const change = (directory: string | null) => run(async () => {
    const info = await request<Location>('/v1/library/location', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ directory }),
    });
    setLocation(info);
    setPicked(null);
  });

  return (
    <div className="max-w-xl space-y-4 text-sm">
      <p className="text-zinc-500 dark:text-zinc-400">{t('musicLocationHint')}</p>
      {!location && !error && <Loader2 size={17} className="animate-spin" aria-label={t('loading')} />}
      {location && (
        <>
          <div className="space-y-1 rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-white/10 dark:bg-black/20">
            <p className="font-semibold">{t('musicLocationCurrent')}</p>
            <p className="break-all font-mono text-xs" data-testid="music-location-current">{location.directory}</p>
            {location.is_default && <p className="text-xs text-zinc-500">{t('musicLocationUsingDefault')}</p>}
          </div>
          {location.restart_required && (
            <p role="status" className="rounded-lg border border-amber-500/30 p-3 text-amber-600 dark:text-amber-300">
              {t('musicLocationRestart')} <span className="block break-all font-mono text-xs">{location.pending_directory}</span>
            </p>
          )}
          {location.error && <p role="alert" className="text-rose-500">{t('musicLocationMoveFailed')} {location.error}</p>}
          <div className="flex flex-wrap gap-2">
            <button type="button" disabled={working} onClick={() => void choose()}
              className="inline-flex items-center gap-2 rounded-lg bg-zinc-800 px-3 py-2 text-white hover:bg-zinc-700 disabled:opacity-50">
              {working ? <Loader2 size={15} className="animate-spin" /> : <FolderOpen size={15} />}
              {t('musicLocationChoose')}
            </button>
            {(!location.is_default || location.restart_required) && (
              <button type="button" disabled={working} onClick={() => void change(null)}
                className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 hover:bg-zinc-100 disabled:opacity-50 dark:border-white/20 dark:hover:bg-white/10">
                <RotateCcw size={15} />{t('musicLocationReset')}
              </button>
            )}
          </div>
          {picked && (
            <div className="space-y-3 rounded-lg border border-indigo-500/30 p-3">
              <p className="text-xs font-semibold">{t('musicLocationChosen')}</p>
              <p className="break-all font-mono text-xs">{picked}</p>
              <p className="text-xs text-zinc-500">{t('musicLocationMoveNote')}</p>
              <div className="flex gap-2">
                <button type="button" disabled={working} onClick={() => void change(picked)}
                  className="rounded-lg bg-indigo-600 px-3 py-2 text-xs font-bold text-white hover:bg-indigo-500 disabled:opacity-50">
                  {t('musicLocationConfirm')}
                </button>
                <button type="button" onClick={() => setPicked(null)}
                  className="rounded-lg border border-white/20 px-3 py-2 text-xs">{t('cancel')}</button>
              </div>
            </div>
          )}
        </>
      )}
      {error && <p role="alert" className="text-xs text-rose-500">{error}</p>}
    </div>
  );
};
