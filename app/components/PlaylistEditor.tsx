import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { X, Play, Pencil, Trash2, ListStart, ListEnd, Shuffle, Upload, ImageOff } from 'lucide-react';
import type { Playlist, Song } from '../types';
import type { TrashedSong } from '../services/nativeLibrary';
import { useI18n } from '../context/I18nContext';
import { AlbumCover, SongCover } from './AlbumCover';
import { updateNativePlaylist } from '../services/nativeLibrary';
import { updateLibraryPlaylists } from '../services/studioQueries';

/** A playlist's songs in its order, those still in the library. */
export function playlistSongs(playlist: Playlist, library: Song[]): Song[] {
  const byId = new Map(library.map((song) => [song.id, song]));
  return (playlist.songIds ?? []).flatMap((id) => (byId.has(id) ? [byId.get(id)!] : []));
}

/** Match saved collection memberships against active tracks and recoverable Bin records.
 * A song in the Bin retains its album membership so restoring it brings it back
 * to its original position. Never overwrite songIds with the filtered active list. */
export function collectionSongAvailability(playlist: Playlist, library: Song[], bin: TrashedSong[] = []) {
  const active = playlistSongs(playlist, library);
  const activeIds = new Set(active.map(song => song.id));
  const binById = new Map(bin.map(entry => [entry.song.id, entry]));
  const inBin: TrashedSong[] = [];
  const missingIds: string[] = [];
  for (const id of playlist.songIds ?? []) {
    if (activeIds.has(id)) continue;
    const found = binById.get(id);
    if (found) inBin.push(found);
    else missingIds.push(id);
  }
  return { active, inBin, missingIds };
}

/** Reorder only the visible tracks while preserving the Bin/missing IDs at
 * their existing positions. The hidden tracks must survive every list edit. */
export function reorderVisibleCollectionSongs(allIds: string[], visibleIds: string[], songId: string, to: number): string[] | null {
  const from = visibleIds.indexOf(songId);
  if (from < 0 || to === from || to === from + 1) return null;
  const nextVisible = [...visibleIds];
  nextVisible.splice(from, 1);
  nextVisible.splice(to > from ? to - 1 : to, 0, songId);
  const visibleSet = new Set(visibleIds);
  let offset = 0;
  return allIds.map(id => visibleSet.has(id) ? nextVisible[offset++] : id);
}

/** "1 h 12 min", "4 min 05 s", "58 s": a playlist's length, never rounded down to nothing. */
export function playlistLength(seconds: number, t: (key: 'hoursShort' | 'minutesShort' | 'secondsShort') => string): string {
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  if (hours) return `${hours} ${t('hoursShort')} ${minutes} ${t('minutesShort')}`;
  if (minutes) return `${minutes} ${t('minutesShort')} ${String(rest).padStart(2, '0')} ${t('secondsShort')}`;
  return `${rest} ${t('secondsShort')}`;
}

