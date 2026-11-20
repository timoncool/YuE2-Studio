import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Playlist, Song } from '../types';
import { PlaylistDetail } from './PlaylistDetail';

const song: Song = {
    id: 'collection-song',
    title: 'A song to edit',
    lyrics: '',
    style: 'doo-wop',
    coverUrl: '',
    duration: '3:33',
    createdAt: new Date('2026-11-16T12:00:00Z'),
    audioUrl: '/test-song.mp3',
    tags: [],
};
const collection: Playlist = {
    id: 'collection',
    name: 'The Collection',
    kind: 'playlist',
    songIds: [song.id],
};

vi.mock('../context/I18nContext', () => ({
    useI18n: () => ({ t: (key: string) => key, songCount: (count: number) => String(count), language: 'en' }),
}));
vi.mock('../context/SongActionsContext', () => ({ useSongActions: () => ({}) }));
vi.mock('../services/studioQueries', () => ({
    libraryChanged: vi.fn(),
    useLibraryTrashedSongs: () => ({ data: { songs: [], days: 30 }, isPending: false, isSuccess: true, isError: false }),
    useLibraryPlaylists: () => ({ data: [collection], isPending: false }),
    useLibrarySongs: () => ({ data: [song], isPending: false }),
    updateLibraryPlaylists: vi.fn(),
}));
vi.mock('../services/nativeLibrary', () => ({
    parseDuration: () => 213,
    updateNativePlaylist: vi.fn(),
    deleteNativePlaylist: vi.fn(),
    restoreTrashedSong: vi.fn(async () => undefined),
}));
vi.mock('./AlbumCover', () => ({ SongCover: () => <div>Song cover</div> }));
vi.mock('./PlaylistEditor', () => ({
    PlaylistCover: () => <div>Collection cover</div>,
    PlaylistEditModal: () => null,
    PlaylistMenu: () => null,
    playlistLength: () => '3 minutes',
    playlistSongs: (playlist: Playlist, songs: Song[]) => songs.filter(item => playlist.songIds?.includes(item.id)),
    collectionSongAvailability: (playlist: Playlist, songs: Song[]) => ({
        active: songs.filter(item => playlist.songIds?.includes(item.id)),
        inBin: [],
        missingIds: [],
    }),
    reorderVisibleCollectionSongs: vi.fn(),
}));
vi.mock('./SongDropdownMenu', () => ({ SongDropdownMenu: () => null }));
vi.mock('./ConfirmDialog', () => ({ ConfirmDialog: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(() => {
    act(() => root?.unmount());
    host?.remove();
    root = null;
    host = null;
});

describe('collection song details', () => {
    it.each(['playlist', 'album'] as const)('opens song details from a %s without playing it', kind => {
        collection.kind = kind;
        const onSelect = vi.fn();
        const onPlaySong = vi.fn();
        host = document.createElement('div');
        document.body.append(host);
        root = createRoot(host);
        act(() => root?.render(
            <PlaylistDetail
                playlistId={collection.id}
                onBack={vi.fn()}
                onPlaySong={onPlaySong}
                onSelect={onSelect}
                currentSong={null}
                isPlaying={false}
                notify={vi.fn()}
                onError={vi.fn()}
            />,
        ));

        const info = host.querySelector<HTMLButtonElement>('button[aria-label="songDetails"]');
        expect(info).not.toBeNull();
        act(() => info?.click());
        expect(onSelect).toHaveBeenCalledWith(song);
        expect(onPlaySong).not.toHaveBeenCalled();

        const row = host.querySelector<HTMLLIElement>('li[data-mcp-context^="song "]');
        expect(row).not.toBeNull();
        act(() => row?.click());
        expect(onSelect).toHaveBeenCalledTimes(2);
        expect(onPlaySong).toHaveBeenCalledWith(song, [song]);
    });
});
