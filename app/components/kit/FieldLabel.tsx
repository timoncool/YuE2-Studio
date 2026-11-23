import React from 'react';

export const FIELD_LABEL = 'mb-1.5 block text-[13px] leading-[18px] font-semibold text-zinc-900 dark:text-zinc-100';
export const FIELD_HINT = 'mb-1.5 -mt-1 block text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400';

/** The caption over one field: sentence case, a step above the field's hint and a step below its text. */
export function FieldLabel({ children, hidden }: { children: React.ReactNode; hidden?: boolean }) {
  return <span aria-hidden={hidden} className={FIELD_LABEL}>{children}</span>;
}

/** What a field takes, between its caption and the field. */
export function FieldHint({ children }: { children: React.ReactNode }) {
  return <span className={FIELD_HINT}>{children}</span>;
}
