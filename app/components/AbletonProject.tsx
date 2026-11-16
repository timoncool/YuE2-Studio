import React, { useEffect, useRef, useState } from 'react';
import { Download, Loader2, SlidersHorizontal } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { apiUrl } from '../services/apiBase';
import { saveFile } from '../services/saveFile';

interface Made {
  id: string;
  file_name: string;
  stems: number;
  midi_parts: number;
  tempo_from_score: boolean;
  bpm: number;
  meter: string;
  tracks: string[];
  left_off: string[];
}

const forget = (id: string) => void fetch(apiUrl(`/v1/exports/live/${encodeURIComponent(id)}`), { method: 'DELETE' }).catch(() => undefined);

/**
 * The song as an Ableton Live 12 project, made on demand from what the song
 * has now: the song on a track that plays, its stems on tracks switched off,
 * its MIDI on MIDI tracks. A new zip is made each time.
 */
export const AbletonProject: React.FC<{ songId: string }> = ({ songId }) => {
  const { t } = useI18n();
  const [made, setMade] = useState<Made | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const held = useRef<string | null>(null);

  const letGo = () => {
    if (held.current) forget(held.current);
    held.current = null;
  };

  useEffect(() => {
    setMade(null);
    setError(null);
    return letGo;
  }, [songId]);

  const make = async () => {
    setBusy(true);
    setError(null);
    letGo();
    setMade(null);
    try {
      const response = await fetch(apiUrl('/v1/exports/live'), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ song_id: songId }) });
      const answer = await response.json().catch(() => null);
      if (!response.ok) throw new Error(answer?.error ?? `HTTP ${response.status}`);
      held.current = String(answer.id);
      setMade(answer as Made);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="mt-5 border-t border-zinc-200 pt-4 text-xs dark:border-white/10" data-testid="ableton-project">
      <h3 className="flex items-center gap-1.5 text-sm font-bold text-zinc-900 dark:text-white"><SlidersHorizontal size={14} />{t('exportLiveTitle')}</h3>
      <p className="mt-1.5 leading-5 text-zinc-500">{t('exportLiveHint')}</p>
      {error && <p className="mt-2 text-rose-600 dark:text-rose-300">{t('exportLiveFailed').replace('{error}', error)}</p>}
      {made && (
        <div className="mt-2 space-y-1 leading-5 text-zinc-600 dark:text-zinc-300">
          <p className="font-semibold text-zinc-800 dark:text-zinc-100">
            {t('exportLiveMade').replace('{file}', made.file_name).replace('{tracks}', String(made.tracks.length)).replace('{bpm}', String(made.bpm)).replace('{meter}', made.meter)}
          </p>
          <p>{made.tracks.join(' · ')}</p>
          {!made.tempo_from_score && <p>{t('exportLiveTempoDefault')}</p>}
          {made.stems === 0 && <p>{t('exportLiveNoStems')}</p>}
          {made.left_off.length > 0 && <p>{t('exportLiveLeftOff').replace('{files}', made.left_off.map(file => file.split('/').pop()).join(', '))}</p>}
        </div>
      )}
      <div className="mt-3 flex justify-end gap-2">
        <button type="button" disabled={busy} onClick={() => void make()} className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-300 disabled:opacity-50 dark:border-white/10 dark:text-zinc-200">
          {busy && <Loader2 size={13} className="animate-spin" />}{t(busy ? 'exportLiveMaking' : 'exportLiveMake')}
        </button>
        {made && (
          <button type="button" onClick={() => void saveFile(made.file_name, { url: `/v1/exports/live/${encodeURIComponent(made.id)}/file` })} className="inline-flex items-center gap-1.5 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white">
            <Download size={13} />{t('exportSave')}
          </button>
        )}
      </div>
    </section>
  );
};
