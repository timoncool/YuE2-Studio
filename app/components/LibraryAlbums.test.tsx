import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import type { Playlist, Song } from '../types';
import { LibraryView } from './LibraryView';
import { AuthProvider } from '../context/AuthContext';
import { moveAlbum } from '../services/albumOrder';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ language: 'en', songCount: (n: number) => String(n), t: (key: string) => key }),
}));
vi.mock('../services/studioQueries', async original => ({
  ...(await original<typeof import('../services/studioQueries')>()),
  useLibraryTrashedSongs: () => ({}),
}));
vi.mock('./SongDropdownMenu', () => ({ SongDropdownMenu: () => null }));
vi.mock('./LegacyLibraryPreview', () => ({ LegacyLibraryPreview: () => null }));
vi.mock('./AlbumCover', () => ({ SongCover: () => null, AlbumCover: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('moves only albums with focused controls and drag-and-drop, and still finds unassigned songs', async () => {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  sessionStorage.setItem('studio.library.tab', 'albums');
  const rows = [
    { id: 'a', name: 'First record', kind: 'album', songIds: ['assigned'] },
    { id: 'list', name: 'A playlist', songIds: ['assigned'] },
    { id: 'b', name: 'Second record', kind: 'album', songIds: [] },
  ] as Playlist[];
  const tracks = [
    { id: 'assigned', title: 'Assigned sample' }, { id: 'free', title: 'Unassigned sample' },
  ].map(song => ({ ...song, audioUrl: '/music.wav', style: '', lyrics: '', createdAt: new Date() } as Song));
  const reorder = vi.fn();
  const create = vi.fn();
  const noAction = () => undefined;
  function Fixture() {
    const [playlists, setPlaylists] = useState(rows);
    return <AuthProvider><LibraryView allSongs={tracks} likedSongs={[]} playlists={playlists}
      onReorderAlbums={async (from, to) => { reorder(from, to); setPlaylists(list => moveAlbum(list, from, to)); }}
      onCreateAlbum={create} onCreatePlaylist={noAction} onPlaySong={noAction} onPlayPlaylist={noAction}
      currentSong={null} isPlaying={false} onSelectPlaylist={noAction} onError={noAction} onToast={noAction}
      likedSongIds={new Set()} onToggleLike={noAction} onAddToPlaylist={noAction} onDeleteMany={noAction}
      onMidiMany={noAction} selectedSong={null} onSelect={noAction} onShowDetails={noAction} detailsSongId={null} />
    </AuthProvider>;
  }
  const cards = () => [...host.querySelectorAll<HTMLElement>('[data-mcp-context^="playlist "]')];
  const names = () => cards().map(card => card.querySelector('h3')?.textContent);
  const press = (label: string) => act(() => {
    const button = [...host.querySelectorAll('button')]
      .find(item => item.textContent?.trim().startsWith(label) || item.getAttribute('aria-label') === label);
    expect(button, label).toBeTruthy();
    button!.click();
  });
  try {
    await act(async () => root.render(<Fixture />));
    expect(names()).toEqual(['First record', 'Second record']);
    press('newAlbum');
    expect(create).toHaveBeenCalledOnce();
    const move = host.querySelector<HTMLButtonElement>('[aria-label="moveAlbumEarlier: Second record"]')!;
    move.focus();
    expect(document.activeElement).toBe(move);
    await act(async () => move.click());
    expect(reorder).toHaveBeenLastCalledWith('b', 'a');
    expect(names()).toEqual(['Second record', 'First record']);

    const [from, to] = cards();
    const transfer = new DataTransfer();
    const drag = (target: HTMLElement, type: string) => {
      const event = Object.assign(new Event(type, { bubbles: true, cancelable: true }), { dataTransfer: transfer });
      target.dispatchEvent(event);
    };
    act(() => drag(from, 'dragstart'));
    act(() => drag(to, 'dragover'));
    await act(async () => drag(to, 'drop'));
    expect(reorder).toHaveBeenLastCalledWith('b', 'a');
    expect(names()).toEqual(['First record', 'Second record']);

    press('allSongs');
    press('filters');
    press('unassignedFilter');
    expect(host.textContent).toContain('Unassigned sample');
    expect(host.textContent).not.toContain('Assigned sample');
  } finally {
    act(() => root.unmount());
    host.remove();
    sessionStorage.removeItem('studio.library.tab');
  }
});



