import React, { useEffect } from 'react';
import { Check, Pencil, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

interface EditModeModalProps {
    /** The session just opened from the library page, or null when nothing is being asked. */
    offer: { sessionName: string } | null;
    /** Yes: leave the library behind and bring up the page that carries the alpha panel. */
    onAccept: () => void;
    /** No: the person keeps working where they are. */
    onDecline: () => void;
}

/**
 * Opening a session from the library page leaves the person on a list of sessions: what
 * their click did is not in front of them. The window asks whether to step over to the
 * page where the session is worked on - the one the alpha panel belongs to.
 */
export const EditModeModal: React.FC<EditModeModalProps> = ({ offer, onAccept, onDecline }) => {
    const { t } = useI18n();

    useEffect(() => {
        if (!offer) return;
        const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') onDecline(); };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [offer, onDecline]);

    if (!offer) return null;

    return (
        <div className="fixed inset-0 z-[110] flex items-center justify-center bg-black/60 backdrop-blur-sm">
            <div
                className="w-full max-w-sm mx-4 rounded-2xl border border-zinc-200 bg-white p-6 shadow-2xl dark:border-white/10 dark:bg-zinc-900"
                data-slot="edit-mode-modal"
            >
                <div className="flex items-start gap-3">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-emerald-500/10">
                        <Pencil size={20} className="text-emerald-500" />
                    </div>
                    <div className="min-w-0 flex-1">
                        <h3 className="text-base font-semibold text-zinc-900 dark:text-white">{t('editModeAsk')}</h3>
                        <p className="mt-1 text-sm leading-relaxed text-zinc-500 dark:text-zinc-400">
                            {t('editModeOpen').replace('{session}', offer.sessionName)}
                        </p>
                    </div>
                </div>
                <div className="mt-5 flex gap-2">
                    <button
                        type="button"
                        onClick={onAccept}
                        className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700"
                    >
                        <Check size={16} />
                        {t('editModeYes')}
                    </button>
                    <button
                        type="button"
                        onClick={onDecline}
                        className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-700 transition-colors hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700"
                    >
                        <X size={16} />
                        {t('editModeNo')}
                    </button>
                </div>
            </div>
        </div>
    );
};
