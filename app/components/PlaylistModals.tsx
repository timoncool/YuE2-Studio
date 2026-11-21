import React, { useState } from 'react';
import { createPortal } from 'react-dom';
import { X, Plus, Music, GripVertical } from 'lucide-react';
import { Playlist } from '../types';
import { useI18n } from '../context/I18nContext';
import { useLibrarySongs, useLibraryTrashedSongs } from '../services/studioQueries';
import { collectionSongAvailability } from './PlaylistEditor';

interface CreatePlaylistModalProps {
  isOpen: boolean;
  onClose: () => void;
  onCreate: (name: string, description: string) => void;
  kind?: 'playlist' | 'album';
}

export const CreatePlaylistModal: React.FC<CreatePlaylistModalProps> = ({ isOpen, onClose, onCreate, kind = 'playlist' }) => {
  const { t } = useI18n();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');

  if (!isOpen) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (name.trim()) {
      onCreate(name, description);
      setName('');
      setDescription('');
      onClose();
    }
  };

  // over the whole window, wherever it is opened from
  return createPortal(
    <div className="fixed inset-0 z-60 flex items-center justify-center bg-black/50 dark:bg-black/80 backdrop-blur-xs p-4">
      <div className="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-white/10 rounded-xl w-full max-w-md p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-200">
        <div className="flex justify-between items-center mb-6">
          <h2 className="text-xl font-bold text-zinc-900 dark:text-white">{kind === 'album' ? t('createAlbum') : t('createPlaylist')}</h2>
          <button onClick={onClose} className="text-zinc-400 hover:text-zinc-900 dark:hover:text-white">
            <X size={20} />
          </button>
        </div>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase mb-1">{t('playlistName')}</label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="w-full bg-zinc-50 dark:bg-black/50 border border-zinc-200 dark:border-white/10 rounded-lg p-3 text-zinc-900 dark:text-white focus:outline-hidden focus:border-pink-500 focus:ring-1 focus:ring-pink-500 placeholder-zinc-400 dark:placeholder-zinc-600"
              placeholder={kind === 'album' ? t('albumNamePlaceholder') : t('playlistNamePlaceholder')}
              autoFocus
            />
          </div>
          <div>
            <label className="block text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase mb-1">{t('playlistDescription')}</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              className="w-full bg-zinc-50 dark:bg-black/50 border border-zinc-200 dark:border-white/10 rounded-lg p-3 text-zinc-900 dark:text-white focus:outline-hidden focus:border-pink-500 focus:ring-1 focus:ring-pink-500 resize-none h-24 placeholder-zinc-400 dark:placeholder-zinc-600"
              placeholder={t('descriptionPlaceholder')}
            />
          </div>
          <div className="flex justify-end gap-3 mt-6">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 rounded-lg text-sm font-medium text-zinc-600 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-white hover:bg-zinc-100 dark:hover:bg-white/5 transition-colors"
            >
              {t('cancel')}
            </button>
            <button
              type="submit"
              disabled={!name.trim()}
              className="px-4 py-2 rounded-lg text-sm font-bold bg-zinc-900 dark:bg-white text-white dark:text-black hover:scale-105 transition-transform disabled:opacity-50 disabled:hover:scale-100 shadow-lg"
            >
              {t('createButton')}
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body,
  );
};

interface AddToPlaylistModalProps {
  isOpen: boolean;
  onClose: () => void;
  playlists: Playlist[];
  onSelect: (playlistId: string) => void;
  onCreateNew?: () => void;
  onCreateNewAlbum?: () => void;
  onReorderAlbums?: (fromId: string, toId: string) => Promise<void>;
  /** Choose only the requested collection type, not a mixed playlist/album list. */
  kind?: 'playlist' | 'album' | 'all';
}

/** Playlists and albums share storage, but choosing one in the context menu
 * must not show unrelated collections. */
export function collectionsForAdding(playlists: Playlist[], kind: 'playlist' | 'album' | 'all'): Playlist[] {
  return playlists.filter(playlist => kind === 'all' || (kind === 'album' ? playlist.kind === 'album' : playlist.kind !== 'album'));
}

