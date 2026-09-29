import React from 'react';
import { ArrowDownWideNarrow, ArrowUpNarrowWide } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { SortBy, SortOrder } from '../services/workspaces';

interface SortControlsProps {
    order: SortOrder;
    onOrder: (order: SortOrder) => void;
    /** What the list holds, so the name key says whose name it is: a song's or a session's. */
    what: 'songs' | 'sessions';
}

/**
 * The two controls in front of a list's heading: one word that switches what
 * the list is ordered by (the day it was made, or its name), and one arrow that
 * switches the direction. Both are borderless, like the rest of the strip, and
 * the order they set is the person's - it stays for every list.
 */
export const SortControls: React.FC<SortControlsProps> = ({ order, onOrder, what }) => {
    const { t } = useI18n();
    const button =
        'tap-highlight-none flex items-center gap-1 rounded-[3px] px-1.5 py-0.5 text-[13px] font-medium text-zinc-500 transition-colors hover:bg-zinc-200 hover:text-black dark:text-zinc-400 dark:hover:bg-white/10 dark:hover:text-white';
    const byLabel = order.by === 'created'
        ? t('sortByCreated')
        : order.by === 'updated'
            ? t('sortByUpdated')
            : what === 'songs' ? t('sortTrackName') : t('sortSessionName');
    const directionLabel = order.descending ? t('sortDescending') : t('sortAscending');
    /* One button walks the three ways a list can be read, so the heading never
       grows a row of its own. */
    const next: SortBy = order.by === 'created' ? 'updated' : order.by === 'updated' ? 'name' : 'created';
    return (
        <div className="flex items-center gap-0.5" data-slot="sort-controls">
            <button
                type="button"
                onClick={() => onOrder({ ...order, by: next })}
                title={byLabel}
                className={button}
            >
                {byLabel}
            </button>
            <button
                type="button"
                onClick={() => onOrder({ ...order, descending: !order.descending })}
                title={directionLabel}
                aria-label={directionLabel}
                className={button}
            >
                {order.descending ? <ArrowDownWideNarrow size={15} /> : <ArrowUpNarrowWide size={15} />}
            </button>
        </div>
    );
};
