import { afterEach, describe, expect, it, vi } from 'vitest';
import { ALBUM_DESCRIPTION_PREFIX, COLLECTION_DESCRIPTION_PREFIX, encodeCollectionDescription, createNativePlaylist, mapNativePlaylist, reorderNativeAlbums, updateNativePlaylist } from './nativeLibrary';

const raw = {
  id: 'collection-1',
  name: 'Demo',
  description: null as string | null,
  song_ids: ['track-1', 'track-2'],
  created_at: '2026-11-18T00:00:00Z',
  updated_at: '2026-11-18T00:00:00Z',
};

afterEach(() => vi.unstubAllGlobals());

describe('album collections in the native library', () => {
  it('recognizes albums without exposing the storage marker as the description', () => {
    const album = mapNativePlaylist({ ...raw, description: `${ALBUM_DESCRIPTION_PREFIX}First record` });
    expect(album.kind).toBe('album');
    expect(album.description).toBe('First record');
    expect(album.songIds).toEqual(['track-1', 'track-2']);

    const playlist = mapNativePlaylist({ ...raw, description: 'A normal playlist' });
    expect(playlist.kind).toBeUndefined();
    expect(playlist.description).toBe('A normal playlist');
  });

  it('preserves the album type when creating and editing its songs', async () => {
    const descriptions: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (_url: string, options: RequestInit) => {
      const body = JSON.parse(String(options.body)) as { name: string; description: string; song_ids: string[] };
      descriptions.push(body.description);
      return new Response(JSON.stringify({ ...raw, name: body.name, description: body.description, song_ids: body.song_ids }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    }));
    const created = await createNativePlaylist('Demo', 'First record', ['track-1'], 'album');
    expect(created.kind).toBe('album');
    expect(created.description).toBe('First record');

    const updated = await updateNativePlaylist(created.id, created, ['track-1', 'track-2']);
    expect(updated.kind).toBe('album');
    expect(updated.songIds).toEqual(['track-1', 'track-2']);
    expect(descriptions).toEqual([
      `${ALBUM_DESCRIPTION_PREFIX}First record`,
      `${ALBUM_DESCRIPTION_PREFIX}First record`,
    ]);
  });
  it('saves the exact order to the native album endpoint and reuses the server order', async () => {
    const writes: Array<{ url: string; ids: string[]; method?: string }> = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, options: RequestInit) => {
      writes.push({ url, ids: JSON.parse(String(options.body)).ids as string[], method: options.method });
      return new Response(JSON.stringify([
        { ...raw, id: 'c', name: 'Third', description: ALBUM_DESCRIPTION_PREFIX },
        { ...raw, id: 'a', name: 'First', description: ALBUM_DESCRIPTION_PREFIX },
        { ...raw, id: 'b', name: 'Second', description: ALBUM_DESCRIPTION_PREFIX },
      ]), { status: 200 });
    }));
    const albums = await reorderNativeAlbums(['c', 'a', 'b']);
    expect(writes).toEqual([{ url: '/v1/library/albums/order', ids: ['c', 'a', 'b'], method: 'PUT' }]);
    expect(albums.map(item => item.id)).toEqual(['c', 'a', 'b']);
    expect(albums.every(item => item.kind === 'album')).toBe(true);
  });

  it('surfaces stale album-order conflicts instead of silently losing changes', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: 'Albums changed; refresh the library' }), { status: 409 })));
    await expect(reorderNativeAlbums(['a'])).rejects.toThrow('Albums changed');
  });

  it('persists custom cover artwork for albums and playlists without breaking older records', () => {
    const image = 'data:image/jpeg;base64,YWJj';
    const albumData = encodeCollectionDescription({ kind: 'album', description: 'A concept record', coverUrl: image });
    const playlistData = encodeCollectionDescription({ description: 'A mix', coverUrl: image });
    expect(albumData?.startsWith(COLLECTION_DESCRIPTION_PREFIX)).toBe(true);
    expect(playlistData?.startsWith(COLLECTION_DESCRIPTION_PREFIX)).toBe(true);

    const album = mapNativePlaylist({ ...raw, description: albumData });
    expect(album.kind).toBe('album');
    expect(album.coverUrl).toBe(image);
    expect(album.description).toBe('A concept record');

    const playlist = mapNativePlaylist({ ...raw, description: playlistData });
    expect(playlist.kind).toBeUndefined();
    expect(playlist.coverUrl).toBe(image);
    expect(playlist.description).toBe('A mix');

    expect(encodeCollectionDescription({ ...album, coverUrl: undefined })).toBe(ALBUM_DESCRIPTION_PREFIX + 'A concept record');
    expect(mapNativePlaylist({ ...raw, description: 'Plain playlist' }).description).toBe('Plain playlist');
  });

});
