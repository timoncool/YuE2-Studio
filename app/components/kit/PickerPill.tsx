import React, { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { ChevronDown } from 'lucide-react';

type Place = { left: number; width: number; top?: number; bottom?: number; height: number };

const FOCUSABLE = 'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';
const ITEM = '[role^="menuitem"]:not(:disabled)';

/**
 * A compact choice the create form shows as one line — "● MiniMax Music3 ▾" — with its list opening under it.
 * The list is portalled and placed against the window, like Combobox, so a narrow scrolling panel never clips it.
 * `kind="menu"` holds PickerOption items and moves by arrow keys (APG menu button); `kind="dialog"` holds any
 * controls and moves by Tab. `children` gets `close` to call after a pick; focus goes back to the trigger.
 */
export function PickerPill({ label, value, status, children, className = '', icon, align = 'left', showLabel = true, kind = 'menu', look = 'field' }: {
  label: string;
  value?: React.ReactNode;
  /** A small mark before the value: ready, starting, failed, needs a download. */
  status?: React.ReactNode;
  children: (close: () => void) => React.ReactNode;
  className?: string;
  /** An icon-only trigger, named by `label`. */
  icon?: React.ReactNode;
  align?: 'left' | 'right';
  showLabel?: boolean;
  kind?: 'menu' | 'dialog';
  /** `split` is the second half of a split button, as tall as the main action beside it. */
  look?: 'field' | 'split' | 'primary' | 'add';
}) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<Place | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const list = useId();
  const close = (returnFocus = true) => {
    setOpen(false);
    if (returnFocus) trigger.current?.focus();
  };
  useEffect(() => {
    if (!open) return;
    const away = (event: MouseEvent) => {
      const at = event.target as Node;
      if (!box.current?.contains(at) && !menu.current?.contains(at)) setOpen(false);
    };
    const esc = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.stopPropagation(); close(); } };
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
  const placed = Boolean(place);
  useEffect(() => {
    const pane = menu.current;
    if (!open || !placed || !pane) return;
    const first = kind === 'menu'
      ? pane.querySelector<HTMLElement>(`${ITEM}[aria-checked="true"]`) ?? pane.querySelector<HTMLElement>(ITEM)
      : pane.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? pane).focus();
  }, [open, placed, kind]);
  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (kind === 'dialog') {
      if (event.key !== 'Tab') return;
      const controls = [...(menu.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])];
      const edge = event.shiftKey ? controls[0] : controls.at(-1);
      // leaving the panel by Tab returns to its trigger, not to the end of the page the panel is portalled into
      if (!controls.length || document.activeElement === edge || document.activeElement === menu.current) { event.preventDefault(); close(); }
      return;
    }
    const items = [...(menu.current?.querySelectorAll<HTMLElement>(ITEM) ?? [])];
    if (!items.length) return;
    const at = items.indexOf(document.activeElement as HTMLElement);
    const go = (index: number) => { event.preventDefault(); items[(index + items.length) % items.length].focus(); };
    if (event.key === 'ArrowDown') go(at + 1);
    else if (event.key === 'ArrowUp') go(at < 0 ? items.length - 1 : at - 1);
    else if (event.key === 'Home') go(0);
    else if (event.key === 'End') go(items.length - 1);
    else if (event.key === 'Tab') { event.preventDefault(); close(); }
  };
  const onBlur = (event: React.FocusEvent<HTMLDivElement>) => {
    const next = event.relatedTarget as Node | null;
    if (next && !menu.current?.contains(next) && !box.current?.contains(next)) close(false);
  };
  return (
    <div ref={box} className={`relative min-w-0 ${className}`}>
      <button ref={trigger} type="button" aria-haspopup={kind} aria-expanded={open} aria-controls={open ? list : undefined}
        onClick={() => setOpen(value => !value)}
        onKeyDown={event => { if (kind === 'menu' && (event.key === 'ArrowDown' || event.key === 'ArrowUp') && !open) { event.preventDefault(); setOpen(true); } }}
        aria-label={icon ? label : undefined} title={icon ? label : undefined}
        className={look === 'add'
          ? `flex min-h-9 w-full items-center justify-center gap-1.5 rounded-lg border border-dashed py-2 text-xs font-semibold transition-colors max-md:min-h-11 ${open ? 'border-pink-500 text-pink-600 dark:text-pink-300' : 'border-zinc-300 text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300'}`
          : look === 'primary'
          ? `flex h-12 w-11 items-center justify-center rounded-r-xl border-l border-white/30 bg-pink-600 text-white transition hover:brightness-110 ${open ? 'brightness-110' : ''}`
          : look === 'split'
          ? `flex h-12 min-w-0 items-center justify-center gap-2 rounded-xl border text-sm font-semibold transition-colors ${icon ? 'w-11' : 'w-full px-3'} ${open ? 'border-pink-500 bg-pink-500/5 text-pink-600 dark:text-pink-300' : 'border-zinc-300 bg-white text-zinc-700 hover:border-pink-400 dark:border-white/15 dark:bg-white/5 dark:text-zinc-200'}`
          : `flex min-h-9 min-w-0 items-center gap-2 rounded-lg border text-left transition-colors max-md:min-h-11 ${icon ? 'w-9 justify-center max-md:w-11' : 'w-full px-2.5 py-2'} ${open ? 'border-pink-500 bg-pink-500/5' : 'border-zinc-200 bg-zinc-50 hover:border-pink-300 dark:border-white/10 dark:bg-black/25'}`}>
        {icon ?? <>
          {showLabel ? <span className="shrink-0 text-[10px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</span>
            : look !== 'add' && <span className="sr-only">{label}: </span>}
          {status}
          <span className={look === 'split' || look === 'add' ? 'min-w-0 truncate' : 'min-w-0 flex-1 truncate text-sm font-semibold text-zinc-900 dark:text-white'}>{value}</span>
          {look !== 'add' && <ChevronDown size={14} aria-hidden className={`shrink-0 text-zinc-500 transition-transform dark:text-zinc-400 ${open ? 'rotate-180' : ''}`} />}
        </>}
      </button>
      {open && place && createPortal(
        <div ref={menu} id={list} role={kind} aria-label={label} tabIndex={-1} onKeyDown={onKeyDown} onBlur={onBlur}
          style={{ left: place.left, top: place.top, bottom: place.bottom, width: place.width, maxHeight: place.height }}
          className="fixed z-[80] overflow-y-auto rounded-xl border border-zinc-200 bg-white p-2 shadow-2xl outline-none dark:border-white/10 dark:bg-zinc-900">
          {children(close)}
        </div>,
        document.body,
      )}
    </div>
  );
}

