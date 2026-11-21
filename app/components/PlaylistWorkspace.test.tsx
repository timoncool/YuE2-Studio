import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import type { Playlist, Song } from '../types';

const reads = vi.hoisted(() => ({ playlists: [] as Playlist[], songs: [] as Song[] }));

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ language: 'en', t: (key: string) => key, songCount: (count: number) => String(count) }),
}));
vi.mock('../context/AuthContext', () => ({ useAuth: () => ({ user: null }) }));
vi.mock('./KaraokeAction', () => ({ KaraokeAction: () => null }));
vi.mock('../services/adapters', () => ({ useAdapterLibrary: () => ({ installed: [], slots: [] }), usesFromSettings: () => [], localized: () => '' }));
vi.mock('../services/studioQueries', () => ({
  useKaraokeStatus: () => ({ data: { ready: false } }),
  useLibraryPlaylists: () => ({ data: reads.playlists }),
  useLibrarySongs: () => ({ data: reads.songs }),
  updateLibraryPlaylists: (change: (playlists: Playlist[]) => Playlist[]) => { reads.playlists = change(reads.playlists); },
}));
vi.mock('./AlbumCover', () => ({ AlbumCover: () => null, SongCover: () => null }));
vi.mock('./SongDropdownMenu', async () => {
  const { useSongActions } = await import('../context/SongActionsContext');
  return { SongDropdownMenu: ({ song }: { song: Song }) => {
    const actions = useSongActions();
    return <button onClick={() => actions.reusePrompt?.(song)}>reusePrompt</button>;
  } };
});

const { PlaylistDetail } = await import('./PlaylistDetail');
const { RightSidebar } = await import('./RightSidebar');
const { SongActionsProvider } = await import('../context/SongActionsContext');

vi.mock('./AudioWaveform', () => ({ AudioWaveform: () => null }));

it('marks the auditioned side as preferred and restores the badge from the library read', async () => {
  const first = { id: 'first', title: 'Same title', tags: [], createdAt: new Date(0), style: '', duration: '0:10', audioUrl: '/v1/library/media/one.wav' } as Song;
  const second = { ...first, id: 'second', audioUrl: '/v1/library/media/two.wav' };
  const stored = { id: 'work', name: 'Drafts', song_ids: [first.id, second.id] };
  reads.songs = [first, second];
  reads.playlists = [{ id: 'work', name: 'Drafts', songIds: stored.song_ids }];
  const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'PUT') return Response.json({ preferred_song_id: 'first', changed: true });
    return Response.json([{ ...stored, preferred_song_id: 'first' }]);
  });
  vi.stubGlobal('fetch', fetcher);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  const root = createRoot(host);
  const noOp = () => {};
  const openTake = vi.fn();
  const render = (key: string) => act(() => root.render(
    <PlaylistDetail key={key} playlistId="work" onBack={noOp} onPlaySong={noOp} onCreateHere={noOp}
      onReuseHere={noOp} onSelect={noOp} onOpenLibraryTake={openTake} currentSong={null} isPlaying={false} notify={noOp} onError={noOp} />,
  ));
  const button = (label: string) => Array.from(host.querySelectorAll('button'))
    .find(element => element.textContent?.trim() === label)!;
  const row = (id: string) => host.querySelector(`[data-mcp-context="song ${id}: Same title"]`)!;
  try {
    render('before');
    await act(async () => button('Compare takes').click());
    await act(async () => button('Open in Library').click());
    expect(openTake).toHaveBeenCalledWith(reads.songs[1]);
    const [a, b] = Array.from(host.querySelectorAll('audio'));
    expect(a.muted).toBe(true);
    Object.defineProperty(a, 'readyState', { value: 1 });
    b.currentTime = 3.25;
    await act(async () => button('A').click());
    expect(a.muted).toBe(false);
    expect(b.muted).toBe(true);
    expect(a.currentTime).toBe(3.25);
    await act(async () => button('Open in Library').click());
    expect(openTake).toHaveBeenLastCalledWith(first);
    await act(async () => button('Mark as preferred take').click());
    expect(fetcher).toHaveBeenCalledWith('/v1/library/playlists/work/preferred-take', expect.objectContaining({
      method: 'PUT', body: JSON.stringify({ song_id: first.id }),
    }));
    const { loadNativePlaylists } = await import('../services/nativeLibrary');
    reads.playlists = await loadNativePlaylists();
    render('reopened');
    expect(row(first.id).textContent).toContain('Preferred take');
    expect(row(second.id).textContent).not.toContain('Preferred take');
    expect(reads.playlists[0].songIds).toEqual([first.id, second.id]);
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});

