import React, { useMemo, useState } from 'react';
import { Song } from '../services/api';
import { useI18n } from '../context/I18nContext';
import { ArrowLeft, Play, Pause, MoreHorizontal, Shuffle, Trash2, Pencil, GripVertical, Plus, Search, Check, ListMusic, RotateCcw, Loader2, Rows3, Rows4 } from 'lucide-react';
import { deleteNativePlaylist, parseDuration, restoreTrashedSong, updateNativePlaylist } from '../services/nativeLibrary';
import { libraryChanged, updateLibraryPlaylists, useLibraryPlaylists, useLibrarySongs, useLibraryTrashedSongs } from '../services/studioQueries';
import { named } from '../services/accessibleName';
import { captionSummary } from '../services/examples';
import { ownsSong, useSongActions } from '../context/SongActionsContext';
import { useAuth } from '../context/AuthContext';
import { DENSITY_KEY, readDensity, SongItem, type Density } from './SongList';
import { SongCover } from './AlbumCover';
import { ConfirmDialog } from './ConfirmDialog';
import { collectionSongAvailability, reorderVisibleCollectionSongs, PlaylistCover, PlaylistEditModal, PlaylistMenu, playlistLength } from './PlaylistEditor';
import type { Playlist } from '../types';
import type { ToastType } from './Toast';

interface PlaylistDetailProps {
    playlistId: string;
    onBack: () => void;
    onPlaySong: (song: Song, list?: Song[]) => void;
    onSelect: (song: Song) => void;
    selectedSong?: Song | null;
    onToggleLike?: (id: string) => void;
    onOpenCoverRegen?: (song: Song) => void;
    onSongUpdate?: (song: Song) => void;
    /** What the player holds, so its row says so. */
    currentSong: Song | null;
    isPlaying: boolean;
    /** A message for a moment, with one step it offers, such as undo. */
    notify: (message: string, type?: ToastType, action?: { label: string; onClick: () => void }) => void;
    /** Says why a change to the playlist did not go through. */
    onError: (message: string) => void;
}

function shuffled<T>(items: T[]): T[] {
    const copy = [...items];
    for (let index = copy.length - 1; index > 0; index -= 1) {
        const other = Math.floor(Math.random() * (index + 1));
        [copy[index], copy[other]] = [copy[other], copy[index]];
    }
    return copy;
}

