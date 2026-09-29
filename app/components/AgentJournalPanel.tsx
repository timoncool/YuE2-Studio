import React, { useEffect, useState } from 'react';
import { Bot, MonitorSpeaker, Trash2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { ToastType } from './Toast';

/**
 * The message log, opened by the bell in the control strip.
 *
 * The strip dispatches `yue:journal-toggle`; this panel listens for it and shows
 * everything the window reported, newest first. Every message the toast shows
 * lands here too, so one that flashed past is still readable - whichever way it
 * arrives: an action inside the window, or an agent working the window over MCP
 * through the `notify` bridge. Each entry keeps its source, so the two are told
 * apart at a glance: a robot for the agent, a speaker for the studio.
 *
 * The history lives in the window's own state, as everything the studio shows
 * does; nothing is read from disk and the desktop build stays self-contained.
 *
 * Styled like the studio's side panels (`RightSidebar`): suno-panel background,
 * a left border, a scrolling list, and both themes honoured.
 *
 * A hello from RafStudio: built with love and appreciation for your studio, and meant to be
 * worth your review.
 */
export type NoticeSource = 'studio' | 'agent';

export interface AgentNotice {
    /** Stable key for the list. */
    id: number;
    text: string;
    tone: ToastType;
    /** Who reported it: the studio itself, or an agent working the window. */
    source: NoticeSource;
    /** When it was reported, in milliseconds. */
    at: number;
}

/** The tone colours the entry; the source gives the icon its shape. */
const TONE_CLASS: Record<ToastType, string> = {
    info: 'text-violet-500',
    success: 'text-emerald-500',
    error: 'text-rose-500',
};

const SOURCE_ICON: Record<NoticeSource, React.ReactNode> = {
    studio: <MonitorSpeaker size={15} />,
    agent: <Bot size={15} />,
};

interface AgentJournalPanelProps {
    entries: AgentNotice[];
    /** Called with the id of the entry whose own cross was clicked. */
    onRemove: (id: number) => void;
    /** Called when the person asks for a clean log. The window asks first: the past cannot be
     * brought back, so the decision is theirs, not the panel's. */
    onClearRequest: () => void;
}

export const AgentJournalPanel: React.FC<AgentJournalPanelProps> = ({ entries, onRemove, onClearRequest }) => {
    const { t } = useI18n();
    const [isOpen, setIsOpen] = useState(false);

    // The bell toggles the log; Escape closes it.
    useEffect(() => {
        const onToggle = () => setIsOpen((open) => !open);
        const onKey = (event: KeyboardEvent) => {
            if (event.key === 'Escape') setIsOpen(false);
        };
        window.addEventListener('yue:journal-toggle', onToggle);
        window.addEventListener('keydown', onKey);
        return () => {
            window.removeEventListener('yue:journal-toggle', onToggle);
            window.removeEventListener('keydown', onKey);
        };
    }, []);

    if (!isOpen) return null;

    // Newest first: what was just reported is what the user is looking for.
    const newestFirst = [...entries].reverse();

    return (
        <div
            className="absolute inset-y-0 right-0 z-40 flex w-[min(26rem,100%)] min-w-0 flex-col border-l border-zinc-200 bg-zinc-50 transition-colors duration-300 dark:border-white/10 dark:bg-suno-panel"
            data-slot="agent-journal-panel"
        >
            {/* The title bar matches the song-details panel's own header (h-14,
                px-4, the same blured bar) so the two right-hand panels line up */}
            <div className="h-14 flex items-center gap-2 px-4 border-b border-zinc-200 dark:border-white/5 flex-shrink-0 bg-zinc-50/50 dark:bg-suno-panel/50 backdrop-blur-md z-10">
                <span className="min-w-0 truncate font-semibold text-sm text-zinc-900 dark:text-white">
                    {t('controlPanelMessages')}
                </span>
                <span className="shrink-0 text-xs text-zinc-500 dark:text-zinc-400">{entries.length}</span>
                {entries.length > 0 && (
                    <button
                        type="button"
                        onClick={onClearRequest}
                        title={t('controlPanelMessagesClear')}
                        className="p-1.5 hover:bg-zinc-200 dark:hover:bg-white/10 rounded-full text-zinc-500 dark:text-zinc-400 transition-colors ml-auto shrink-0"
                    >
                        <Trash2 size={16} />
                    </button>
                )}
                <button
                    type="button"
                    onClick={() => setIsOpen(false)}
                    title={t('close')}
                    className={`p-1.5 hover:bg-zinc-200 dark:hover:bg-white/10 rounded-full text-zinc-500 dark:text-zinc-400 transition-colors shrink-0 ${entries.length > 0 ? '' : 'ml-auto'}`}
                >
                    <X size={18} />
                </button>
            </div>

            <div className="custom-scrollbar min-h-0 flex-1 overflow-y-auto px-3 py-2">
                {entries.length === 0 && (
                    <p className="py-2 text-xs text-zinc-500 dark:text-zinc-400">
                        {t('controlPanelMessagesEmpty')}
                    </p>
                )}
                <ul className="space-y-2">
                    {newestFirst.map((entry) => (
                        <li key={entry.id} className="group flex items-start gap-2">
                            <span
                                className={`mt-0.5 shrink-0 ${TONE_CLASS[entry.tone] ?? TONE_CLASS.info}`}
                                title={entry.source === 'agent'
                                    ? t('controlPanelSourceAgent')
                                    : t('controlPanelSourceStudio')}
                            >
                                {SOURCE_ICON[entry.source] ?? SOURCE_ICON.studio}
                            </span>
                            <span className="min-w-0 flex-1">
                                <span className="block text-sm leading-snug text-zinc-800 dark:text-zinc-100">
                                    {entry.text}
                                </span>
                                <span className="block text-[11px] text-zinc-500 dark:text-zinc-500">
                                    {new Date(entry.at).toLocaleTimeString()}
                                </span>
                            </span>
                            <button
                                type="button"
                                onClick={() => onRemove(entry.id)}
                                title={t('delete')}
                                className="tap-highlight-none -mr-1 mt-0.5 shrink-0 rounded-md p-1 text-zinc-400 transition-colors hover:bg-zinc-200 hover:text-rose-600 focus-visible:bg-zinc-200 focus-visible:text-rose-600 dark:text-zinc-500 dark:hover:bg-white/10 dark:hover:text-rose-400 dark:focus-visible:bg-white/10 dark:focus-visible:text-rose-400"
                            >
                                <X size={13} />
                            </button>
                        </li>
                    ))}
                </ul>
            </div>
        </div>
    );
};