it('opens the selected take’s songbook in its workspace and only removes its membership', async () => {
  const take = (id: string): Song => ({
    id, title: 'Same title', style: 'Folk', lyrics: 'Verse', coverUrl: '', duration: '0:20',
    createdAt: new Date(0), tags: [], audioUrl: `/v1/library/media/${id}`,
    generationParams: { abc: id === 'first'
      ? 'X:1\nM:4/4\nL:1/4\nK:C\n"C"C D E G|\nw:Or-i-gi-nal'
      : 'X:1\nM:4/4\nL:1/4\nK:D\n"D"D E F A|\nw:Dif-fer-ent take' },
  });
  const source = take('first');
  const duplicate = take('second');
  reads.songs = [source, duplicate];
  reads.playlists = [{ id: 'work', name: 'Song workspace', songIds: [source.id] } as Playlist];
  const create = vi.fn();
  const reuseHere = vi.fn();
  const globalReuse = vi.fn();
  const removeFromLibrary = vi.fn();
  const onError = vi.fn();
  const fetcher = vi.fn(async (_url: string, init?: RequestInit) => {
    const request = JSON.parse(String(init?.body));
    return Response.json({ id: 'work', name: 'Song workspace', song_ids: request.song_ids });
  });
  const node = document.createElement('div');
  document.body.append(node);
  const root = createRoot(node);
  vi.stubGlobal('fetch', fetcher);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  function Workspace() {
    const [selected, select] = useState<Song | null>(null);
    return (
      <SongActionsProvider value={{ reusePrompt: globalReuse, remove: removeFromLibrary }}>
        <PlaylistDetail playlistId="work" onBack={() => {}} onPlaySong={() => {}} onSelect={select} onOpenLibraryTake={select}
          onCreateHere={create} onReuseHere={reuseHere} currentSong={null} isPlaying={false}
          notify={() => {}} onError={onError} />
        <RightSidebar song={selected} />
      </SongActionsProvider>
    );
  }
  const render = () => act(() => root.render(<Workspace />));
  const exactButton = (label: string) => [...node.querySelectorAll('button')].find(button => button.textContent?.trim() === label)!;
  const click = (button: HTMLButtonElement) => act(async () => button.click());
  const row = (song: Song) => node.querySelector(`[data-mcp-context="song ${song.id}: ${song.title}"]`)!;

  try {
    render();
    await click(exactButton('create'));
    expect(create).toHaveBeenCalledOnce();
    await click(exactButton('addSongs'));
    await click(exactButton('add'));
    expect(reads.playlists[0].songIds).toEqual([source.id, duplicate.id]);
    render();
    await click(row(duplicate).querySelector('button[aria-label="moreActions"]')!);
    await click(exactButton('reusePrompt'));
    expect(reuseHere).toHaveBeenCalledWith(duplicate);
    expect(globalReuse).not.toHaveBeenCalled();

    await act(async () => row(duplicate).dispatchEvent(new MouseEvent('click', { bubbles: true })));
    await click(exactButton('Songbook'));
    const sheet = () => document.querySelector<HTMLDialogElement>('dialog[aria-label="Songbook"]')!;
    expect(sheet().open).toBe(true);
    expect(sheet().querySelector('strong')?.textContent).toBe('D');
    expect(sheet().textContent).toContain('Dif-');
    await click(sheet().querySelector('button[aria-label="Close songbook"]')!);

    await click(row(duplicate).querySelector('button[aria-label="removeFromPlaylist"]')!);
    render();
    expect(row(duplicate)).toBeNull();
    await act(async () => row(source).dispatchEvent(new MouseEvent('click', { bubbles: true })));
    await click(exactButton('Songbook'));
    expect(sheet().querySelector('strong')?.textContent).toBe('C');
    expect(sheet().textContent).toContain('Or-');
    expect(sheet().textContent).not.toContain('Dif-');
    expect(reads.playlists[0].songIds).toEqual([source.id]);
    expect(reads.songs.map(song => song.audioUrl)).toEqual([source.audioUrl, duplicate.audioUrl]);
    expect(removeFromLibrary).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
    expect(fetcher.mock.calls.map(([url, init]) => [url, init?.method])).toEqual([
      ['/v1/library/playlists/work', 'PUT'], ['/v1/library/playlists/work', 'PUT'],
    ]);
  } finally {
    act(() => root.unmount());
    node.remove();
    vi.unstubAllGlobals();
  }
});
