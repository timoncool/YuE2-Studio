import React, { useEffect, useMemo, useState } from 'react';
import { Archive, FolderOpen, Pencil, Plus, Search } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { orderSessions, type SortOrder } from '../services/workspaces';
import { stampOf, changedAfter } from '../services/dates';
import { Pager } from './Pager';

/**
 * A workspace session: the working set the studio collects everything into.
 *
 * One session is open at a time - the current one. Closing a session files it
 * away; opening it again makes it current. Tracks never move between files: a
 * session is a mark on them, which is why closing one is instant and lossless.
 */
export interface WorkspaceSession {
    id: string;
    name: string;
    /** `import` for the session the studio keeps itself: it is shown by its mark, not its name. */
    kind?: string | null;
    createdAt: number;
    /** When it was last touched: renamed, opened, closed. Falls back to createdAt. */
    updatedAt?: number;
    /** null while the session is open; the moment it was closed otherwise. */
    closedAt: number | null;
}

interface SessionListProps {
    sessions: WorkspaceSession[];
    activeSessionId: string | null;
    /** How many tracks a session holds, for the row's subtitle. */
    trackCount: (sessionId: string) => number;
    /** Makes a session current. */
    onOpenSession: (id: string) => void;
    /** Files the session away; only the current one can be closed. */
    onCloseSession: (id: string) => void;
    onRenameSession: (id: string, name: string) => void;
    /** Creating one asks for a name in a modal, so it is asked here. */
    onCreateRequest: () => void;
    /** How many rows one page holds: the person's setting, shared with every other list. */
    itemsPerPage: number;
    /** What the sessions are ordered by, and which way - set in the alpha panel above. */
    order: SortOrder;
}

/**
 * The session browser - built like the song list rather than a modal, so it can
 * grow the same way: a search field, a list of rows, and room for paging and
 * filters later. A million sessions would scroll; a million modal rows would not.
 */
