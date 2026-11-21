import React, { useRef, useState } from 'react';
import { useI18n } from '../context/I18nContext';
import { preferredTakeStrings } from '../i18n/preferredTake';
import { selectNativePlaylistPreferredTake } from '../services/nativeLibrary';
import { updateLibraryPlaylists } from '../services/studioQueries';
import type { Playlist, Song } from '../types';
import { TakeWords } from './TakeWords';
import { AudioCompare } from './AudioCompare';

export const PlaylistTakeComparison: React.FC<{
  playlist: Playlist;
  songs: Song[];
  playerIsPlaying: boolean;
  onError: (message: string) => void;
  onOpenLibraryTake: (song: Song) => void;
}> = ({ playlist, songs, playerIsPlaying, onOpenLibraryTake, onError }) => {
  const { language } = useI18n();
  const labels = preferredTakeStrings[language];
  const [open, setOpen] = useState(playlist.id.startsWith('checkpoint-listen-v1-'));
  const [leftId, setLeftId] = useState('');
  const [rightId, setRightId] = useState('');
  const pending = useRef(false);
  const [saving, setSaving] = useState(false);
  if (songs.length < 2) return null;

  const left = songs.find(song => song.id === leftId)
    ?? songs.find(song => song.id === playlist.preferredSongId)
    ?? songs[0];
  const right = songs.find(song => song.id === rightId && song.id !== left.id)
    ?? songs.find(song => song.id !== left.id)!;
  const label = (song: Song) => `${(playlist.songIds ?? []).indexOf(song.id) + 1}. ${song.title}`;
  const mark = async (song: Song) => {
    if (pending.current) return;
    pending.current = true;
    setSaving(true);
    try {
      const preferredSongId = await selectNativePlaylistPreferredTake(playlist.id, song.id);
      updateLibraryPlaylists(lists => lists.map(entry => (
        entry.id === playlist.id && entry.songIds?.includes(preferredSongId)
          ? { ...entry, preferredSongId }
          : entry
      )));
    } catch (error) {
      onError(error instanceof Error ? error.message : String(error));
    } finally {
      pending.current = false;
      setSaving(false);
    }
  };

  const sideSelect = (side: 'A' | 'B', song: Song, other: Song, choose: (id: string) => void) => (
    <label className="flex min-w-0 flex-1 items-center gap-2 text-sm">
      {side}
      <select aria-label={side} className="min-w-0 flex-1 rounded bg-zinc-800 px-2 py-1" value={song.id}
        onChange={event => choose(event.target.value)}>
        {songs.filter(candidate => candidate.id !== other.id).map(candidate => (
          <option value={candidate.id} key={candidate.id}>{label(candidate)}</option>
        ))}
      </select>
    </label>
  );
  const toggle = () => {
    if (!open) {
      setLeftId(left.id);
      setRightId(right.id);
    }
    setOpen(!open);
  };

  return (
    <section className="mb-3 rounded-lg border border-white/10 p-2">
      <button type="button" className="rounded px-2 py-1 text-sm font-medium hover:bg-white/10"
        aria-expanded={open} onClick={toggle}>{labels.compare}</button>
      {open && (playerIsPlaying ? <p className="px-2 py-1 text-sm text-zinc-400">{labels.pausePlayer}</p> : (
        <>
          <div className="flex flex-wrap gap-3 px-2 py-2">
            {sideSelect('A', left, right, setLeftId)}
            {sideSelect('B', right, left, setRightId)}
          </div>
          <AudioCompare key={JSON.stringify([left.id, left.audioUrl, right.id, right.audioUrl])}
            original={left.audioUrl!} processed={right.audioUrl!} labels={['A', 'B']} action={side => {
              const active = side === 'before' ? left : right;
              const preferred = playlist.preferredSongId === active.id;
              return (
                <div className="mt-2 flex flex-wrap gap-2">
                  <button type="button" disabled={preferred || saving}
                    className="rounded border border-white/15 px-2 py-1 text-sm enabled:hover:bg-white/10 disabled:opacity-60"
                    onClick={() => void mark(active)}>
                    {preferred ? labels.preferred : labels.choose}
                  </button>
                  <button type="button" className="rounded border border-white/15 px-2 py-1 text-sm hover:bg-white/10"
                    onClick={() => onOpenLibraryTake(active)}>
                    {labels.openInLibrary}
                  </button>
                </div>
              );
            }} />
          <TakeWords key={JSON.stringify([left.id, right.id, left.lyrics, left.audioUrl, right.audioUrl])} left={left} right={right} />
        </>
      ))}
    </section>
  );
};
