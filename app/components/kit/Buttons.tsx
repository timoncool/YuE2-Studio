import React from 'react';

/** The studio's main action: one per card, the step the person came for. */
export const PRIMARY_BUTTON =
  'inline-flex items-center justify-center gap-2 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:cursor-not-allowed disabled:opacity-50';

/** A second action beside the main one: cancel, back, retry. */
export const SECONDARY_BUTTON =
  'inline-flex items-center justify-center gap-1.5 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/15 dark:text-zinc-200';

type ButtonProps = React.ButtonHTMLAttributes<HTMLButtonElement>;

export function PrimaryButton({ className = '', type = 'button', ...props }: ButtonProps) {
  return <button type={type} className={`${PRIMARY_BUTTON} ${className}`.trim()} {...props} />;
}

export function SecondaryButton({ className = '', type = 'button', ...props }: ButtonProps) {
  return <button type={type} className={`${SECONDARY_BUTTON} ${className}`.trim()} {...props} />;
}