/** The playlist's picture: the covers of its first four songs, as Spotify draws one; one song, its cover. */
export const PlaylistCover: React.FC<{ playlist: Playlist; songs: Song[]; className?: string }> = ({ playlist, songs, className = '' }) => {
  if (playlist.coverUrl) return <img src={playlist.coverUrl} alt="" className={`object-cover ${className}`} />;
  const seen = new Set<string>();
  const covers = songs.filter((song) => {
    const key = song.coverUrl || song.id;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  if (covers.length >= 4) {
    return (
      <div className={`grid grid-cols-2 grid-rows-2 overflow-hidden ${className}`}>
        {covers.slice(0, 4).map((song) => <SongCover key={song.id} song={song} size="full" className="h-full w-full" />)}
      </div>
    );
  }
  if (covers.length) return <SongCover song={covers[0]} size="full" className={className} />;
  return <AlbumCover seed={playlist.id || playlist.name} size="full" className={className} />;
};

/** Resize custom cover art to a compact JPEG before saving it with the collection. */
export async function prepareCollectionCover(file: File): Promise<string> {
  if (!['image/png', 'image/jpeg', 'image/webp'].includes(file.type)) throw new Error('Use a JPG, PNG or WebP image.');
  if (file.size > 8 * 1024 * 1024) throw new Error('The image must be smaller than 8 MB.');
  return new Promise<string>((resolve, reject) => {
    const objectUrl = URL.createObjectURL(file);
    const image = new Image();
    image.onload = () => {
      URL.revokeObjectURL(objectUrl);
      try {
        const scale = Math.min(1, 512 / Math.max(image.naturalWidth, image.naturalHeight));
        const canvas = document.createElement('canvas');
        canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
        canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
        const ctx = canvas.getContext('2d');
        if (!ctx) throw new Error('Image editing is unavailable.');
        ctx.fillStyle = '#111111';
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
        const value = canvas.toDataURL('image/jpeg', 0.78);
        if (value.length > 450000) throw new Error('The cover image is too large after resizing.');
        resolve(value);
      } catch (error) { reject(error); }
    };
    image.onerror = () => { URL.revokeObjectURL(objectUrl); reject(new Error('Could not read that image.')); };
    image.src = objectUrl;
  });
}

/** Name, description and custom artwork of a playlist or album. */
export const PlaylistEditModal: React.FC<{ playlist: Playlist | null; onClose: () => void; onError: (message: string) => void }> = ({ playlist, onClose, onError }) => {
  const { t } = useI18n();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [coverUrl, setCoverUrl] = useState<string | undefined>();
  const coverRequest = useRef(0);
  const [preparing, setPreparing] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!playlist) return;
    setName(playlist.name);
    setDescription(playlist.description ?? '');
    setCoverUrl(playlist.coverUrl);
    setPreparing(false);
    return () => { coverRequest.current++; };
  }, [playlist]);

  useEffect(() => {
    if (!playlist) return undefined;
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, [playlist, onClose]);

  if (!playlist) return null;

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!name.trim() || preparing || saving) return;
    setSaving(true);
    try {
      const updated = await updateNativePlaylist(playlist.id, { ...playlist, name: name.trim(), description: description.trim() || undefined, coverUrl });
      updateLibraryPlaylists((lists) => lists.map((entry) => (entry.id === updated.id ? updated : entry)));
      onClose();
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  return createPortal(
    <div className="fixed inset-0 z-60 flex items-center justify-center bg-black/50 p-4 backdrop-blur-xs dark:bg-black/80" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div role="dialog" aria-modal="true" aria-labelledby="playlist-edit-title" className="w-full max-w-md rounded-xl border border-zinc-200 bg-white p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-200 dark:border-white/10 dark:bg-zinc-900">
        <div className="mb-6 flex items-center justify-between">
          <h2 id="playlist-edit-title" className="text-xl font-bold text-zinc-900 dark:text-white">{playlist.kind === 'album' ? t('editAlbum') : t('editPlaylistDetails')}</h2>
          <button type="button" onClick={onClose} aria-label={t('close')} className="text-zinc-400 hover:text-zinc-900 dark:hover:text-white">
            <X size={20} />
          </button>
        </div>
        <form onSubmit={(event) => void save(event)} className="space-y-4">
          <div className="flex items-center gap-4 rounded-lg border border-zinc-200 p-3 dark:border-white/10">
            <div className="h-24 w-24 shrink-0 overflow-hidden rounded-md bg-zinc-200 dark:bg-zinc-800">
              {coverUrl ? <img src={coverUrl} alt={t('coverArt')} className="h-full w-full object-cover" /> : <AlbumCover seed={playlist.id} size="full" className="h-full w-full" />}
            </div>
            <div className="flex min-w-0 flex-col items-start gap-2">
              <label className="inline-flex cursor-pointer items-center gap-2 rounded-lg bg-zinc-900 px-3 py-2 text-xs font-semibold text-white dark:bg-white dark:text-black">
                <Upload size={14} />{t('uploadCollectionCover')}
                <input type="file" className="sr-only" accept="image/png,image/jpeg,image/webp" onChange={event => {
                  const file = event.currentTarget.files?.[0];
                  event.currentTarget.value = '';
                  if (!file) return;
                  const request = ++coverRequest.current;
                  setPreparing(true);
                  void prepareCollectionCover(file).then(value => {
                    if (coverRequest.current === request) setCoverUrl(value);
                  }).catch(error => {
                    if (coverRequest.current === request) onError(error instanceof Error ? error.message : String(error));
                  }).finally(() => {
                    if (coverRequest.current === request) setPreparing(false);
                  });
                }} />
              </label>
              {coverUrl && <button type="button" className="inline-flex items-center gap-2 text-xs text-zinc-500 hover:text-red-500" onClick={() => { coverRequest.current++; setPreparing(false); setCoverUrl(undefined); }}><ImageOff size={14} />{t('removeCollectionCover')}</button>}
              <span className="text-[11px] text-zinc-500">{t('collectionCoverHint')}</span>
            </div>
          </div>
          <div>
            <label htmlFor="playlist-edit-name" className="mb-1 block text-xs font-semibold uppercase text-zinc-500 dark:text-zinc-400">{t('playlistName')}</label>
            <input
              id="playlist-edit-name"
              type="text"
              value={name}
              onChange={(event) => setName(event.target.value)}
              autoFocus
              className="w-full rounded-lg border border-zinc-200 bg-zinc-50 p-3 text-zinc-900 focus:border-pink-500 focus:outline-hidden focus:ring-1 focus:ring-pink-500 dark:border-white/10 dark:bg-black/50 dark:text-white"
            />
          </div>
          <div>
            <label htmlFor="playlist-edit-description" className="mb-1 block text-xs font-semibold uppercase text-zinc-500 dark:text-zinc-400">{t('playlistDescription')}</label>
            <textarea
              id="playlist-edit-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder={t('descriptionPlaceholder')}
              className="h-24 w-full resize-none rounded-lg border border-zinc-200 bg-zinc-50 p-3 text-zinc-900 placeholder-zinc-400 focus:border-pink-500 focus:outline-hidden focus:ring-1 focus:ring-pink-500 dark:border-white/10 dark:bg-black/50 dark:text-white dark:placeholder-zinc-600"
            />
          </div>
          <div className="flex justify-end gap-3 pt-2">
            <button type="button" onClick={onClose} className="px-4 py-2 text-sm font-medium text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white">{t('cancel')}</button>
            <button type="submit" disabled={!name.trim() || preparing || saving} className="rounded-full bg-zinc-900 px-6 py-2 text-sm font-bold text-white transition-transform hover:scale-105 disabled:opacity-50 dark:bg-white dark:text-black">{t('save')}</button>
          </div>
        </form>
      </div>
    </div>,
    document.body,
  );
};