export const AddToPlaylistModal: React.FC<AddToPlaylistModalProps> = ({ isOpen, onClose, playlists, onSelect, onCreateNew, onCreateNewAlbum, onReorderAlbums, kind = 'all' }) => {
  const { t, songCount } = useI18n();
  const [draggedAlbum, setDraggedAlbum] = useState<string | null>(null);
  const [hoverAlbum, setHoverAlbum] = useState<string | null>(null);
  const [savingAlbumOrder, setSavingAlbumOrder] = useState(false);
  const library = useLibrarySongs();
  const bin = useLibraryTrashedSongs(isOpen);
  if (!isOpen) return null;
  const choices = collectionsForAdding(playlists, kind);

  return (
    <div className="fixed inset-0 z-60 flex items-center justify-center bg-black/50 dark:bg-black/80 backdrop-blur-xs p-4">
      <div className="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-white/10 rounded-xl w-full max-w-sm p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-200">
        <div className="flex justify-between items-center mb-4">
          <h2 className="text-lg font-bold text-zinc-900 dark:text-white">{kind === 'album' ? t('addToAlbum') : kind === 'playlist' ? t('addToPlaylist') : t('addToCollection')}</h2>
          <button onClick={onClose} className="text-zinc-400 hover:text-zinc-900 dark:hover:text-white">
            <X size={20} />
          </button>
        </div>

        {/* Create New Button - Always visible at top */}
        {kind !== 'album' && <button
          onClick={onCreateNew}
          className="w-full flex items-center gap-3 p-3 rounded-lg hover:bg-zinc-100 dark:hover:bg-white/5 transition-colors group mb-3 border border-dashed border-zinc-300 dark:border-white/20 hover:border-zinc-400 dark:hover:border-white/40"
        >
          <div className="w-10 h-10 bg-zinc-100 dark:bg-zinc-800/50 rounded-sm flex items-center justify-center text-zinc-600 dark:text-white/70 group-hover:text-zinc-900 dark:group-hover:text-white">
            <Plus size={20} />
          </div>
          <div className="text-left">
            <div className="font-semibold text-zinc-700 dark:text-white/90 group-hover:text-zinc-900 dark:group-hover:text-white">{t('createNewPlaylist')}</div>
          </div>
        </button>}

        {kind !== 'playlist' && onCreateNewAlbum && (
          <button type="button" onClick={onCreateNewAlbum} className="mb-3 flex w-full items-center gap-3 rounded-lg border border-dashed border-zinc-300 p-3 text-left transition-colors hover:bg-zinc-100 dark:border-white/20 dark:hover:bg-white/5">
            <div className="flex h-10 w-10 items-center justify-center rounded-sm bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-white/70"><Plus size={20} /></div>
            <span className="font-semibold text-zinc-700 dark:text-white/90">{t('createAlbum')}</span>
          </button>
        )}
        <div className="h-px bg-zinc-100 dark:bg-white/10 my-2"></div>

        <div className="space-y-1 max-h-60 overflow-y-auto custom-scrollbar">
          {choices.length === 0 ? (
            <div className="text-center py-6 text-zinc-500 text-sm italic">
              {kind === 'album' ? t('albumsEmptyTitle') : t('noExistingPlaylists')}
            </div>
          ) : (
            choices.map(playlist => {
              const availability = collectionSongAvailability(playlist, library.data ?? [], bin.data?.songs ?? []);
              return (
              <button
                key={playlist.id}
                draggable={kind === 'album' && Boolean(onReorderAlbums) && !savingAlbumOrder}
                onDragStart={event => {
                  if (kind !== 'album') return;
                  event.dataTransfer.effectAllowed = 'move';
                  event.dataTransfer.setData('text/plain', playlist.id);
                  setDraggedAlbum(playlist.id);
                }}
                onDragOver={event => {
                  if (kind !== 'album' || !draggedAlbum || savingAlbumOrder) return;
                  event.preventDefault();
                  event.dataTransfer.dropEffect = 'move';
                  setHoverAlbum(playlist.id);
                }}
                onDrop={event => {
                  if (kind !== 'album' || !draggedAlbum || !onReorderAlbums) return;
                  event.preventDefault();
                  const from = draggedAlbum;
                  setDraggedAlbum(null);
                  setHoverAlbum(null);
                  if (from === playlist.id) return;
                  setSavingAlbumOrder(true);
                  void onReorderAlbums(from, playlist.id).finally(() => setSavingAlbumOrder(false));
                }}
                onDragEnd={() => { setDraggedAlbum(null); setHoverAlbum(null); }}
                onClick={() => {
                  if (savingAlbumOrder) return;
                  onSelect(playlist.id);
                  onClose();
                }}
                className={`group flex w-full items-center gap-3 rounded-lg p-3 transition-colors hover:bg-zinc-100 dark:hover:bg-white/5 ${kind === 'album' ? 'cursor-grab active:cursor-grabbing' : ''} ${draggedAlbum === playlist.id ? 'opacity-40' : ''} ${hoverAlbum === playlist.id && draggedAlbum !== playlist.id ? 'ring-2 ring-pink-500' : ''}`}
              >
                <div className="w-10 h-10 bg-zinc-200 dark:bg-zinc-800 rounded-sm flex items-center justify-center text-zinc-500 group-hover:text-zinc-900 dark:group-hover:text-white shrink-0 overflow-hidden">
                  {playlist.coverUrl ? (
                    <img src={playlist.coverUrl} className="w-full h-full object-cover" alt="" />
                  ) : (
                    <Music size={18} />
                  )}
                </div>
                <div className="text-left overflow-hidden">
                  <div className="font-medium text-zinc-900 dark:text-white truncate">{playlist.name}</div>
                  <div className="text-xs text-zinc-500">
                    {playlist.kind === 'album' ? t('album') : t('playlist')} · {library.isSuccess ? songCount(availability.active.length) : '…'}
                    {bin.isSuccess && availability.inBin.length > 0 && <span className="text-amber-400"> · {t('collectionInBin').replace('{count}', String(availability.inBin.length))}</span>}
                    {bin.isSuccess && availability.missingIds.length > 0 && <span className="text-amber-400"> · {t('collectionMissing').replace('{count}', String(availability.missingIds.length))}</span>}
                  </div>
                </div>
                {kind === 'album' && <GripVertical size={17} className="ml-auto shrink-0 text-zinc-400" aria-label={t('dragAlbumToReorder')} />}
              </button>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
};