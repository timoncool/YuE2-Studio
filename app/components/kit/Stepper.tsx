import React from 'react';
import { Minus, Plus } from 'lucide-react';

/** A small whole number moved one step at a time: transposition, a capo fret. Arrow keys work on the value. */
export function Stepper({ value, min, max, label, onChange, format = String }: {
  value: number;
  min: number;
  max: number;
  label: string;
  onChange: (value: number) => void;
  format?: (value: number) => string;
}) {
  const set = (next: number) => onChange(Math.min(max, Math.max(min, next)));
  const button = 'inline-flex size-9 items-center justify-center text-zinc-600 transition-colors hover:bg-zinc-100 hover:text-pink-600 disabled:opacity-40 dark:text-zinc-300 dark:hover:bg-white/10';
  return (
    <div role="group" aria-label={label} className="inline-flex items-center overflow-hidden rounded-lg border border-zinc-200 dark:border-white/10">
      <button type="button" className={button} aria-label={`${label}: −1`} disabled={value <= min} onClick={() => set(value - 1)}><Minus size={14} aria-hidden /></button>
      <span role="spinbutton" tabIndex={0} aria-label={label} aria-valuenow={value} aria-valuemin={min} aria-valuemax={max} aria-valuetext={format(value)}
        onKeyDown={event => {
          if (event.key === 'ArrowUp' || event.key === 'ArrowRight') { event.preventDefault(); set(value + 1); }
          if (event.key === 'ArrowDown' || event.key === 'ArrowLeft') { event.preventDefault(); set(value - 1); }
        }}
        className="min-w-10 px-1 text-center text-sm font-semibold tabular-nums text-zinc-900 outline-none focus-visible:ring-2 focus-visible:ring-pink-500 dark:text-white">
        {format(value)}
      </span>
      <button type="button" className={button} aria-label={`${label}: +1`} disabled={value >= max} onClick={() => set(value + 1)}><Plus size={14} aria-hidden /></button>
    </div>
  );
}
