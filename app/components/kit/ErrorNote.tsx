import React, { useState, type ReactNode } from 'react';
import { Bug, Check, CircleAlert, Copy, Stethoscope, X } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { errorNoteStrings } from '../../i18n/errorNote';
import { useSetupStatus } from '../../services/studioQueries';
import type { ReportFacts } from '../../services/issueReport';
import { ProblemReportDialog } from '../ProblemReportDialog';

/** An error that stays until it is closed: what failed and why (the studio's own words), what to do (a hint where
 * the cause is known, else the diagnostics and a report), and the whole text to copy into a report. */
export function ErrorNote({ text, hint, onClose }: { text: string; hint?: ReactNode; onClose?: () => void }) {
  const { t, language } = useI18n();
  const words = errorNoteStrings[language as keyof typeof errorNoteStrings] ?? errorNoteStrings.en;
  const facts = useSetupStatus<ReportFacts>().data;
  const [copied, setCopied] = useState(false);
  const [reporting, setReporting] = useState(false);
  const copy = () => {
    navigator.clipboard.writeText(text).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1500); }, (e: unknown) => console.error('[ERROR] copy failed:', e));
  };
  const btn = 'inline-flex min-h-6 items-center gap-1 px-2 py-0.5 rounded border border-[var(--color-warn)]/40 text-[11px] hover:bg-[var(--color-warn)]/10';
  return (
    <div role="alert" className="mb-3 px-3 py-2 rounded-lg border border-[var(--color-warn)]/40 bg-[color-mix(in_oklab,var(--color-warn)_6%,transparent)] text-[12px]">
      <div className="flex items-start gap-2">
        <CircleAlert size={14} className="mt-0.5 shrink-0 text-[var(--color-warn)]" />
        <p className="min-w-0 flex-1 break-words whitespace-pre-wrap text-[var(--color-text)]">{text}</p>
        {onClose && <button type="button" onClick={onClose} aria-label={words.close} title={words.close} className="-m-1 inline-flex size-6 shrink-0 items-center justify-center rounded text-[var(--color-muted)] hover:text-[var(--color-text)]"><X size={13} /></button>}
      </div>
      <div className="mt-1.5 ml-6 flex flex-wrap items-center gap-1.5 text-[var(--color-muted)]">
        {hint ? <span className="mr-1">{hint}</span> : <span className="mr-1">{words.whatToDo}</span>}
        <button type="button" onClick={copy} className={btn}>{copied ? <Check size={11} /> : <Copy size={11} />}{copied ? words.copied : words.copy}</button>
        <button type="button" onClick={() => window.dispatchEvent(new CustomEvent('yue:open-settings', { detail: 'engine' }))} className={btn}><Stethoscope size={11} />{words.diagnostics}</button>
        <button type="button" onClick={() => setReporting(true)} className={btn}><Bug size={11} />{t('problemTitle')}</button>
      </div>
      {reporting && <ProblemReportDialog facts={facts} onClose={() => setReporting(false)} />}
    </div>
  );
}
