import React from 'react';
import { Bell, Disc3, Layers, Library, RefreshCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { SortControls } from './SortControls';
import type { SortOrder } from '../services/workspaces';

/**
 * The control strip above the track list: the sessions button, the view switch, the sort
 * control, the refresh button, and the bell that opens the message log.
 *
 * A hello from RafStudio: glad to have worked alongside you, Timon - this fork is made with
 * love and appreciation for the studio you built, and every line of it is meant to be worth
 * your review.
 */
/** What the song list shows: the open session's tracks, or the whole library. */
export type SessionView = 'session' | 'library';

interface TopControlPanelProps {
    view: SessionView;
    onViewChange: (view: SessionView) => void;
    /** True while the session browser fills the list column. */
    sessionsOpen: boolean;
    /** Opens the session browser, or returns to the tracks. */
    onToggleSessions: () => void;
    /** Reads the lists again: what changed elsewhere is not always announced to us. */
    onRefresh: () => void;
    /** How the list below is read, and what it holds - so the name key says whose name it is. */
    order: SortOrder;
    onOrder: (order: SortOrder) => void;
    what: 'songs' | 'sessions';
}

/**
 * ALPHA - the control strip above the song list.
 *
 * It spans the whole SongList column, flush with its top edge, and is styled
 * like a ToolStrip: a bar a shade lighter than the window behind it (the list
 * is pure black, the bar is suno-card #18181b), with a bottom border and a
 * hover highlight on every item.
 *
 * It carries the session list button, the switcher between the open session and
 * the whole library, and the bell that toggles the
 * agent message log on the right.
 */
export const TopControlPanel: React.FC<TopControlPanelProps> = ({
    view,
    onViewChange,
    sessionsOpen,
    onToggleSessions,
    onRefresh,
    order,
    onOrder,
    what,
}) => {
    const { t } = useI18n();

    // The bell toggles the log; AgentJournalPanel listens for this same event.
    const toggleJournal = () => {
        window.dispatchEvent(new CustomEvent('yue:journal-toggle'));
    };

    // A ToolStrip item: flat, highlighted on hover.
    const itemClass =
        'tap-highlight-none flex items-center justify-center rounded-[3px] p-1.5 text-zinc-600 transition-colors hover:bg-zinc-200 hover:text-black dark:text-zinc-300 dark:hover:bg-white/10 dark:hover:text-white';

    // One half of the switcher; the chosen half reads as pressed.
    const segmentClass = (isActive: boolean) =>
        `tap-highlight-none flex items-center gap-1.5 rounded-[2px] px-2 py-1 text-[13px] font-medium transition-colors ${
            isActive
                ? 'bg-white text-zinc-900 shadow-sm dark:bg-white/15 dark:text-white'
                : 'text-zinc-500 hover:text-black dark:text-zinc-400 dark:hover:text-white'
        }`;

    return (
        <div
            className="flex w-full items-center gap-0.5 border-b border-zinc-300 bg-zinc-100 px-2 py-1 transition-colors duration-300 dark:border-[#2f2f33] dark:bg-[#18181b]"
            data-slot="top-control-panel"
            role="toolbar"
            aria-label={t('controlPanel')}
        >
            {/* Reading again by hand: changes made in another window, or while this
                one slept, are not always announced to it. */}
            <button
                type="button"
                onClick={onRefresh}
                title={t('controlPanelRefresh')}
                aria-label={t('controlPanelRefresh')}
                className={itemClass}
            >
                <RefreshCw size={17} />
            </button>

            <button
                type="button"
                onClick={onToggleSessions}
                title={t('controlPanelSessions')}
                aria-pressed={sessionsOpen}
                className={sessionsOpen
                    ? 'tap-highlight-none flex items-center justify-center rounded-[3px] bg-white p-1.5 text-zinc-900 shadow-sm transition-colors dark:bg-white/15 dark:text-white'
                    : itemClass}
            >
                <Layers size={19} />
            </button>

            <span className="mx-1 h-5 w-px shrink-0 bg-zinc-300 dark:bg-white/10" aria-hidden="true" />

            {/* How the list below is read: what it is ordered by, and which way. */}
            <SortControls order={order} onOrder={onOrder} what={what} />

            <span className="mx-1 h-5 w-px shrink-0 bg-zinc-300 dark:bg-white/10" aria-hidden="true" />

            <div
                className="flex items-center gap-0.5 rounded-[3px] bg-zinc-200/70 p-0.5 dark:bg-black/30"
                role="group"
                aria-label={t('controlPanelSessions')}
            >
                <button
                    type="button"
                    onClick={() => onViewChange('session')}
                    title={t('controlPanelSessionView')}
                    aria-pressed={view === 'session'}
                    className={segmentClass(view === 'session')}
                >
                    <Disc3 size={15} />
                    <span className="hidden sm:inline">{t('controlPanelSessionView')}</span>
                </button>
                <button
                    type="button"
                    onClick={() => onViewChange('library')}
                    title={t('library')}
                    aria-pressed={view === 'library'}
                    className={segmentClass(view === 'library')}
                >
                    <Library size={15} />
                    <span className="hidden sm:inline">{t('library')}</span>
                </button>
            </div>


            <span className="ml-auto" />

            <span className="mx-1 h-5 w-px shrink-0 bg-zinc-300 dark:bg-white/10" aria-hidden="true" />

            <button
                type="button"
                onClick={toggleJournal}
                title={t('controlPanelMessages')}
                className={itemClass}
            >
                <Bell size={19} />
            </button>
        </div>
    );
};
