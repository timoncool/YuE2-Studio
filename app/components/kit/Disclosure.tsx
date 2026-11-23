import React, { type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';

/** A part folded under its title: a turning arrow, the title, and what opens under it. One look for every fold. */
export function Disclosure({ title, children, open, className = '' }: { title: ReactNode; children: ReactNode; open?: boolean; className?: string }) {
  return (
    <details open={open} className={`group ${className}`}>
      <summary className="flex min-h-6 cursor-pointer list-none items-center gap-1.5 text-sm font-semibold text-zinc-800 hover:text-zinc-950 dark:text-zinc-200 dark:hover:text-white [&::-webkit-details-marker]:hidden">
        <ChevronRight size={15} aria-hidden className="shrink-0 transition-transform group-open:rotate-90" />{title}
      </summary>
      <div className="mt-3">{children}</div>
    </details>
  );
}
