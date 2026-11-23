import React from 'react';
import Combobox, { type ComboOption } from './Combobox';

export type SelectOption = Omit<ComboOption, 'search'>;

/**
 * A choice from a short fixed list, drawn like every other list of the studio: the same field, the same list under
 * it, arrows and Enter to choose, Escape to close. Long lists the person searches in are a Combobox.
 */
export function Select<T extends string>({ value, options, onChange, label, className, inputClassName, disabled, placeholder }: {
  value: T;
  options: (SelectOption & { value: T })[];
  onChange: (value: T) => void;
  label: string;
  className?: string;
  inputClassName?: string;
  disabled?: boolean;
  placeholder?: string;
}) {
  return <Combobox search={false} label={label} value={value} options={options} onChange={next => onChange(next as T)}
    className={className} disabled={disabled} placeholder={placeholder}
    inputClassName={inputClassName ?? SELECT_FIELD} />;
}

export const SELECT_FIELD =
  'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/25 dark:text-white';
