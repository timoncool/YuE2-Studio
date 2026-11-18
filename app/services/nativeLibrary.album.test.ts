import { afterEach, describe, expect, it, vi } from 'vitest';
import { ALBUM_DESCRIPTION_PREFIX, createNativePlaylist, mapNativePlaylist, updateNativePlaylist } from './nativeLibrary';

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
});
