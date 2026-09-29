import React, { useEffect, useState } from 'react';
import { Bot, Check, LayoutList, Layers, FolderPlus, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/** What the person may answer about where the agent's tracks go. */
export interface SessionChoiceAnswer {
    /** `current`, `each`, `new:<name>` - the words the service understands - or `deny`. */
    choice: string;
    /** Keep the answer for the work at hand, so the next track is not asked about. */
    remember: boolean;
    /** Keep it for good, among the agent settings. */
    always: boolean;
}

export interface SessionChoiceRequest {
    tool: string;
    title: string;
    hasSession: boolean;
    sessionName: string;
    /** Sends the answer back to the studio, which passes it on to the agent. */
    answer: (answer: SessionChoiceAnswer) => void;
}

interface SessionChoiceModalProps {
    request: SessionChoiceRequest | null;
}

/**
 * The agent is about to make tracks and nobody has said where they go. Guessing once put
 * five songs into one session the person never chose - and a closed window meant the track
 * landed in no session at all. So the window asks, the same way it asks about the leash:
 * the open session, a session per song, or one new session for them all, remembered for
 * this work or for good.
 */
export const SessionChoiceModal: React.FC<SessionChoiceModalProps> = ({ request }) => {
    const { t } = useI18n();
    const [newName, setNewName] = useState('');
    const [remember, setRemember] = useState(true);
    const [always, setAlways] = useState(false);

    useEffect(() => {
        if (!request) return;
        setNewName('');
        setRemember(true);
        setAlways(false);
        const onKey = (event: KeyboardEvent) => {
            if (event.key === 'Escape') request.answer({ choice: 'deny', remember: false, always: false });
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [request]);

    if (!request) return null;

    const send = (choice: string) => request.answer({
        choice,
        remember,
        // a session named for this pack is not a standing answer: there is nothing to keep
        always: choice.startsWith('new:') ? false : always,
    });

    const option = (icon: React.ReactNode, label: string, onClick: () => void, className: string, disabled = false) => (
        <button
            type="button"
            onClick={onClick}
            disabled={disabled}
            className={`flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${className}`}
        >
            {icon}
            {label}
        </button>
    );

    return (
        <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <div
                className="w-full max-w-md mx-4 rounded-2xl border border-zinc-200 bg-white p-6 shadow-2xl dark:border-white/10 dark:bg-zinc-900"
                data-slot="session-choice-modal"
            >
                <div className="flex items-start gap-3">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-sky-500/10">
                        <Bot size={20} className="text-sky-500" />
                    </div>
                    <div className="min-w-0 flex-1">
                        <h3 className="text-base font-semibold text-zinc-900 dark:text-white">
                            {t('sessionChoiceTitle')}
                        </h3>
                        <p className="mt-1 text-sm leading-relaxed text-zinc-500 dark:text-zinc-400">
                            {t('sessionChoiceBody').replace('{title}', request.title || request.tool)}
                        </p>
                        {request.hasSession && request.sessionName && (
                            <p className="mt-1 truncate text-xs text-zinc-400 dark:text-zinc-500" data-slot="session-choice-open">
                                {request.sessionName}
                            </p>
                        )}
                    </div>
                    <button
                        type="button"
                        onClick={() => send('deny')}
                        className="rounded-full p-1.5 text-zinc-400 transition-colors hover:bg-zinc-100 hover:text-zinc-600 dark:hover:bg-white/10 dark:hover:text-zinc-200"
                        title={t('sessionChoiceDeny')}
                    >
                        <X size={18} />
                    </button>
                </div>

                <div className="mt-6 flex flex-col gap-2">
                    {option(<Check size={16} />, t('sessionChoiceCurrent'), () => send('current'),
                        'bg-blue-600 text-white hover:bg-blue-700', !request.hasSession)}
                    {option(<LayoutList size={16} />, t('sessionChoiceEach'), () => send('each'),
                        'bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700')}
                    <div className="flex items-center gap-2">
                        <input
                            value={newName}
                            onChange={(event) => setNewName(event.target.value)}
                            placeholder={t('sessionChoiceNewName')}
                            data-slot="session-choice-name"
                            className="min-w-0 flex-1 rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none placeholder:text-zinc-400 focus:border-sky-500 dark:border-white/10 dark:bg-zinc-800 dark:text-white"
                        />
                        {option(<FolderPlus size={16} />, t('sessionChoiceNew'), () => send(`new:${newName.trim()}`),
                            'bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700', newName.trim().length === 0)}
                    </div>
                    {!request.hasSession && (
                        <p className="text-xs text-zinc-400 dark:text-zinc-500">{t('sessionChoiceNoSession')}</p>
                    )}
                </div>

                <div className="mt-6 flex flex-col gap-2 border-t border-zinc-200 pt-4 dark:border-white/10">
                    <label className="flex items-center gap-2 text-sm text-zinc-600 dark:text-zinc-300">
                        <input type="checkbox" checked={remember} onChange={(event) => setRemember(event.target.checked)} />
                        {t('sessionChoiceRemember')}
                    </label>
                    <label className={`flex items-center gap-2 text-sm text-zinc-600 dark:text-zinc-300 ${newName.trim().length === 0 ? '' : 'opacity-60'}`}>
                        <input type="checkbox" checked={always} onChange={(event) => setAlways(event.target.checked)} />
                        {t('sessionChoiceAlways')}
                    </label>
                </div>
            </div>
        </div>
    );
};
