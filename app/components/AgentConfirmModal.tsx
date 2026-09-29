import React, { useEffect } from 'react';
import { Bot, Check, Clock, ShieldX, Trash2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/** What an agent asks to do, and the scope the answer belongs to. */
export interface AgentPermissionRequest {
    action: string;
    scope: string;
    /** A removal - gone for good: one plain question, do it or do not. */
    removal?: boolean;
    /** What the removal is about, in the person's words: the name of the thing. */
    target?: string;
    details?: Record<string, unknown>;
    /** Sends the answer back to the studio, which passes it on to the agent. */
    answer: (decision: AgentPermissionDecision) => void;
}

export type AgentPermissionDecision = 'once' | 'session' | 'always' | 'ask' | 'deny_always' | 'deny';

interface AgentConfirmModalProps {
    request: AgentPermissionRequest | null;
}

/**
 * The agent's request to do something that cannot be undone. The window is the
 * only place that may answer, and it answers for the future too: once for this
 * request, this session, always from anywhere - or never, remembered as firmly
 * as a yes. "Ask every time" keeps asking, which is the default the studio
 * starts from.
 */
export const AgentConfirmModal: React.FC<AgentConfirmModalProps> = ({ request }) => {
    const { t } = useI18n();

    useEffect(() => {
        if (!request) return;
        const onKey = (event: KeyboardEvent) => {
            if (event.key !== 'Escape') return;
            // A removal has one answer to give back: no.
            request.answer(request.removal ? 'deny' : 'ask');
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [request]);

    if (!request) return null;

    const option = (
        decision: AgentPermissionDecision,
        icon: React.ReactNode,
        label: string,
        className: string,
    ) => (
        <button
            key={decision}
            type="button"
            onClick={() => request.answer(decision)}
            className={`flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition-colors ${className}`}
        >
            {icon}
            {label}
        </button>
    );

    return (
        <div className="fixed inset-0 z-[120] flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <div
                className="w-full max-w-md mx-4 rounded-2xl border border-zinc-200 bg-white p-6 shadow-2xl dark:border-white/10 dark:bg-zinc-900"
                data-slot="agent-confirm-modal"
            >
                <div className="flex items-start gap-3">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-amber-500/10">
                        <Bot size={20} className="text-amber-500" />
                    </div>
                    <div className="min-w-0 flex-1">
                        <h3 className="text-base font-semibold text-zinc-900 dark:text-white">
                            {request.removal
                                ? request.target
                                    ? t('agentConfirmRemoveTitle').replace('{target}', request.target)
                                    : t('agentConfirmRemoveTitlePlain')
                                : t('agentConfirmTitle')}
                        </h3>
                        <p className="mt-1 text-sm leading-relaxed text-zinc-500 dark:text-zinc-400">
                            {request.removal ? t('agentConfirmRemoveBody') : t('agentConfirmBody').replace('{action}', request.action)}
                        </p>
                    </div>
                </div>

                <div className="mt-6 flex flex-col gap-2">
                    {request.removal ? (
                        <>
                            {/* Something gone for good: one plain question, do it or do not. */}
                            {option('once', <Trash2 size={16} />, t('agentConfirmRemove'),
                                'bg-rose-600 text-white hover:bg-rose-700')}
                            {option('deny', <X size={16} />, t('agentConfirmCancel'),
                                'bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700')}
                        </>
                    ) : (
                        <>
                            {option('once', <Check size={16} />, t('agentConfirmOnce'),
                                'bg-blue-600 text-white hover:bg-blue-700')}
                            {option('session', <Clock size={16} />, t('agentConfirmSession'),
                                'bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700')}
                            {option('ask', <X size={16} />, t('agentConfirmAsk'),
                                'bg-zinc-100 text-zinc-700 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700')}
                            {option('deny_always', <ShieldX size={16} />, t('agentConfirmDenyAlways'),
                                'text-zinc-500 hover:bg-zinc-100 hover:text-rose-600 dark:text-zinc-400 dark:hover:bg-white/10 dark:hover:text-rose-400')}
                        </>
                    )}
                </div>
            </div>
        </div>
    );
};
