import React, { useId, type ReactNode } from 'react';

const TONES = {
  ready: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
  missing: 'bg-zinc-200/70 text-zinc-700 dark:bg-white/10 dark:text-zinc-200',
  busy: 'bg-pink-500/10 text-pink-700 dark:text-pink-300',
  blocked: 'bg-amber-500/10 text-amber-700 dark:text-amber-300',
};

/** The model's state in a tool's header, in words: «Готов», «Нужно скачать · 3,7 ГБ», «Скачивается · 42 %». */
export function ToolState({ tone, children }: { tone: keyof typeof TONES; children: ReactNode }) {
  return <span className={`shrink-0 rounded-full px-2 py-0.5 text-xs font-medium ${TONES[tone]}`}>{children}</span>;
}

/** Why the main action waits, under it: «Выберите трек», «Опишите звук». */
export function ActionReason({ children }: { children: ReactNode }) {
  return <p className="mt-1.5 text-[13px] leading-[18px] text-zinc-600 dark:text-zinc-400">{children}</p>;
}

/**
 * One tool on the Tools page, the same frame for each: header with the model's state, what it makes, its inputs in
 * one column, one action slot (download first, then the action, then its progress), the result, the error of this
 * tool, and one footer line about the model.
 */
export function ToolCard({ icon, title, state, description, children, action, result, error, footer }: {
  icon: ReactNode;
  title: string;
  state?: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
  result?: ReactNode;
  error?: ReactNode;
  footer?: ReactNode;
}) {
  const heading = useId();
  return (
    <section aria-labelledby={heading} className="rounded-2xl border border-zinc-200 bg-zinc-50 p-5 dark:border-white/10 dark:bg-white/3">
      <header className="flex items-center gap-2.5">
        <span className="shrink-0 text-pink-500" aria-hidden>{icon}</span>
        <h2 id={heading} className="min-w-0 flex-1 text-xl leading-7 font-semibold text-zinc-950 dark:text-white">{title}</h2>
        {state}
      </header>
      {description && <p className="mt-1 text-sm leading-[22px] text-zinc-700 dark:text-zinc-300">{description}</p>}
      {children && <div className="mt-4 grid gap-4">{children}</div>}
      {action && <div className="mt-5">{action}</div>}
      {result && <div className="mt-4">{result}</div>}
      {error && <div className="mt-4">{error}</div>}
      {footer && <p className="mt-4 border-t border-zinc-200 pt-3 text-xs text-zinc-600 dark:border-white/10 dark:text-zinc-400">{footer}</p>}
    </section>
  );
}

