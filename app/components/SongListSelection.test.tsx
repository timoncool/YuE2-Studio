import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { AuthProvider } from '../context/AuthContext';
import { ConfirmDialog } from './ConfirmDialog';
import { SongList } from './SongList';
import type { Song } from '../types';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', songCount: (count: number) => String(count), t: (key: string) => key }) }));
vi.mock('./AlbumCover', () => ({ SongCover: () => null }));
vi.mock('./SongDropdownMenu', () => ({ SongDropdownMenu: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => { document.body.replaceChildren(); });

it('selects only visible finished songs and hands the selection to a cancellable delete', async () => {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const songs = [
    { id: 'original', title: 'Original' },
    { id: 'stem', title: 'Stem', derived: { from: 'original', fromTitle: 'Original', tool: 'stems' } },
    { id: 'cover', title: 'Cover', derived: { from: 'original', fromTitle: 'Original', tool: 'cover' } },
    { id: 'placeholder', title: 'Generating', isGenerating: true },
  ].map(song => ({ ...song, createdAt: new Date(), style: '', lyrics: '', audioUrl: '/song.wav' } as Song));
  const remove = vi.fn();
  const key = (event: KeyboardEvent, target: EventTarget = window) => act(() => { target.dispatchEvent(event); });
  const press = (text: string) => act(() => {
    const button = [...host.querySelectorAll('button')].find(button => button.textContent?.trim() === text);
    expect(button, text).toBeTruthy();
    button!.click();
  });
  function LibraryFixture() {
    const [asking, setAsking] = React.useState<Song[] | null>(null);
    return <AuthProvider>
      <SongList songs={songs} currentSong={null} selectedSong={null} likedSongIds={new Set()}
        isPlaying={false} onPlay={() => {}} onSelect={() => {}} onToggleLike={() => {}}
        onAddToPlaylist={() => {}} onDeleteMany={setAsking} />
      <ConfirmDialog isOpen={asking !== null} title="Confirm delete" message="Move these songs to the bin"
        onConfirm={() => { remove(asking?.map(song => song.id)); setAsking(null); }}
        onCancel={() => setAsking(null)} />
    </AuthProvider>;
  }
  try {
    await act(async () => root.render(<LibraryFixture />));
    press('select');
    const search = host.querySelector<HTMLInputElement>('input')!;
    const inSearch = new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, bubbles: true, cancelable: true });
    key(inSearch, search);
    expect(inSearch.defaultPrevented).toBe(false);
    expect(host.textContent).toContain('0 nSelected');
    const selectAll = new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, cancelable: true });
    key(selectAll);
    expect(selectAll.defaultPrevented).toBe(true);
    expect(host.textContent).toContain('2 nSelected');

    key(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    expect(host.textContent).not.toContain('nSelected');
    press('select');
    key(new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, cancelable: true }));
    press('delete');
    expect(host.textContent).toContain('Confirm delete');
    press('cancel');
    expect(remove).not.toHaveBeenCalled();

    // Expanding stems adds the visible stem to the next selection.
    press('stems · 1');
    press('select');
    key(new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, cancelable: true }));
    expect(host.textContent).toContain('3 nSelected');
    press('delete');
    press('delete');
    expect(new Set(remove.mock.calls[0][0])).toEqual(new Set(['original', 'stem', 'cover']));
  } finally {
    act(() => root.unmount());
  }
});
