import React from 'react';
import { ExternalLink, Search } from 'lucide-react';

const CARD = 'rounded-2xl border border-zinc-200 bg-zinc-50 p-4 dark:border-white/10 dark:bg-white/3';
const OUTLINE =
  'inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/10 dark:text-zinc-200';
const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/30 dark:text-white';

/** A catalogue's small coloured label: a kind, a family, a language. */
export function CatalogBadge({ children, title }: { children: React.ReactNode; title?: string }) {
  return (
    <span title={title} className="rounded-full bg-pink-500/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-pink-600 dark:text-pink-300">
      {children}
    </span>
  );
}

/**
 * One entry of a catalogue the studio downloads from: its name, who made it, its labels, what it is, a link to its page
 * and the action that takes it. The LoRA and the voice catalogues draw their entries with it.
 */
export function CatalogCard({ title, meta, badges, description, note, footer, page, pageLabel, onOpenPage, action, selected, children }: {
  title: React.ReactNode;
  meta?: React.ReactNode;
  badges?: React.ReactNode;
  description?: React.ReactNode;
  note?: React.ReactNode;
  footer?: React.ReactNode;
  page?: string | null;
  pageLabel?: string;
  onOpenPage?: (url: string) => void;
  action: React.ReactNode;
  selected?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <section className={`${CARD} flex flex-col ${selected ? 'border-pink-400 dark:border-pink-500/60' : ''}`}>
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{title}</p>
          {meta && <p className="mt-0.5 flex flex-wrap items-center gap-x-2 text-[11px] text-zinc-500">{meta}</p>}
        </div>
        {badges && <span className="flex shrink-0 items-center gap-1">{badges}</span>}
      </div>
      {description && <p className="mt-2 flex-1 text-xs leading-5 text-zinc-600 dark:text-zinc-300">{description}</p>}
      {note}
      <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
        <span>{footer}</span>
        <div className="flex items-center gap-2">
          {page && onOpenPage && (
            <button type="button" onClick={() => onOpenPage(page)} className={OUTLINE} title={pageLabel} aria-label={pageLabel}>
              <ExternalLink size={13} />
            </button>
          )}
          {action}
        </div>
      </div>
      {children}
    </section>
  );
}

/** The search field over a catalogue. */
export function CatalogSearch({ value, onChange, label }: { value: string; onChange: (value: string) => void; label: string }) {
  return (
    <div className="relative flex-1">
      <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
      <input type="search" value={value} onChange={event => onChange(event.target.value)} placeholder={label} aria-label={label}
        className={`${CONTROL} pl-9`} />
    </div>
  );
}

/** A catalogue's choice of a few values as one segmented control. */
export function CatalogSegments<T extends string>({ value, options, onChange }: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div role="radiogroup" className="flex shrink-0 rounded-lg bg-zinc-100 p-1 dark:bg-white/5">
      {options.map(option => (
        <button key={option.value} type="button" role="radio" aria-checked={value === option.value} onClick={() => onChange(option.value)}
          className={`rounded-md px-3 py-1 text-xs font-semibold ${value === option.value ? 'bg-white text-black shadow-xs dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}>
          {option.label}
        </button>
      ))}
    </div>
  );
}
