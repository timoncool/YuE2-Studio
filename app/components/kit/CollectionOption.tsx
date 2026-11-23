import React from 'react';
import { Check, Music, Plus } from 'lucide-react';

/**
 * One playlist or album in every list that sends songs somewhere: the Create button's destination, "Add to playlist"
 * on a song, the album picker. The same row, cover, wording and check mark everywhere.
 */
export function CollectionOption({ name, cover, meta, selected, onPick, value, role = 'menuitemradio', trailing, className = '', ...rest }: {
  name: React.ReactNode;
  cover?: string;
  meta?: React.ReactNode;
  selected?: boolean;
  onPick: () => void;
  value?: string;
  /** `menuitemradio` inside a PickerPill menu, `button` in a dialog's list. */
  role?: 'menuitemradio' | 'button';
  trailing?: React.ReactNode;
} & Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'role' | 'value' | 'onClick'>) {
  return (
    <button type="button" role={role === 'button' ? undefined : role} aria-checked={role === 'button' ? undefined : Boolean(selected)}
      tabIndex={role === 'button' ? undefined : -1} data-value={value} onClick={onPick} {...rest}
      className={`group flex w-full min-w-0 items-center gap-3 rounded-lg px-2 py-2 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-pink-500 ${selected ? 'bg-pink-500/10' : 'hover:bg-zinc-100 focus:bg-zinc-100 dark:hover:bg-white/5 dark:focus:bg-white/5'} ${className}`}>
      <span className="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-md bg-zinc-200 text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
        {cover ? <img src={cover} className="size-full object-cover" alt="" /> : <Music size={18} aria-hidden />}
      </span>
      <span className="min-w-0 flex-1">
        <span className={`block truncate text-sm font-medium ${selected ? 'text-pink-600 dark:text-pink-300' : 'text-zinc-900 dark:text-white'}`}>{name}</span>
        {meta && <span className="block truncate text-xs text-zinc-500 dark:text-zinc-400">{meta}</span>}
      </span>
      {trailing}
      {selected && <Check size={16} className="shrink-0 text-pink-500" aria-hidden />}
    </button>
  );
}

/** The "New playlist" / "New album" row that opens the list, in the same shape as the rows under it. */
export function NewCollectionOption({ label, onPick, role = 'menuitem' }: { label: string; onPick: () => void; role?: 'menuitem' | 'button' }) {
  return (
    <button type="button" role={role === 'button' ? undefined : role} tabIndex={role === 'button' ? undefined : -1} onClick={onPick}
      className="group flex w-full items-center gap-3 rounded-lg px-2 py-2 text-left outline-none transition-colors hover:bg-zinc-100 focus:bg-zinc-100 focus-visible:ring-2 focus-visible:ring-pink-500 dark:hover:bg-white/5 dark:focus:bg-white/5">
      <span className="flex size-10 shrink-0 items-center justify-center rounded-md border border-dashed border-zinc-300 text-zinc-600 group-hover:border-pink-400 group-hover:text-pink-600 dark:border-white/20 dark:text-zinc-300">
        <Plus size={18} aria-hidden />
      </span>
      <span className="text-sm font-semibold text-zinc-800 dark:text-white/90">{label}</span>
    </button>
  );
}
