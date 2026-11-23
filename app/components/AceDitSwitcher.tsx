import React, { useCallback, useEffect, useRef, useState } from 'react';
import { formatSize } from '../services/fileSize';
import { Check, ChevronDown, Download, Loader2, Square } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { nativeModelSetStrings } from '../i18n/nativeModelSet';
import { apiUrl } from '../services/apiBase';
import type { SetDownload } from '../services/nativeMusicProfiles';
import { WorkProgress } from './WorkProgress';

/**
 * ACE-Step Studio 4.0.1's model switcher: the DiT a song renders with, picked where songs are made. Its list holds
 * every variant of the catalogue with its quantisations; an installed one is used at once, a missing one is
 * downloaded and becomes the song's DiT when the download finishes. The set's language model, text encoder and VAE
 * stay as they are.
 */

type Dit = { file: string; bytes: number; installed: boolean; download?: SetDownload | null };

const QUANTS = ['q4', 'q5', 'q6', 'q8', 'mxfp4'];
const QUANT_LABEL: Record<string, string> = { q4: 'Q4', q5: 'Q5', q6: 'Q6', q8: 'Q8', mxfp4: 'MXFP4' };
const QUANT_OF: Record<string, string> = { Q4_K_M: 'q4', Q5_K_M: 'q5', Q6_K: 'q6', Q8_0: 'q8', MXFP4: 'mxfp4' };

const DIT_GROUPS: Array<{ key: string; variants: string[] }> = [
  { key: 'xl', variants: ['xl-turbo', 'xl-sft', 'xl-base', 'xl-sftturbo50'] },
  { key: 'standard', variants: ['turbo', 'sft', 'base', 'sftturbo50', 'turbo-shift1', 'turbo-shift3', 'turbo-continuous'] },
  { key: 'merges', variants: ['merge-sft-turbo-xl-ta-0.3', 'merge-sft-turbo-xl-ta-0.7', 'merge-base-turbo-xl-ta-0.5', 'merge-base-sft-xl-ta-0.5'] },
  { key: 'community', variants: ['xl-turbo-regrind'] },
];

/** `acestep-v15-xl-turbo-Q8_0.gguf` is the variant `xl-turbo` in the quantisation `q8`. */
export const splitDit = (file: string): { variant: string; quant: string } => {
  const match = /^(.*)-(Q4_K_M|Q5_K_M|Q6_K|Q8_0|MXFP4|BF16)\.gguf$/.exec(file);
  if (!match) return { variant: file, quant: '' };
  const stem = match[1].startsWith('acestep_1.5_xl_turbo_regrind') ? 'xl-turbo-regrind' : match[1].replace(/^acestep-v15-/, '');
  return { variant: stem, quant: QUANT_OF[match[2]] ?? match[2].toLowerCase() };
};


