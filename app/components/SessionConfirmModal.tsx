import React, { useEffect, useState } from 'react';
import { Bot, Check, Clock, FolderOpen, Plus, ShieldCheck, ShieldX, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { WorkspaceSession } from './SessionList';

interface SessionConfirmModalProps {
    /** What an agent asks for and where the studio stands: an open session, or none. */
    request: { sessionId: string; sessionName: string; title?: string } | null;
    /** Every session, for the branch where nothing is open. */
    sessions: WorkspaceSession[];
    /** Allow this one track. */
    onAllowOnce: () => void;
    /** Stop asking inside the open session. */
    onAllowSession: () => void;
    /** Stop asking anywhere. */
    onAllowAlways: () => void;
    /** Ask again every time: remembered answers are forgotten. */
    onAskEveryTime: () => void;
    /** Never allow this again. */
    onDenyAlways: () => void;
    /** Open one of the sessions that is filed away, and let the request go on. */
    onOpenExisting: (sessionId: string) => void;
    /** Make a session with this name, open it, and let the request go on. */
    onCreate: (name: string) => void;
    /** Say no this time, changing nothing. */
    onDecline: () => void;
}

/**
 * Where an agent's track may be made, and how far the answer reaches.
 *
 * The question is put in the window because a track belongs in one session and only
 * the person opens sessions: with one open they answer for it, without one they make a
 * session or bring one back first. The answer carries the same five decisions as every
 * other request from an agent - once, this session, always, ask every time, never - so
 * the person sets the leash from wherever they are asked.
 */
export const SessionConfirmModal: React.FC<SessionConfirmModalProps> = ({
    request,
    sessions,
    onAllowOnce,
    onAllowSession,
    onAllowAlways,
    onAskEveryTime,
    onDenyAlways,
    onOpenExisting,
    onCreate,
    onDecline,
}) => {
    const { t } = useI18n();
    const [name, setName] = useState('');

    useEffect(() => {
        if (!request) {
            setName('');
            return;
        }
        const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') onDecline(); };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [request, onDecline]);

    if (!request) return null;

    const open = request.sessionId !== '';
    const archived = sessions.filter((session) => session.id !== request.sessionId);

    const decision = (icon: React.ReactNode, label: string, onClick: () => void, className: string) => (
        <button
            type="button"
            onClick={onClick}
            className={`flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition-colors ${className}`}
        >
            {icon}
            {label}
        </button>
    );

    return (
        <div className="fixed inset-0 z-[110] flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <div
                className="w-full max-w-md mx-4 rounded-2xl border border-zinc-200 bg-white p-6 shadow-2xl dark:border-white/10 dark:bg-zinc-900"
                data-slot="session-confirm-modal"
            >
                <div className="flex items-start gap-3">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-blue-500/10">
                        <Bot size={20} className="text-blue-500" />
                    </div>
                    <div className="min-w-0 flex-1">
                        <h3 className="text-base font-semibold text-zinc-900 dark:text-white">
                            {t('sessionConfirmTitle')}
                        </h3>
                        <p className="mt-1 text-sm leading-relaxed text-zinc-500 dark:text-zinc-400">
                            {(open ? t('sessionConfirmBody') : t('sessionNeededBody'))
                                .replace('{session}', request.sessionName)
                                .replace('{title}', request.title || t('sessionConfirmTitle'))}
                        </p>
                    </div>
                </div>

                {!open && (
                    <div className="mt-5 space-y-2">
                        <div className="flex gap-2">
                            <input
                                value={name}
                                onChange={(event) => setName(event.target.value)}
                                placeholder={t('sessionNamePlaceholder')}
                                className="min-w-0 flex-1 rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-800 placeholder:text-zinc-400 dark:border-white/10 dark:bg-black/30 dark:text-zinc-100"
                            />
                            <button
                                type="button"
                                disabled={name.trim() === ''}
                                onClick={() => onCreate(name.trim())}
                                className="flex shrink-0 items-center gap-1.5 rounded-lg bg-blue-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-40"
                            >
                                <Plus size={15} />
                                {t('sessionNeededCreate')}
                            </button>
                        </div>
                        {archived.length > 0 && (
                            <div className="max-h-40 space-y-1 overflow-y-auto">
                                {archived.map((session) => (
                                    <button
                                        key={session.id}
                                        type="button"
                                        onClick={() => onOpenExisting(session.id)}
                                        className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm text-zinc-800 transition-colors hover:bg-zinc-100 dark:text-zinc-100 dark:hover:bg-white/10"
                                    >
                                        <FolderOpen size={15} className="shrink-0 text-zinc-400" />
                                        <span className="min-w-0 truncate">{session.name}</span>
                                        <span className="ml-auto shrink-0 text-[10px] uppercase text-zinc-400">
                                            {t('sessionOpen')}
                                        </span>
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>
                )}

                {open && (
                    <div className="mt-5 flex flex-col gap-2">
                        {decision(<Check size={16} />, t('agentConfirmOnce'), onAllowOnce,
                            'bg-blue-600 text-white hover:bg-blue-700')}
                        {decision(<Clock size={16} />, t('agentConfirmSession'), onAllowSession,
                            'bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700')}
                        {decision(<ShieldCheck size={16} />, t('agentConfirmAlways'), onAllowAlways,
                            'bg-emerald-600 text-white hover:bg-emerald-700')}
                    </div>
                )}

                <div className="mt-3 flex flex-col gap-2 border-t border-zinc-200 pt-3 dark:border-white/10">
                    {decision(<X size={16} />, t('agentConfirmAsk'), onAskEveryTime,
                        'bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700')}
                    {decision(<ShieldX size={16} />, t('agentConfirmDenyAlways'), onDenyAlways,
                        'text-zinc-500 hover:bg-zinc-100 hover:text-rose-600 dark:text-zinc-400 dark:hover:bg-white/10 dark:hover:text-rose-400')}
                    {decision(<X size={16} />, t('sessionConfirmDecline'), onDecline,
                        'text-zinc-500 hover:bg-zinc-100 hover:text-rose-600 dark:text-zinc-400 dark:hover:bg-white/10 dark:hover:text-rose-400')}
                </div>
            </div>
        </div>
    );
};