export interface PlaylistMenuActions {
  play: () => void;
  shuffle: () => void;
  queue: (next: boolean) => void;
  edit: () => void;
  remove: () => void;
}

/** What a playlist offers from its "..." button and from a right click. */
export const PlaylistMenu: React.FC<{ at: { x: number; y: number } | null; empty: boolean; actions: PlaylistMenuActions; onClose: () => void; kind?: Playlist['kind'] }> = ({ at, empty, actions, onClose, kind }) => {
  const { t } = useI18n();
  const menu = useRef<HTMLDivElement>(null);
  const [spot, setSpot] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    const node = menu.current;
    if (!at || !node) {
      setSpot(null);
      return;
    }
    const box = node.getBoundingClientRect();
    setSpot({ left: Math.max(8, Math.min(at.x, window.innerWidth - box.width - 8)), top: Math.max(8, Math.min(at.y, window.innerHeight - box.height - 8)) });
  }, [at]);

  useEffect(() => {
    if (!at) return undefined;
    const outside = (event: MouseEvent) => {
      if (menu.current && !menu.current.contains(event.target as Node)) onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', outside);
    document.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('mousedown', outside);
      document.removeEventListener('keydown', key);
    };
  }, [at, onClose]);

  if (!at) return null;
  const item = (icon: React.ReactNode, label: string, run: () => void, options: { disabled?: boolean; danger?: boolean } = {}) => (
    <button
      type="button"
      role="menuitem"
      disabled={options.disabled}
      onClick={(event) => {
        event.stopPropagation();
        onClose();
        run();
      }}
      className={`flex w-full items-center gap-3 px-3 py-2 text-left text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${options.danger ? 'text-red-400 hover:bg-red-500/10 hover:text-red-300' : 'text-zinc-300 hover:bg-white/5 hover:text-white'}`}
    >
      <span className="flex h-4 w-4 items-center justify-center opacity-70">{icon}</span>
      {label}
    </button>
  );
  return createPortal(
    <div
      ref={menu}
      role="menu"
      onClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
      style={{ left: spot?.left ?? at.x, top: spot?.top ?? at.y, visibility: spot ? 'visible' : 'hidden' }}
      className="fixed z-200 w-56 rounded-xl border border-white/10 bg-zinc-900 py-1.5 shadow-2xl transition-none animate-in fade-in"
    >
      {item(<Play size={14} />, t('play'), actions.play, { disabled: empty })}
      {item(<Shuffle size={14} />, t('shufflePlay'), actions.shuffle, { disabled: empty })}
      {item(<ListStart size={14} />, t('playNextMenu'), () => actions.queue(true), { disabled: empty })}
      {item(<ListEnd size={14} />, t('addToQueueMenu'), () => actions.queue(false), { disabled: empty })}
      <div className="mx-2 my-1 h-px bg-white/10" />
      {item(<Pencil size={14} />, kind === 'album' ? t('editAlbum') : t('editPlaylistDetails'), actions.edit)}
      {item(<Trash2 size={14} />, kind === 'album' ? t('deleteAlbum') : t('deletePlaylist'), actions.remove, { danger: true })}
    </div>,
    document.body,
  );
};
