import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AlertTriangle, CheckCircle2, Download, Loader2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import { apiUrl } from '../services/apiBase';
import { saveFile } from '../services/saveFile';
import { SongMidiFiles } from './SongMidiFiles';
import { AbletonProject } from './AbletonProject';

const EXPORT_EVENT = 'studio:export';

/** Opens the export window for a song from anywhere in the studio. */
export function openExport(song: { id: string; title?: string }) {
  window.dispatchEvent(new CustomEvent(EXPORT_EVENT, { detail: { id: song.id, title: song.title ?? '' } }));
}

type Format = 'wav' | 'flac' | 'mp3';
type Depth = '16' | '24' | '32f';

interface Measure { lufs: number | null; true_peak_dbtp: number; sample_peak_dbfs: number }
interface Note { code: string; params: Record<string, number> }
interface Report {
  file_name: string;
  bytes: number;
  format: Format;
  sample_rate: number;
  bit_depth: Depth | null;
  mp3_kbps: number | null;
  source_rate: number;
  source: { bits: number | null; float: boolean; lossy: boolean };
  gain_db: number;
  before: Measure;
  after: Measure;
  notes: Note[];
  original_sha256: string;
  original_unchanged: boolean;
}
interface Job { id: string; stage: string; fraction: number; done: boolean; error: string | null; report: Report | null }

/** A target names a use, not a promise about how a service will play the song back. */
const TARGETS: { id: string; lufs: number | null; ceiling: number }[] = [
  { id: 'off', lufs: null, ceiling: -1 },
  { id: 'stream', lufs: -14, ceiling: -1 },
  { id: 'apple', lufs: -16, ceiling: -1 },
  { id: 'ebu', lufs: -23, ceiling: -1 },
  { id: 'custom', lufs: -14, ceiling: -1 },
];

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 aria-invalid:border-rose-500 dark:border-white/10 dark:bg-black/25 dark:text-white dark:aria-invalid:border-rose-500';
const LABEL = 'mb-1 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400';

const level = (value: number | null | undefined, unit: string) => (value === null || value === undefined || !Number.isFinite(value) ? '–' : `${value.toFixed(1)} ${unit}`);

const forget = (id: string) => void fetch(apiUrl(`/v1/exports/${encodeURIComponent(id)}`), { method: 'DELETE' }).catch(() => undefined);