/** One choice in a pill's list: its name, a line on what it is, a side note (size, "needs a download") and a mark. */
export function PickerOption({ selected, onPick, name, hint, side, mark, disabled, value, action = false }: {
  selected: boolean;
  /** A row that does something (open a page, add) rather than choose a value. */
  action?: boolean;
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
    <button type="button" role={action ? 'menuitem' : 'menuitemradio'} aria-checked={action ? undefined : selected} disabled={disabled} onClick={onPick} data-value={value} tabIndex={-1}
      className={`flex w-full min-w-0 items-start gap-2 rounded-lg px-2 py-2 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-pink-500 disabled:opacity-40 max-md:min-h-11 ${selected ? 'bg-pink-500/10' : 'hover:bg-zinc-100 focus:bg-zinc-100 dark:hover:bg-white/5 dark:focus:bg-white/5'}`}>
      <span className="min-w-0 flex-1">
        <span className={`block truncate text-xs font-semibold ${selected ? 'text-pink-600 dark:text-pink-300' : 'text-zinc-900 dark:text-white'}`}>{name}</span>
        {hint && <span className="mt-0.5 block text-[11px] leading-4 text-zinc-500 dark:text-zinc-400">{hint}</span>}
      </span>
      {side && <span className="shrink-0 text-[10px] font-semibold text-zinc-500 dark:text-zinc-400">{side}</span>}
      {mark && <span className="mt-0.5 shrink-0">{mark}</span>}
    </button>
  );
}
