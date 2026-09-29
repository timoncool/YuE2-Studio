import React from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * The pager under a long list: how many items fit on a page is a setting of the person
 * (Settings - Appearance), so every list that can grow shares this one leaf.
 */
interface PagerProps {
  page: number;
  pageCount: number;
  onPage: (page: number) => void;
}

export const Pager: React.FC<PagerProps> = ({ page, pageCount, onPage }) => {
  const { t } = useI18n();
  if (pageCount <= 1) return null;
  const button =
    'p-1.5 rounded-md border border-zinc-300 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 transition-colors hover:bg-zinc-100 dark:hover:bg-white/5 disabled:opacity-40 disabled:hover:bg-transparent';
  return (
    /* Centred between its neighbours: the tab strip above carries its own margin, so the
       same space is left below - the buttons hang between the two, not off either. */
    <div className="mb-6 flex items-center justify-center gap-2 select-none">
      <button type="button" className={button} disabled={page <= 0} onClick={() => onPage(page - 1)} title={t('previous')}>
        <ChevronLeft size={16} />
      </button>
      <span className="text-xs text-zinc-500 dark:text-zinc-400 tabular-nums">
        {t('pageOf').replace('{page}', String(page + 1)).replace('{total}', String(pageCount))}
      </span>
      <button type="button" className={button} disabled={page >= pageCount - 1} onClick={() => onPage(page + 1)} title={t('next')}>
        <ChevronRight size={16} />
      </button>
    </div>
  );
};