export const ExportDialog: React.FC = () => {
  const { t } = useI18n();
  const [song, setSong] = useState<{ id: string; title: string } | null>(null);
  const [format, setFormat] = useState<Format>('wav');
  const [rate, setRate] = useState<string>('source');
  const [depth, setDepth] = useState<Depth>('24');
  const [kbps, setKbps] = useState('320');
  const [target, setTarget] = useState('off');
  const [customLufs, setCustomLufs] = useState('-14');
  const [customCeiling, setCustomCeiling] = useState('-1');
  const [job, setJob] = useState<Job | null>(null);
  const [error, setError] = useState<string | null>(null);
  const poll = useRef<number | null>(null);
  const held = useRef<string | null>(null);

  const letGo = () => {
    if (poll.current) window.clearInterval(poll.current);
    poll.current = null;
    if (held.current) forget(held.current);
    held.current = null;
  };

  useEffect(() => {
    const open = (event: Event) => {
      letGo();
      setSong((event as CustomEvent<{ id: string; title: string }>).detail);
      setJob(null);
      setError(null);
    };
    window.addEventListener(EXPORT_EVENT, open);
    return () => {
      window.removeEventListener(EXPORT_EVENT, open);
      letGo();
    };
  }, []);

  // each format holds its own depths and rates
  useEffect(() => {
    if (format === 'flac' && depth === '32f') setDepth('24');
    if (format === 'mp3' && (rate === '88200' || rate === '96000')) setRate('44100');
  }, [format, depth, rate]);

  if (!song) return null;

  const busy = job !== null && !job.done;
  const close = () => {
    if (busy) return;
    letGo();
    setSong(null);
  };

  // a decimal comma is a decimal point; what is not a number is not sent
  const decimal = (text: string) => (text.trim() === '' ? Number.NaN : Number(text.trim().replace(',', '.')));
  const customOk = target !== 'custom' || (Number.isFinite(decimal(customLufs)) && Number.isFinite(decimal(customCeiling)));
  const chosen = TARGETS.find(entry => entry.id === target) ?? TARGETS[0];
  const loudness = chosen.lufs === null
    ? null
    : chosen.id === 'custom'
      ? { target_lufs: decimal(customLufs), ceiling_dbtp: decimal(customCeiling) }
      : { target_lufs: chosen.lufs, ceiling_dbtp: chosen.ceiling };

  const start = async () => {
    setError(null);
    letGo();
    const body = {
      song_id: song.id,
      format,
      sample_rate: rate === 'source' ? null : Number(rate),
      bit_depth: format === 'mp3' ? null : depth,
      mp3_kbps: format === 'mp3' ? Number(kbps) : null,
      loudness,
    };
    try {
      const response = await fetch(apiUrl('/v1/exports'), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      const answer = await response.json().catch(() => null);
      if (!response.ok) {
        setError(answer?.error ?? `HTTP ${response.status}`);
        return;
      }
      const id = String(answer.id);
      held.current = id;
      setJob({ id, stage: 'queued', fraction: 0, done: false, error: null, report: null });
      poll.current = window.setInterval(async () => {
        const next = await fetch(apiUrl(`/v1/exports/${encodeURIComponent(id)}`)).then(r => (r.ok ? r.json() : null)).catch(() => null);
        if (!next || held.current !== id) return;
        setJob(next as Job);
        if (next.done && poll.current) {
          window.clearInterval(poll.current);
          poll.current = null;
        }
      }, 400);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const cancel = () => {
    if (job) void fetch(apiUrl(`/v1/exports/${encodeURIComponent(job.id)}/cancel`), { method: 'POST' }).catch(() => undefined);
  };

  const report = job?.report ?? null;
  const save = () => {
    if (report && job) void saveFile(report.file_name, { url: `/v1/exports/${encodeURIComponent(job.id)}/file` });
  };
  const again = () => {
    letGo();
    setJob(null);
  };

  const bits = (value: number | string) => t('exportBits').replace('{bits}', String(value));
  const noteText = (note: Note) => Object.entries(note.params ?? {}).reduce(
    (text, [name, value]) => text.replace(`{${name}}`, name === 'gain' && value > 0 ? `+${value}` : String(value)),
    t(`exportNote_${note.code}` as TranslationKey),
  );

  return createPortal(
    <div className="fixed inset-0 z-80 flex items-center justify-center bg-black/50 p-4" role="dialog" aria-modal="true" aria-label={t('exportTitle')}>
      <div className="max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-2xl border border-zinc-200 bg-white p-5 shadow-xl dark:border-white/10 dark:bg-suno-card">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="text-lg font-bold text-zinc-900 dark:text-white">{t('exportTitle')}</h2>
            <p className="text-xs text-zinc-500">{t('exportIntro').replace('{title}', song.title || t('untitled'))}</p>
          </div>
          <button type="button" onClick={close} disabled={busy} className="rounded p-1 text-zinc-400 hover:text-zinc-700 disabled:opacity-40 dark:hover:text-zinc-200" title={t('close')} aria-label={t('close')}><X size={16} /></button>
        </div>

        {!report && (
          <div className="mt-4 grid grid-cols-2 gap-3">
            <label className="block">
              <span className={LABEL}>{t('exportFormat')}</span>
              <select className={CONTROL} value={format} disabled={busy} onChange={event => setFormat(event.target.value as Format)}>
                <option value="wav">WAV</option>
                <option value="flac">{t('exportFormatFlac')}</option>
                <option value="mp3">{t('exportFormatMp3')}</option>
              </select>
            </label>
            <label className="block">
              <span className={LABEL}>{t('exportRate')}</span>
              <select className={CONTROL} value={rate} disabled={busy} onChange={event => setRate(event.target.value)}>
                <option value="source">{t('exportRateSource')}</option>
                <option value="44100">44.1 kHz</option>
                <option value="48000">48 kHz</option>
                {format !== 'mp3' && <option value="88200">88.2 kHz</option>}
                {format !== 'mp3' && <option value="96000">96 kHz</option>}
              </select>
            </label>
            {format === 'mp3' ? (
              <label className="block">
                <span className={LABEL}>{t('exportBitrate')}</span>
                <select className={CONTROL} value={kbps} disabled={busy} onChange={event => setKbps(event.target.value)}>
                  {['320', '256', '224', '192', '160', '128'].map(value => <option key={value} value={value}>{value} kbps CBR</option>)}
                </select>
              </label>
            ) : (
              <label className="block">
                <span className={LABEL}>{t('exportDepth')}</span>
                <select className={CONTROL} value={depth} disabled={busy} onChange={event => setDepth(event.target.value as Depth)}>
                  <option value="16">{bits(16)}</option>
                  <option value="24">{bits(24)}</option>
                  {format === 'wav' && <option value="32f">{t('exportFloat')}</option>}
                </select>
              </label>
            )}
            <label className="col-span-2 block">
              <span className={LABEL}>{t('exportLoudness')}</span>
              <select className={CONTROL} value={target} disabled={busy} onChange={event => setTarget(event.target.value)}>
                {TARGETS.map(entry => <option key={entry.id} value={entry.id}>{t(`exportTarget_${entry.id}` as TranslationKey)}</option>)}
              </select>
            </label>
            {target === 'custom' && (
              <>
                <label className="block"><span className={LABEL}>{t('exportTargetLufs')}</span><input className={CONTROL} inputMode="decimal" value={customLufs} disabled={busy} aria-invalid={!Number.isFinite(decimal(customLufs))} onChange={event => setCustomLufs(event.target.value)} /></label>
                <label className="block"><span className={LABEL}>{t('exportCeiling')}</span><input className={CONTROL} inputMode="decimal" value={customCeiling} disabled={busy} aria-invalid={!Number.isFinite(decimal(customCeiling))} onChange={event => setCustomCeiling(event.target.value)} /></label>
              </>
            )}
            <p className="col-span-2 text-[11px] leading-4 text-zinc-500">{t('exportHint')}</p>
          </div>
        )}

        {job && !job.done && (
          <div className="mt-4">
            <div className="flex items-center justify-between text-xs text-zinc-600 dark:text-zinc-300">
              <span className="flex items-center gap-1.5"><Loader2 size={13} className="animate-spin" />{t(`exportStage_${job.stage}` as TranslationKey)}</span>
              <button type="button" onClick={cancel} className="text-xs font-semibold text-rose-600 hover:text-rose-500">{t('cancel')}</button>
            </div>
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
              <div className="h-full bg-pink-500 transition-all" style={{ width: `${Math.round(job.fraction * 100)}%` }} />
            </div>
            <p className="mt-1 text-[10px] text-zinc-400">{t('exportStepNote')}</p>
          </div>
        )}

        {job?.done && job.error && (
          <p className="mt-4 flex gap-1.5 text-sm text-rose-600 dark:text-rose-300">
            <AlertTriangle size={15} className="mt-0.5 shrink-0" />
            {job.error === 'cancelled' ? t('exportCancelled') : job.error}
          </p>
        )}

        {report && (
          <div className="mt-4 space-y-3 text-sm">
            <p className="flex gap-1.5 text-emerald-700 dark:text-emerald-300"><CheckCircle2 size={16} className="mt-0.5 shrink-0" />{report.file_name} · {(report.bytes / 1_048_576).toFixed(1)} MB</p>
            <table className="w-full text-left text-xs text-zinc-800 dark:text-zinc-200">
              <tbody className="[&_td]:py-0.5 [&_td:first-child]:pr-3 [&_td:first-child]:text-zinc-500">
                <tr><td>{t('exportWrittenAs')}</td><td>{report.format.toUpperCase()}, {report.sample_rate / 1000} kHz, {report.bit_depth ? (report.bit_depth === '32f' ? t('exportFloat') : bits(report.bit_depth)) : `${report.mp3_kbps} kbps`}</td></tr>
                <tr><td>{t('exportFrom')}</td><td>{report.source_rate / 1000} kHz, {report.source.lossy ? t('exportLossy') : report.source.float ? t('exportFloat') : bits(report.source.bits ?? '?')}</td></tr>
                <tr><td>{t('exportLoudness')}</td><td>{level(report.before.lufs, 'LUFS')} → {level(report.after.lufs, 'LUFS')}</td></tr>
                <tr><td>{t('exportTruePeak')}</td><td>{level(report.before.true_peak_dbtp, 'dBTP')} → {level(report.after.true_peak_dbtp, 'dBTP')}</td></tr>
                <tr><td>{t('exportGain')}</td><td>{report.gain_db >= 0 ? '+' : ''}{report.gain_db.toFixed(2)} dB</td></tr>
                <tr><td>{t('exportOriginal')}</td><td>{report.original_unchanged ? t('exportUnchanged') : t('exportChanged')} (SHA-256 {report.original_sha256.slice(0, 12)}…)</td></tr>
              </tbody>
            </table>
            <ul className="list-disc space-y-0.5 pl-5 text-xs text-zinc-600 dark:text-zinc-300">
              {report.notes.map(note => <li key={note.code}>{noteText(note)}</li>)}
            </ul>
          </div>
        )}

        {error && <p className="mt-3 text-xs text-rose-600 dark:text-rose-300">{error}</p>}

        <div className="mt-5 flex justify-end gap-2">
          {report ? (
            <>
              <button type="button" onClick={again} className="rounded-lg border border-zinc-200 px-3 py-2 text-xs font-semibold text-zinc-700 dark:border-white/10 dark:text-zinc-200">{t('exportAgain')}</button>
              <button type="button" onClick={save} className="inline-flex items-center gap-1.5 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white"><Download size={13} />{t('exportSave')}</button>
            </>
          ) : (
            <button type="button" disabled={busy || !customOk} onClick={() => void start()} className="inline-flex items-center gap-1.5 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white disabled:opacity-50">
              {busy && <Loader2 size={13} className="animate-spin" />}{t('exportStart')}
            </button>
          )}
        </div>

        <SongMidiFiles songId={song.id} />
        <AbletonProject songId={song.id} />
      </div>
    </div>,
    document.body,
  );
};
