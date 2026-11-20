import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import type { Playlist, Song } from '../types';

const reads = vi.hoisted(() => ({ playlists: [] as Playlist[], songs: [] as Song[] }));

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ language: 'en', t: (key: string) => key, songCount: (count: number) => String(count) }),
}));
vi.mock('../services/studioQueries', () => ({
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
const { SongActionsProvider } = await import('../context/SongActionsContext');

it('continues and reuses a take in the open playlist and only removes its membership', async () => {
  const take = (id: string): Song => ({
    id, title: 'Same title', style: 'Folk', lyrics: 'Verse', coverUrl: '', duration: '0:20',
    createdAt: new Date(0), tags: [], audioUrl: `/v1/library/media/${id}`,
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
  const render = () => act(() => root.render(
    <SongActionsProvider value={{ reusePrompt: globalReuse, remove: removeFromLibrary }}>
      <PlaylistDetail playlistId="work" onBack={() => {}} onPlaySong={() => {}} onSelect={() => {}}
        onCreateHere={create} onReuseHere={reuseHere} currentSong={null} isPlaying={false}
        notify={() => {}} onError={onError} />
    </SongActionsProvider>,
  ));
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
    await click(row(duplicate).querySelector('button[aria-label="removeFromPlaylist"]')!);
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