export const PlaylistDetail: React.FC<PlaylistDetailProps> = ({ playlistId, onBack, onPlaySong, onSelect, selectedSong, onToggleLike, onOpenCoverRegen, onSongUpdate, currentSong, isPlaying, notify, onError }) => {
    const { t, songCount } = useI18n();
    const actions = useSongActions();
    const { user } = useAuth();
    const [density, setDensity] = useState<Density>(readDensity);
    const toggleDensity = () => {
        const next = density === 'compact' ? 'comfortable' : 'compact';
        setDensity(next);
        try { window.localStorage.setItem(DENSITY_KEY, next); } catch { /* visual choice lasts this session */ }
    };
    // the library's own reads: a change made anywhere, a deletion included, shows here at once
    const playlists = useLibraryPlaylists();
    const library = useLibrarySongs();
    const bin = useLibraryTrashedSongs();
    const playlist = playlists.data?.find(entry => entry.id === playlistId) ?? null;
    const loading = playlists.isPending || library.isPending;
    const availability = useMemo(
        () => collectionSongAvailability(playlist ?? { id: '', name: '' }, library.data ?? [], bin.data?.songs ?? []),
        [playlist, library.data, bin.data],
    );
    const songs = availability.active;
    const [restoringId, setRestoringId] = useState<string | null>(null);
    const [confirmDelete, setConfirmDelete] = useState(false);
    const [editing, setEditing] = useState<Playlist | null>(null);
    const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
    const [dragId, setDragId] = useState<string | null>(null);
    const [dropAt, setDropAt] = useState<number | null>(null);
    const [adding, setAdding] = useState<boolean | null>(null);
    const [query, setQuery] = useState('');

    // the order is shown at once and kept by the library; when it refuses, the list goes back
    const saveSongs = async (songIds: string[]): Promise<boolean> => {
        if (!playlist) return false;
        const before = playlist;
        updateLibraryPlaylists(lists => lists.map(entry => (entry.id === before.id ? { ...entry, songIds, song_count: songIds.length } : entry)));
        try {
            const updated = await updateNativePlaylist(before.id, before, songIds);
            updateLibraryPlaylists(lists => lists.map(entry => (entry.id === updated.id ? updated : entry)));
            return true;
        } catch (error) {
            updateLibraryPlaylists(lists => lists.map(entry => (entry.id === before.id ? before : entry)));
            onError(error instanceof Error ? error.message : String(error));
            return false;
        }
    };

    const removeSong = (song: Song) => {
        if (!playlist) return;
        // Use the full stored list, including songs in the Bin. Removing one
        // visible track must never silently detach other, restorable tracks.
        const ids = [...(playlist.songIds ?? [])];
        void saveSongs(ids.filter(id => id !== song.id)).then(saved => {
            if (!saved) return;
            notify(t('removedFromPlaylist').replace('{title}', song.title).replace('{name}', playlist.name), 'success', {
                label: t('undo'),
                onClick: () => void saveSongs(ids),
            });
        });
    };

    const moveSong = (songId: string, to: number) => {
        if (!playlist) return;
        const ids = reorderVisibleCollectionSongs(
            playlist.songIds ?? [], songs.map(entry => entry.id), songId, to,
        );
        if (ids) void saveSongs(ids);
    };

    const addSong = (song: Song) => {
        if (!playlist || playlist.songIds?.includes(song.id)) return;
        // Keep all stored memberships, even when some songs are in the Bin.
        setAdding(true);
        void saveSongs([...(playlist.songIds ?? []), song.id]);
    };

    const restoreFromBin = async (song: Song) => {
        setRestoringId(song.id);
        try {
            await restoreTrashedSong(song.id);
            libraryChanged();
            notify(t('trashRestored').replace('{title}', song.title), 'success');
        } catch (error) {
            onError(error instanceof Error ? error.message : String(error));
        } finally {
            setRestoringId(null);
        }
    };

    const handleDeletePlaylist = async () => {
        if (!playlist) return;
        setConfirmDelete(false);
        try {
            await deleteNativePlaylist(playlist.id);
            updateLibraryPlaylists(lists => lists.filter(entry => entry.id !== playlist.id));
            onBack();
        } catch (error) {
            onError(error instanceof Error ? error.message : String(error));
        }
    };

    if (loading) return (
        <div className="flex items-center justify-center h-full bg-black">
            <div className="text-zinc-400 gap-2 flex items-center">
                <div className="w-4 h-4 border-2 border-zinc-400 border-t-transparent rounded-full animate-spin"></div>
                {t('loadingPlaylist')}
            </div>
        </div>
    );

    if (!playlist) return (
        <div className="flex flex-col items-center justify-center h-full gap-4 bg-black">
            <div className="text-zinc-400">{t('playlistNotFound')}</div>
            <button onClick={onBack} className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 rounded-lg text-white">
                {t('goBack')}
            </button>
        </div>
    );

    const playable = songs.filter(song => song.audioUrl && !song.isGenerating);
    const playingHere = Boolean(currentSong && songs.some(song => song.id === currentSong.id));
    const seconds = songs.reduce((sum, song) => sum + (parseDuration(song.duration) ?? 0), 0);
    const showAdd = adding ?? songs.length === 0;
    const inList = new Set(songs.map(song => song.id));
    const needle = query.trim().toLowerCase();
    const candidates = (library.data ?? [])
        .filter(song => song.audioUrl && !song.isGenerating && !inList.has(song.id))
        .filter(song => !needle || song.title.toLowerCase().includes(needle) || song.style.toLowerCase().includes(needle))
        .slice(0, 30);
    const menuActions = {
        play: () => playable.length && onPlaySong(playable[0], playable),
        shuffle: () => {
            const order = shuffled(playable);
            if (order.length) onPlaySong(order[0], order);
        },
        queue: (next: boolean) => actions.queue?.(playable, next),
        edit: () => setEditing(playlist),
        remove: () => setConfirmDelete(true),
    };
    const gradients = ['from-purple-900 to-black', 'from-blue-900 to-black', 'from-indigo-900 to-black', 'from-rose-900 to-black'];
    const bgGradient = gradients[playlist.name.length % gradients.length];

    return (
        <div className={`relative w-full h-full flex flex-col bg-linear-to-b ${bgGradient} overflow-hidden`}>
            {/* Header */}
            <div className="shrink-0 p-4 md:p-8 pt-12 md:pt-8 flex flex-col md:flex-row gap-4 md:gap-8 items-center md:items-end bg-black/20 backdrop-blur-lg border-b border-white/10">
                <button
                    type="button"
                    onClick={() => setEditing(playlist)}
                    {...named(t('editPlaylistDetails'))}
                    className="group relative h-32 w-32 shrink-0 overflow-hidden rounded-lg bg-zinc-800 shadow-2xl md:h-52 md:w-52"
                >
                    <PlaylistCover playlist={playlist} songs={songs} className="h-full w-full" />
                    <span className="absolute inset-0 flex items-center justify-center bg-black/50 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
                        <Pencil size={36} className="text-white" />
                    </span>
                </button>

                <div className="min-w-0 flex-1 space-y-2 md:space-y-4 text-center md:text-left">
                    <span className="text-xs font-bold tracking-wider uppercase text-white/80">{playlist.kind === 'album' ? t('album') : t('playlist')}</span>
                    <h1>
                        <button
                            type="button"
                            onClick={() => setEditing(playlist)}
                            title={t('editPlaylistDetails')}
                            className="max-w-full wrap-break-word text-left text-2xl md:text-5xl lg:text-7xl font-bold text-white tracking-tight leading-none drop-shadow-lg hover:underline"
                        >
                            {playlist.name}
                        </button>
                    </h1>
                    {playlist.description && (
                        <p className="text-zinc-300 text-sm max-w-2xl">{playlist.description}</p>
                    )}
                    <div className="flex items-center justify-center md:justify-start gap-2 text-sm text-white font-medium flex-wrap">
                        <span>{songCount(songs.length)}</span>
                        {availability.inBin.length > 0 && bin.isSuccess && (
                            <span className="text-amber-300">{t('collectionInBin').replace('{count}', String(availability.inBin.length))}</span>
                        )}
                        {seconds > 0 && (
                            <>
                                <span className="w-1 h-1 rounded-full bg-white/50"></span>
                                <span className="text-zinc-300">{playlistLength(seconds, t)}</span>
                            </>
                        )}
                    </div>
                </div>
            </div>

            {/* Actions Bar */}
            <div className="px-4 md:px-8 py-3 md:py-4 bg-black/20 flex items-center gap-3 md:gap-4">
                <button
                    onClick={menuActions.play}
                    disabled={!playable.length}
                    {...named(playable.length ? t('play') : t('playlistNothingToPlay'))}
                    className="w-12 h-12 md:w-14 md:h-14 rounded-full bg-green-500 hover:scale-105 transition-transform flex items-center justify-center text-black shadow-lg disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:scale-100"
                >
                    {playingHere && isPlaying ? <Pause size={24} fill="currentColor" /> : <Play size={24} fill="currentColor" className="ml-1" />}
                </button>
                <button
                    onClick={menuActions.shuffle}
                    disabled={!playable.length}
                    {...named(t('shufflePlay'))}
                    className="p-2 text-zinc-300 transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
                >
                    <Shuffle size={22} />
                </button>
                <button
                    onClick={() => setEditing(playlist)}
                    {...named(t('editPlaylistDetails'))}
                    className="p-2 text-zinc-300 transition-colors hover:text-white"
                >
                    <Pencil size={20} />
                </button>
                <button
                    onClick={(event) => {
                        const box = event.currentTarget.getBoundingClientRect();
                        setMenuAt({ x: box.left, y: box.bottom + 6 });
                    }}
                    aria-haspopup="menu"
                    {...named(t('moreActions'))}
                    className="p-2 text-zinc-300 transition-colors hover:text-white"
                >
                    <MoreHorizontal size={22} />
                </button>
                <button type="button"
                    onClick={toggleDensity}
                    aria-pressed={density === 'compact'}
                    {...named(density === 'compact' ? t('listComfortable') : t('listCompact'))}
                    className="rounded-lg p-2 text-zinc-300 transition-colors hover:bg-white/10 hover:text-white"
                >
                    {density === 'compact' ? <Rows3 size={20} /> : <Rows4 size={20} />}
                </button>
                <button
                    onClick={() => setConfirmDelete(true)}
                    className="p-2 text-zinc-400 transition-colors hover:text-red-500"
                    {...named(playlist.kind === 'album' ? t('deleteAlbum') : t('deletePlaylist'))}
                >
                    <Trash2 size={20} />
                </button>
            </div>

            {/* Song List */}
            <div className="flex-1 overflow-y-auto bg-black/40">
                <div className="px-2 md:px-8 py-2 md:py-4 pb-24 lg:pb-32">
                    <ol className="space-y-1" onDragLeave={(event) => { if (event.currentTarget === event.target) setDropAt(null); }}>
                        {songs.map((song, index) => {
                            const current = currentSong?.id === song.id;
                            return (
                                <li
                                    key={song.id}
                                    data-mcp-context={`song ${song.id}: ${song.title}`}
                                    tabIndex={0}
                                    aria-current={current ? 'true' : undefined}
                                    onDragOver={(event) => {
                                        if (!dragId) return;
                                        event.preventDefault();
                                        const bounds = event.currentTarget.getBoundingClientRect();
                                        setDropAt(event.clientY < bounds.top + bounds.height / 2 ? index : index + 1);
                                    }}
                                    onDrop={(event) => {
                                        if (!dragId) return;
                                        event.preventDefault();
                                        if (dropAt !== null) moveSong(dragId, dropAt);
                                        setDragId(null);
                                        setDropAt(null);
                                    }}
                                    onKeyDown={(event) => {
                                        if (event.target !== event.currentTarget && (event.target as HTMLElement).closest('input, textarea, button')) return;
                                        if (event.altKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
                                            event.preventDefault();
                                            moveSong(song.id, event.key === 'ArrowUp' ? index - 1 : index + 2);
                                        } else if (event.target === event.currentTarget && event.key === 'Enter') {
                                            event.preventDefault();
                                            onSelect(song);
                                        }
                                    }}
                                    className={`group relative flex items-center gap-1 rounded-md outline-hidden focus-visible:ring-2 focus-visible:ring-pink-500 ${dragId === song.id ? 'opacity-40' : ''}`}
                                >
                                    {dropAt === index && <span className="pointer-events-none absolute -top-0.5 left-0 right-0 z-10 h-0.5 bg-green-500" />}
                                    {dropAt === index + 1 && index === songs.length - 1 && <span className="pointer-events-none absolute -bottom-0.5 left-0 right-0 z-10 h-0.5 bg-green-500" />}
                                    <span
                                        draggable
                                        tabIndex={0}
                                        onDragStart={event => {
                                            event.dataTransfer.effectAllowed = 'move';
                                            event.dataTransfer.setData('text/plain', song.id);
                                            setDragId(song.id);
                                        }}
                                        onDragEnd={() => { setDragId(null); setDropAt(null); }}
                                        onClick={event => event.stopPropagation()}
                                        title={t('dragToReorder')}
                                        aria-label={t('dragToReorder')}
                                        className="shrink-0 cursor-grab rounded p-1 text-zinc-500 hover:text-white"
                                    >
                                        <GripVertical size={16} />
                                    </span>
                                    <div className="min-w-0 flex-1">
                                        <SongItem
                                            song={song}
                                            isCurrent={current}
                                            isSelected={selectedSong?.id === song.id}
                                            isSelectionMode={false}
                                            compact={density === 'compact'}
                                            isChecked={false}
                                            isLiked={Boolean(song.liked)}
                                            isPlaying={isPlaying}
                                            isOwner={ownsSong(user, song)}
                                            onPlay={() => onPlaySong(song, playable)}
                                            onSelect={() => onSelect(song)}
                                            onToggleSelect={() => {}}
                                            onToggleLike={() => onToggleLike?.(song.id)}
                                            onAddToPlaylist={() => actions.addToPlaylist?.(song)}
                                            onOpenCoverRegen={() => onOpenCoverRegen?.(song)}
                                            onShowDetails={() => onSelect(song)}
                                            detailsOpen={selectedSong?.id === song.id}
                                            onSongUpdate={onSongUpdate}
                                            onRemoveFromCollection={() => removeSong(song)}
                                            removeFromCollectionLabel={playlist.kind === 'album' ? t('removeFromAlbum') : t('removeFromPlaylist')}
                                        />
                                    </div>
                                    <button
                                        type="button"
                                        onClick={() => removeSong(song)}
                                        {...named(playlist.kind === 'album' ? t('removeFromAlbum') : t('removeFromPlaylist'))}
                                        className="shrink-0 rounded p-2 text-zinc-500 hover:bg-red-500/10 hover:text-red-400"
                                    >
                                        <Trash2 size={16} />
                                    </button>
                                </li>
                            );
                        })}
                    </ol>

                    {/* Keep Bin memberships visible and recoverable, without treating them as playable songs. */}
                    {bin.isSuccess && (availability.inBin.length > 0 || availability.missingIds.length > 0) && (
                        <section className="mt-6 rounded-lg border border-amber-500/25 bg-amber-500/5 p-4" aria-label={t('collectionBinTitle')}>
                            <h2 className="font-semibold text-amber-100">{t('collectionBinTitle')}</h2>
                            {availability.inBin.length > 0 && (
                                <p className="mt-1 text-xs text-zinc-400">{t('collectionBinHint')}</p>
                            )}
                            <ul className="mt-3 space-y-2">
                                {availability.inBin.map(entry => (
                                    <li key={entry.song.id} className="flex items-center gap-3 rounded-md bg-black/30 px-3 py-2">
                                        <div className="min-w-0 flex-1 truncate text-sm text-white">{entry.song.title}</div>
                                        <button
                                            type="button"
                                            onClick={() => void restoreFromBin(entry.song)}
                                            disabled={restoringId !== null}
                                            className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-white/20 px-3 py-1 text-xs text-white hover:bg-white/10 disabled:opacity-50"
                                        >
                                            {restoringId === entry.song.id ? <Loader2 size={13} className="animate-spin" /> : <RotateCcw size={13} />}
                                            {t('trashRestore')}
                                        </button>
                                    </li>
                                ))}
                            </ul>
                            {availability.missingIds.length > 0 && (
                                <p className="mt-3 text-xs text-amber-200">{t('collectionMissing').replace('{count}', String(availability.missingIds.length))}</p>
                            )}
                        </section>
                    )}
                    {bin.isError && (
                        <p className="mt-4 text-xs text-amber-300">{t('collectionBinCheckFailed')}</p>
                    )}

                    {/* songs to add, found right here: an empty playlist opens with it */}
                    <section className="mt-8 border-t border-white/10 pt-6" aria-labelledby="playlist-add-title">
                        {songs.length === 0 && availability.inBin.length === 0 && (
                            <div className="mb-6 flex flex-col items-center gap-2 text-center text-zinc-400">
                                <ListMusic size={36} className="text-zinc-500" />
                                <p className="font-medium text-white">{t('playlistEmptyTitle')}</p>
                                <p className="max-w-md text-sm">{t('playlistEmptyHint')}</p>
                            </div>
                        )}
                        <div className="flex items-center justify-between gap-3">
                            <h2 id="playlist-add-title" className="text-lg font-bold text-white">{t('findSongsForPlaylist')}</h2>
                            <button
                                type="button"
                                onClick={() => setAdding(!showAdd)}
                                aria-expanded={showAdd}
                                className="rounded-full border border-white/20 px-3 py-1.5 text-xs font-semibold text-zinc-200 hover:bg-white/10"
                            >
                                {showAdd ? t('hideAddSongs') : t('addSongs')}
                            </button>
                        </div>
                        {showAdd && (
                            <div className="mt-4 space-y-3">
                                <div className="relative max-w-md">
                                    <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-500" />
                                    <input
                                        type="search"
                                        value={query}
                                        onChange={(event) => setQuery(event.target.value)}
                                        placeholder={t('searchYourSongs')}
                                        aria-label={t('searchYourSongs')}
                                        className="w-full rounded-lg border border-white/10 bg-black/40 py-2.5 pl-9 pr-3 text-sm text-white placeholder-zinc-500 focus:border-white/30 focus:outline-hidden"
                                    />
                                </div>
                                {candidates.length === 0 ? (
                                    <p className="text-sm text-zinc-500">{needle ? t('noSongsMatchFilters') : t('allSongsInPlaylist')}</p>
                                ) : (
                                    <ul className="space-y-1">
                                        {candidates.map(song => (
                                            <li key={song.id} className="flex items-center gap-3 rounded-md px-2 py-2 text-sm text-zinc-300 hover:bg-white/10">
                                                <div className="h-10 w-10 shrink-0 overflow-hidden rounded-sm bg-zinc-800">
                                                    <SongCover song={song} size="full" className="h-full w-full" />
                                                </div>
                                                <div className="min-w-0 flex-1">
                                                    <p className="truncate font-medium text-white">{song.title}</p>
                                                    <p className="truncate text-xs text-zinc-500">{captionSummary(song.style)}</p>
                                                </div>
                                                <span className="font-mono text-xs text-zinc-500">{song.duration}</span>
                                                <button
                                                    type="button"
                                                    onClick={() => addSong(song)}
                                                    className="flex items-center gap-1 rounded-full border border-white/20 px-3 py-1 text-xs font-semibold text-white hover:border-white hover:bg-white/10"
                                                >
                                                    <Plus size={14} />
                                                    {t('add')}
                                                </button>
                                            </li>
                                        ))}
                                    </ul>
                                )}
                                {songs.length > 0 && <p className="flex items-center gap-1.5 text-xs text-zinc-500"><Check size={12} />{t('playlistReorderHint')}</p>}
                            </div>
                        )}
                    </section>
                </div>
            </div>

            <PlaylistMenu kind={playlist.kind} at={menuAt} empty={!playable.length} actions={menuActions} onClose={() => setMenuAt(null)} />
            <PlaylistEditModal playlist={editing} onClose={() => setEditing(null)} onError={onError} />
            <ConfirmDialog
                isOpen={confirmDelete}
                title={playlist.kind === 'album' ? t('deleteAlbum') : t('deletePlaylist')}
                message={playlist.kind === 'album' ? t('deleteAlbumConfirm') : t('deletePlaylistConfirm')}
                confirmLabel={playlist.kind === 'album' ? t('deleteAlbum') : t('deletePlaylist')}
                onConfirm={() => void handleDeletePlaylist()}
                onCancel={() => setConfirmDelete(false)}
            />

            <button
                onClick={onBack}
                {...named(t('goBack'))}
                className="absolute top-6 left-6 z-50 w-8 h-8 rounded-full bg-black/50 flex items-center justify-center text-white hover:bg-black/70 transition-colors"
            >
                <ArrowLeft size={18} />
            </button>
        </div>
    );
};
