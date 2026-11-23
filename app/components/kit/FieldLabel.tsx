import React from 'react';

export const FIELD_LABEL = 'mb-1 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400';

/** The small caption over one field inside a card; the card's own title stays the only large heading. */
export function FieldLabel({ children, hidden }: { children: React.ReactNode; hidden?: boolean }) {
  return <span aria-hidden={hidden} className={FIELD_LABEL}>{children}</span>;
}
