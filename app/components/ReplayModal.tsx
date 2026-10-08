import React, { useState } from 'react';
import { AlertTriangle, Loader2, Repeat, X } from 'lucide-react';
import { Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { parseDuration } from '../services/nativeLibrary';

/**
 * Deterministic re-render.
 *
 * YuE2 stores the semantic codes a track was rendered from. Re-submitting them
 * skips the autoregressive stage entirely: the composition and the vocal stay
 * identical while flow matching runs again, so this changes the step count,
 * the sound seed, the number of variations or the output format without
 * writing a different song.
 */

interface ReplayModalProps {
  song: Song;
  /** The window's mark for this request, handed back on the job. */
  clientRef: string;
  onClose: () => void;
  onQueued: (jobId: string) => void;
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

export const ReplayModal: React.FC<ReplayModalProps> = ({ song, clientRef, onClose, onQueued }) => {
  const { t } = useI18n();
  const settings = (song.generationParams ?? {}) as Record<string, unknown>;
  const numberOr = (key: string, fallback: number) =>
    typeof settings[key] === 'number' ? (settings[key] as number) : fallback;

  const [steps, setSteps] = useState<number>(numberOr('steps', 32));
  const [variations, setVariations] = useState<number>(1);
  // Empty keeps the track's own sound seed exactly as the service stored it:
  // the engine draws 64-bit seeds, which a JavaScript number cannot hold.
  const [seed, setSeed] = useState<string>('');
  const originalSeed = settings.seed === undefined || settings.seed === null ? '' : String(settings.seed);
  const [bitrate, setBitrate] = useState<number>(numberOr('mp3_bitrate', 320));
  const [format, setFormat] = useState<'flac' | 'mp3'>(settings.output_format === 'mp3' ? 'mp3' : 'flac');
  // seconds composed on after the last frame; 0 renders the track as it is
  const [extend, setExtend] = useState(0);
  // the service makes songs up to ten minutes long
  const room = 600 - (parseDuration(song.duration) ?? 0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const parsedSeed = seed.trim() === '' ? undefined : Number(seed);
      if (parsedSeed !== undefined && (!Number.isSafeInteger(parsedSeed) || parsedSeed < 0)) {
        throw new Error('Seed must be a non-negative integer.');
      }
      const response = await fetch('/v1/music/replay', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          song_id: song.id,
          client_ref: clientRef,
          steps,
          seed: parsedSeed,
          synth_batch_size: variations,
          output_format: format,
          mp3_bitrate: format === 'mp3' ? bitrate : undefined,
          extend_seconds: extend > 0 ? extend : undefined,
        }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Re-render failed (${response.status})`);
      onQueued(body.id);
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Re-render failed.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-70 flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <div className="w-full max-w-md overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-zinc-900" onClick={event => event.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-4 dark:border-white/10">
          <h3 className="flex items-center gap-2 text-base font-bold text-zinc-900 dark:text-white">
            <Repeat size={17} className="text-pink-500" /> {t('replayTitle')}
          </h3>
          <button type="button" onClick={onClose} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200">
            <X size={18} />
          </button>
        </div>

        <div className="space-y-3 p-5">
          <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{song.title}</p>
          <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('replayHint')}</p>

          <div className="grid grid-cols-2 gap-3">
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('flowSteps')}</span>
              <input type="number" min={1} max={200} value={steps} onChange={event => setSteps(Math.max(1, Number(event.target.value) || 1))} className={CONTROL} />
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('variationsPerRender')}</span>
              <input type="number" min={1} max={9} value={variations} onChange={event => setVariations(Math.min(9, Math.max(1, Number(event.target.value) || 1)))} className={CONTROL} />
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('noiseSeed')}</span>
              <input inputMode="numeric" value={seed} placeholder={originalSeed || '-1'} onChange={event => setSeed(event.target.value)} className={CONTROL} />
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('outputFormat')}</span>
              <select value={format} onChange={event => setFormat(event.target.value as typeof format)} className={CONTROL}>
                <option value="flac">FLAC</option>
                <option value="mp3">MP3</option>
              </select>
            </label>
            <label className="col-span-2 block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('replayExtend')}</span>
              <select value={extend} onChange={event => setExtend(Number(event.target.value))} className={CONTROL}>
                {[0, 15, 30, 60, 90, 120].filter(seconds => seconds <= room).map(seconds => <option key={seconds} value={seconds}>{seconds === 0 ? t('replayExtendNone') : `+${seconds} s`}</option>)}
              </select>
              {extend > 0 && <span className="mt-1 block text-[11px] leading-4 text-zinc-500">{t('replayExtendHint')}</span>}
            </label>
            {format === 'mp3' && (
              <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
                <span className="mb-1.5 block">{t('mp3Bitrate')}</span>
                <select value={bitrate} onChange={event => setBitrate(Number(event.target.value))} className={CONTROL}>
                  {[128, 192, 256, 320].map(rate => <option key={rate} value={rate}>{rate} kbps</option>)}
                </select>
              </label>
            )}
          </div>

          {error && (
            <p role="alert" className="flex items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
              <AlertTriangle size={14} /> {error}
            </p>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-zinc-200 px-5 py-4 dark:border-white/10">
          <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-sm font-medium text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200">
            {t('cancel')}
          </button>
          <button
            type="button"
            onClick={() => void submit()}
            disabled={busy}
            className="inline-flex items-center gap-2 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:opacity-50"
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Repeat size={14} />} {t('replayStart')}
          </button>
        </div>
      </div>
    </div>
  );
};
