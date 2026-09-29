import React, { useEffect, useRef, useState } from 'react';
import { Plus } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

interface SessionCreateModalProps {
    isOpen: boolean;
    /** A session always has a name - that is what tells two apart. */
    onCreate: (name: string) => void;
    onDismiss: () => void;
}

/**
 * Creating a session is the one place a modal earns its keep: it asks for the
 * name and gets out of the way. The browser itself is a list, not this.
 */
export const SessionCreateModal: React.FC<SessionCreateModalProps> = ({ isOpen, onCreate, onDismiss }) => {
    const { t } = useI18n();
    const [name, setName] = useState('');
    const inputRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        if (!isOpen) {
            setName('');
            return;
        }
        inputRef.current?.focus();
        const onKey = (event: KeyboardEvent) => {
            if (event.key === 'Escape') onDismiss();
        };
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [isOpen, onDismiss]);

    if (!isOpen) return null;

    const create = () => {
        const value = name.trim();
        if (!value) return;
        onCreate(value);
    };

    return (
        <div
            className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm"
            onClick={(event) => { if (event.target === event.currentTarget) onDismiss(); }}
        >
            <div
                className="w-full max-w-sm mx-4 rounded-2xl border border-zinc-200 bg-white p-6 shadow-2xl dark:border-white/10 dark:bg-zinc-900"
                data-slot="session-create-modal"
            >
                <h3 className="text-base font-semibold text-zinc-900 dark:text-white">
                    {t('sessionCreate')}
                </h3>
                <input
                    ref={inputRef}
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    onKeyDown={(event) => { if (event.key === 'Enter') create(); }}
                    placeholder={t('sessionNamePlaceholder')}
                    className="mt-4 w-full rounded-lg border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-900 placeholder-zinc-400 focus:border-zinc-400 focus:outline-none dark:border-white/10 dark:bg-black/40 dark:text-white dark:placeholder-zinc-600"
                />
                <div className="mt-6 flex justify-end gap-3">
                    <button
                        type="button"
                        onClick={onDismiss}
                        className="rounded-lg bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-700 transition-colors hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:hover:bg-zinc-700"
                    >
                        {t('cancel')}
                    </button>
                    <button
                        type="button"
                        onClick={create}
                        disabled={!name.trim()}
                        className="flex items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                        <Plus size={15} />
                        {t('create')}
                    </button>
                </div>
            </div>
        </div>
    );
};