export const SessionList: React.FC<SessionListProps> = ({
    sessions,
    activeSessionId,
    trackCount,
    onOpenSession,
    onCloseSession,
    onRenameSession,
    onCreateRequest,
    itemsPerPage,
    order,
}) => {
    const { t, language, songCount } = useI18n();
    const [query, setQuery] = useState('');
    const [renamingId, setRenamingId] = useState<string | null>(null);
    const [renamingValue, setRenamingValue] = useState('');
    /* A list is read a page at a time, like the songs: a new search or a new page
       size starts from the first page. */
    const [page, setPage] = useState(0);

    const visible = useMemo(() => {
        const needle = query.trim().toLowerCase();
        const titleOf = (session: WorkspaceSession) => (session.kind === 'import' ? t('sessionImportName') : session.name);
        const matching = sessions.filter((session) => !needle || titleOf(session).toLowerCase().includes(needle));
        // The current session leads the list: it is the one being worked in.
        return orderSessions(matching, activeSessionId, order);
    }, [sessions, query, activeSessionId, order, t]);

    useEffect(() => { setPage(0); }, [query, sessions.length, itemsPerPage]);

    const commitRename = (id: string) => {
        const name = renamingValue.trim();
        if (name) onRenameSession(id, name);
        setRenamingId(null);
    };

    return (
        <div
            className="h-full min-w-0 flex-1 overflow-y-auto bg-white p-4 pb-32 transition-colors duration-300 dark:bg-black sm:p-6"
            data-slot="session-list"
        >
            {/* Same reading column as the song list: capped for readable lines,
                flush left so both lists start at the same edge. */}
            <div className="w-full min-w-0 max-w-5xl">
                <div className="mb-8 flex flex-col gap-6">
                    <div className="flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
                        {/* The browser gets its own colour, so a glance says which list this is. */}
                        <span className="font-medium text-emerald-600 dark:text-emerald-400">{t('controlPanelSessions')}</span>
                        <span className="text-zinc-400 dark:text-zinc-600">·</span>
                        <span>{sessions.length}</span>
                    </div>

                    <div className="flex items-center gap-3">
                        <div className="relative group flex-1">
                            <input
                                type="text"
                                value={query}
                                onChange={(event) => setQuery(event.target.value)}
                                placeholder={t('sessionSearchPlaceholder')}
                                className="w-full bg-zinc-100 dark:bg-[#121214] border border-zinc-200 dark:border-white/10 rounded-lg pl-10 pr-4 py-2.5 text-sm text-zinc-900 dark:text-white focus:outline-none focus:border-zinc-400 dark:focus:border-white/20 placeholder-zinc-500 dark:placeholder-zinc-600 transition-colors"
                            />
                            <Search className="w-4 h-4 text-zinc-500 absolute left-3 top-3" />
                        </div>

                        <button
                            type="button"
                            onClick={onCreateRequest}
                            className="flex shrink-0 items-center gap-1.5 rounded-lg bg-blue-600 px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-blue-700"
                        >
                            <Plus size={16} />
                            {t('sessionCreate')}
                        </button>
                    </div>
                </div>

                {/* The pager is read before the rows, not after them. */}
                <Pager
                    page={page}
                    pageCount={Math.max(1, Math.ceil(visible.length / itemsPerPage))}
                    onPage={setPage}
                />

                <div className="space-y-2">
                    {visible.length === 0 ? (
                        <div className="flex h-64 flex-col items-center justify-center space-y-4 rounded-2xl border border-dashed border-zinc-200 bg-zinc-50 text-zinc-500 dark:border-white/5 dark:bg-white/[0.02]">
                            <div className="flex h-16 w-16 items-center justify-center rounded-full bg-zinc-100 dark:bg-white/5">
                                <Search size={30} />
                            </div>
                            <p className="font-medium">{t('sessionNoMatch')}</p>
                        </div>
                    ) : (
                        visible.slice(page * itemsPerPage, (page + 1) * itemsPerPage).map((session) => {
                            const isCurrent = session.id === activeSessionId;
                            const isOpenSession = session.closedAt === null;
                            return (
                                <div
                                    key={session.id}
                                    /* Same mark songs carry, so an agent reading the window
                                       sees the session by name and id, not by its place. */
                                    data-mcp-context={`session ${session.id}: ${session.kind === 'import' ? t('sessionImportName') : session.name}`}
                                    className={`flex items-center gap-3 rounded-lg border px-3 py-2.5 transition-colors ${
                                        isCurrent
                                            ? 'border-emerald-500/40 bg-emerald-500/5 dark:border-emerald-500/30'
                                            : 'border-zinc-200 bg-zinc-50 dark:border-white/10 dark:bg-white/[0.02]'
                                    }`}
                                >
                                    <div className="min-w-0 flex-1">
                                        {renamingId === session.id ? (
                                            <input
                                                autoFocus
                                                value={renamingValue}
                                                onChange={(event) => setRenamingValue(event.target.value)}
                                                onKeyDown={(event) => {
                                                    if (event.key === 'Enter') commitRename(session.id);
                                                    if (event.key === 'Escape') setRenamingId(null);
                                                }}
                                                onBlur={() => commitRename(session.id)}
                                                className="w-full rounded-md border border-zinc-300 bg-white px-2 py-1 text-sm text-zinc-900 focus:outline-none dark:border-white/10 dark:bg-black/40 dark:text-white"
                                            />
                                        ) : (
                                            <div className="flex items-center gap-2">
                                                <span className="min-w-0 truncate text-sm font-medium text-zinc-900 dark:text-white">
                                                    {session.kind === 'import' ? t('sessionImportName') : session.name}
                                                </span>
                                                {isCurrent && (
                                                    <span className="shrink-0 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[10px] font-semibold text-emerald-600 dark:text-emerald-400">
                                                        {t('sessionActive')}
                                                    </span>
                                                )}
                                                {!isCurrent && isOpenSession && (
                                                    <span className="shrink-0 rounded-full bg-zinc-500/15 px-2 py-0.5 text-[10px] font-semibold text-zinc-600 dark:text-zinc-300">
                                                        {t('sessionOpenState')}
                                                    </span>
                                                )}
                                            </div>
                                        )}
                                        <span className="block text-[11px] text-zinc-500 dark:text-zinc-500">
                                            {songCount(trackCount(session.id))}
                                            {' · '}
                                            {/* The label is plain and the moment carries the colour:
                                                a made moment is blue, a changed one is orange. */}
                                            <span className="text-zinc-500 dark:text-zinc-500">{t('sessionMadeLabel')}</span>{' '}
                                            <span className="font-semibold text-blue-600 dark:text-blue-400">
                                                {stampOf(session.createdAt, language)}
                                            </span>
                                            {changedAfter(session.createdAt, session.updatedAt) && (
                                                <>
                                                    {' · '}
                                                    <span className="text-zinc-500 dark:text-zinc-500">{t('changedLabel')}</span>{' '}
                                                    <span className="font-semibold text-orange-600 dark:text-orange-400">
                                                        {stampOf(session.updatedAt, language)}
                                                    </span>
                                                </>
                                            )}
                                        </span>
                                    </div>

                                    <button
                                        type="button"
                                        onClick={() => {
                                            setRenamingId(session.id);
                                            setRenamingValue(session.name);
                                        }}
                                        title={t('sessionRename')}
                                        className="tap-highlight-none shrink-0 rounded-md p-1.5 text-zinc-400 transition-colors hover:bg-zinc-200 hover:text-black dark:text-zinc-500 dark:hover:bg-white/10 dark:hover:text-white"
                                    >
                                        <Pencil size={14} />
                                    </button>

                                    {isCurrent ? (
                                        <button
                                            type="button"
                                            onClick={() => onCloseSession(session.id)}
                                            className="tap-highlight-none flex shrink-0 items-center gap-1.5 rounded-md bg-zinc-200 px-2.5 py-1 text-[11px] font-medium text-zinc-700 transition-colors hover:bg-zinc-300 dark:bg-white/10 dark:text-zinc-200 dark:hover:bg-white/20"
                                        >
                                            <Archive size={13} />
                                            {t('sessionClose')}
                                        </button>
                                    ) : (
                                        <button
                                            type="button"
                                            onClick={() => onOpenSession(session.id)}
                                            className="tap-highlight-none flex shrink-0 items-center gap-1.5 rounded-md bg-emerald-600 px-2.5 py-1 text-[11px] font-medium text-white transition-colors hover:bg-emerald-700"
                                        >
                                            <FolderOpen size={13} />
                                            {t('sessionOpen')}
                                        </button>
                                    )}
                                </div>
                            );
                        })
                    )}
                </div>
            </div>
        </div>
    );
};
