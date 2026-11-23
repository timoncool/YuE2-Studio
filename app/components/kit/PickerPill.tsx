import React, { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown } from 'lucide-react';

type Place = { left: number; width: number; top?: number; bottom?: number; height: number };

/**
 * A compact choice the create form shows as one line — "QUALITY · Balanced ▾" — with its list opening under it.
 * The list is portalled and placed against the window, like Combobox, so a narrow scrolling panel never clips it.
 * `children` gets `close` to call after a pick.
 */
export function PickerPill({ label, value, status, children, className = '', icon, align = 'left', showLabel = true }: {
  label: string;
  value?: React.ReactNode;
  /** A small mark before the value: ready, downloading, missing. */
  status?: React.ReactNode;
  children: (close: () => void) => React.ReactNode;
  className?: string;
  /** An icon-only trigger, named by `label`. */
  icon?: React.ReactNode;
  align?: 'left' | 'right';
  showLabel?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<Place | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const list = useId();
  useEffect(() => {
    if (!open) return;
    const away = (event: MouseEvent) => {
      const at = event.target as Node;
      if (!box.current?.contains(at) && !menu.current?.contains(at)) setOpen(false);
    };
    const esc = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.stopPropagation(); setOpen(false); } };
    window.addEventListener('mousedown', away);
    window.addEventListener('keydown', esc, true);
    return () => { window.removeEventListener('mousedown', away); window.removeEventListener('keydown', esc, true); };
  }, [open]);
  useLayoutEffect(() => {
    if (!open) { setPlace(null); return; }
    const measure = (event?: Event) => {
      if (event?.type === 'scroll' && menu.current?.contains(event.target as Node)) return;
      const r = box.current?.getBoundingClientRect();
      if (!r) return;
      const width = Math.min(352, window.innerWidth - 16);
      const left = Math.min(Math.max(8, align === 'right' ? r.right - width : r.left), window.innerWidth - width - 8);
      const below = window.innerHeight - r.bottom - 12, above = r.top - 12;
      const down = below >= Math.min(240, above);
      const height = Math.min(window.innerHeight * 0.7, down ? below : above);
      setPlace(down ? { left, width, top: r.bottom + 4, height } : { left, width, bottom: window.innerHeight - r.top + 4, height });
    };
    measure();
    window.addEventListener('resize', measure);
    window.addEventListener('scroll', measure, true);
    return () => { window.removeEventListener('resize', measure); window.removeEventListener('scroll', measure, true); };
  }, [open, align]);
  return (
    <div ref={box} className={`relative min-w-0 ${className}`}>
      <button type="button" aria-haspopup="true" aria-expanded={open} aria-controls={open ? list : undefined} onClick={() => setOpen(value => !value)}
        aria-label={icon ? label : undefined} title={icon ? label : undefined}
        className={`flex min-h-9 min-w-0 items-center gap-2 rounded-lg border text-left transition-colors ${icon ? 'w-9 justify-center' : 'w-full px-2.5 py-2'} ${open ? 'border-pink-500 bg-pink-500/5' : 'border-zinc-200 bg-zinc-50 hover:border-pink-300 dark:border-white/10 dark:bg-black/25'}`}>
        {icon ?? <>
          {showLabel && <span className="shrink-0 text-[10px] font-bold uppercase tracking-wide text-zinc-400">{label}</span>}
          {status}
          <span className="min-w-0 flex-1 truncate text-sm font-semibold text-zinc-900 dark:text-white">{value}</span>
          <ChevronDown size={14} className={`shrink-0 text-zinc-400 transition-transform ${open ? 'rotate-180' : ''}`} />
        </>}
      </button>
      {open && place && createPortal(
        <div ref={menu} id={list} role="menu" aria-label={label}
          style={{ left: place.left, top: place.top, bottom: place.bottom, width: place.width, maxHeight: place.height }}
          className="fixed z-[80] overflow-y-auto rounded-xl border border-zinc-200 bg-white p-2 shadow-2xl dark:border-white/10 dark:bg-zinc-900">
          {children(() => setOpen(false))}
        </div>,
        document.body,
      )}
    </div>
  );
}

/** One choice in a pill's list: its name, a line on what it is, a side note (size, "recommended") and a mark. */
export function PickerOption({ selected, onPick, name, hint, side, mark, disabled, value }: {
  selected: boolean;
  onPick: () => void;
  name: React.ReactNode;
  hint?: React.ReactNode;
  side?: React.ReactNode;
  mark?: React.ReactNode;
  disabled?: boolean;
  /** Identifies the choice for the window's agent and tests. */
  value?: string;
}) {
  return (
    <button type="button" role="menuitemradio" aria-checked={selected} disabled={disabled} onClick={onPick} data-value={value}
      className={`flex w-full min-w-0 items-start gap-2 rounded-lg px-2 py-2 text-left transition-colors disabled:opacity-40 ${selected ? 'bg-pink-500/10' : 'hover:bg-zinc-100 dark:hover:bg-white/5'}`}>
      <span className="min-w-0 flex-1">
        <span className={`block truncate text-xs font-semibold ${selected ? 'text-pink-600 dark:text-pink-300' : 'text-zinc-900 dark:text-white'}`}>{name}</span>
        {hint && <span className="mt-0.5 block text-[11px] leading-4 text-zinc-500">{hint}</span>}
      </span>
      {side && <span className="shrink-0 text-[10px] font-semibold text-zinc-500 dark:text-zinc-400">{side}</span>}
      {mark && <span className="mt-0.5 shrink-0">{mark}</span>}
    </button>
  );
}