export function AceDitSwitcher({ value, setDit, onChange }: {
  /** The DiT picked for the song; empty for the set's own. */
  value: string;
  /** The DiT of the chosen set. */
  setDit?: string;
  onChange: (file: string) => void;
}) {
  const { t, language } = useI18n();
  const bytes = (value: number) => formatSize(value, language);
  const tt = t as unknown as (key: string) => string;
  const cancelLabel = nativeModelSetStrings[language].cancel;
  const [dits, setDits] = useState<Dit[] | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [wanted, setWanted] = useState<string | null>(null);
  const box = useRef<HTMLDivElement | null>(null);

  const load = useCallback(async () => {
    try {
      const response = await fetch(apiUrl('/v1/ace/dits'));
      const body = await response.json().catch(() => null) as { dits?: Dit[]; error?: string } | null;
      if (!response.ok || !body?.dits) throw new Error(body?.error || String(response.status));
      setDits(body.dits);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, []);
  useEffect(() => { void load(); }, [load]);

  const downloading = dits?.find(dit => dit.download && !dit.download.done) ?? null;
  useEffect(() => {
    if (!downloading && !wanted) return undefined;
    const timer = window.setInterval(() => void load(), 1500);
    return () => window.clearInterval(timer);
  }, [downloading, wanted, load]);

  useEffect(() => {
    if (!wanted || !dits) return;
    const dit = dits.find(entry => entry.file === wanted);
    if (dit?.installed) {
      onChange(wanted);
      setWanted(null);
    } else if (dit?.download?.done && dit.download.error) {
      setError(dit.download.error);
      setWanted(null);
    }
  }, [dits, wanted, onChange]);

  useEffect(() => {
    if (!open) return undefined;
    const close = (event: MouseEvent) => {
      if (box.current && !box.current.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', close);
    return () => window.removeEventListener('mousedown', close);
  }, [open]);

  const current = value || setDit || '';
  const name = (variant: string) => tt(`aceVariant_${variant}`);

  const pick = async (dit: Dit) => {
    setError(null);
    setOpen(false);
    if (dit.installed) {
      onChange(dit.file === setDit ? '' : dit.file);
      return;
    }
    try {
      const response = await fetch(apiUrl('/v1/ace/dits/install'), { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ file: dit.file }) });
      if (!response.ok) {
        const body = await response.json().catch(() => null) as { error?: string } | null;
        throw new Error(body?.error || String(response.status));
      }
      setWanted(dit.file);
      await load();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const cancel = async () => {
    if (!downloading) return;
    await fetch(apiUrl('/v1/ace/dits/install'), { method: 'DELETE', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ file: downloading.file }) });
    setWanted(null);
    await load();
  };

  const shown = current ? splitDit(current) : null;
  return (
    <div ref={box} className="relative">
      <button
        type="button"
        onClick={() => setOpen(state => !state)}
        aria-expanded={open}
        className={`flex w-full min-w-0 items-center gap-2 rounded-lg border px-2.5 py-2 text-left transition-colors ${open ? 'border-pink-500 bg-pink-500/5' : 'border-zinc-200 bg-zinc-50 hover:border-pink-300 dark:border-white/10 dark:bg-black/25'}`}
      >
        <span className="shrink-0 text-[10px] font-bold uppercase tracking-wide text-zinc-400">DiT</span>
        <span className="min-w-0 flex-1 truncate text-xs font-semibold text-zinc-900 dark:text-white">
          {shown ? `${name(shown.variant)} · ${QUANT_LABEL[shown.quant] ?? shown.quant.toUpperCase()}` : '—'}
        </span>
        <ChevronDown size={14} className={`shrink-0 text-zinc-400 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && (
        <div className="mt-1 max-h-[60vh] overflow-y-auto rounded-xl border border-zinc-200 bg-white p-2 dark:border-white/10 dark:bg-zinc-900">
          <p className="px-2 pb-1 text-[11px] text-zinc-500">
            <Check size={10} className="inline" /> {tt('aceInstalledLegend')} · <Download size={10} className="inline" /> {tt('aceDownloadLegend')}
          </p>
          {DIT_GROUPS.map(group => (
            <div key={group.key} className="mb-1 last:mb-0">
              <p className="px-2 pb-1 pt-1.5 text-[10px] font-bold uppercase tracking-wide text-zinc-400">{tt(`aceVariantGroup_${group.key}`)}</p>
              {group.variants.map(variant => {
                const quants = (dits ?? []).filter(dit => splitDit(dit.file).variant === variant)
                  .sort((a, b) => QUANTS.indexOf(splitDit(a.file).quant) - QUANTS.indexOf(splitDit(b.file).quant));
                if (quants.length === 0) return null;
                return (
                  <div key={variant} className={`rounded-lg px-2 py-2 ${quants.some(dit => dit.file === current) ? 'bg-pink-500/5' : ''}`}>
                    <span className="text-xs font-semibold text-zinc-900 dark:text-white">{name(variant)}</span>
                    <p className="mt-0.5 text-[11px] leading-4 text-zinc-500">{tt(`aceVariantHint_${variant}`)}</p>
                    <div className="mt-1.5 flex flex-wrap gap-1">
                      {quants.map(dit => {
                        const on = dit.file === current;
                        const quant = splitDit(dit.file).quant;
                        return (
                          <button
                            key={dit.file}
                            type="button"
                            disabled={downloading !== null && !dit.installed}
                            onClick={() => void pick(dit)}
                            title={`${bytes(dit.bytes)}${quant === 'mxfp4' ? ` · ${tt('aceMxfp4Hint')}` : ''}${dit.installed ? '' : ` · ${tt('aceDownloadAndUse')}`}`}
                            className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10px] font-semibold tabular-nums transition-colors disabled:opacity-40 ${
                              on
                                ? 'border-pink-500 bg-pink-500 text-white'
                                : dit.installed
                                  ? 'border-zinc-300 text-zinc-700 hover:border-pink-400 dark:border-white/20 dark:text-zinc-200'
                                  : 'border-dashed border-zinc-300 text-zinc-500 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-400'
                            }`}
                          >
                            {wanted === dit.file ? <Loader2 size={10} className="animate-spin" /> : on || dit.installed ? <Check size={10} /> : <Download size={10} />}
                            {QUANT_LABEL[quant] ?? quant.toUpperCase()}
                            <span className="font-normal opacity-80">{bytes(dit.bytes)}</span>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      )}
      {downloading?.download && (
        <div className="flex items-end gap-2">
          <div className="min-w-0 flex-1">
            <WorkProgress
              label={`${tt('aceModelDownloading')} ${bytes(downloading.download.downloaded_bytes)} / ${bytes(downloading.download.total_bytes)}`}
              percent={downloading.download.total_bytes ? Math.min(100, Math.round((downloading.download.downloaded_bytes / downloading.download.total_bytes) * 100)) : null}
            />
          </div>
          <button type="button" onClick={() => void cancel()} className="mb-0.5 text-zinc-400 hover:text-rose-500" title={cancelLabel} aria-label={cancelLabel}><Square size={12} /></button>
        </div>
      )}
      {error && <p role="alert" className="mt-2 px-1 text-[11px] text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
}
