import { describe, expect, it, vi } from 'vitest';
import { API_BASE } from './apiBase';
import { mapNativeLibrarySong, trashNativeSongs } from './nativeLibrary';

describe('mapNativeLibrarySong', () => {
  it('uses the native media route only when the library record has audio', () => {
    const withAudio = mapNativeLibrarySong({
      id: 'song id/with space',
      title: 'Native track',
      audio_path: 'outputs/native-track.mp3',
      caption: 'warm synthpop',
      lyrics: 'one line',
      metadata: {},
      generation_settings: {},
      engine_id: 'native-engine',
      profile_id: 'recommended',
      replay_request: { caption: 'warm synthpop' },
      audio_codes: [1, 2, 3],
      created_at: '1',
    });
    const withoutAudio = mapNativeLibrarySong({
      id: 'silent-song',
      title: 'No audio yet',
      audio_path: null,
      caption: '',
      lyrics: '',
      metadata: {},
      generation_settings: {},
      engine_id: 'native-engine',
      created_at: '1',
    });

    expect(withAudio.audioUrl).toBe(`${API_BASE}/v1/library/media/song%20id%2Fwith%20space`);
    const switched = mapNativeLibrarySong({
      id: 'a', title: 'A', audio_path: 'a.wav', caption: '', lyrics: '', engine_id: 'e', created_at: '1', updated_at: '7',
      metadata: { active_version: 'v1', audio_versions: [{ id: 'v1', label: 'Mastered', created_at: '7' }] },
    });
    // a switched version changes the address, so the browser fetches it again
    expect(switched.audioUrl).toBe(`${API_BASE}/v1/library/media/a?v=7`);
    expect(switched.activeVersion).toBe('v1');
    expect(switched.audioVersions?.[0].label).toBe('Mastered');
    expect(withAudio.nativeReplayAvailable).toBe(true);
    expect(withoutAudio.audioUrl).toBeUndefined();
    expect(withoutAudio.nativeReplayAvailable).toBe(false);
  });
});

it('undoes the original group once and keeps independently selected takes separate', async () => {
  const calls = vi.fn(async (url: string) => Response.json({
    moved_ids: url.endsWith('/original') ? ['original', 'stem', 'hidden-stem'] : ['cover'],
  }));
  vi.stubGlobal('fetch', calls);
  try {
    const moved = await trashNativeSongs([
      { id: 'stem', derived: { from: 'original', fromTitle: 'Original', tool: 'stems' } },
      { id: 'original' },
      { id: 'cover', derived: { from: 'original', fromTitle: 'Original', tool: 'cover' } },
    ]);
    expect(calls.mock.calls.map(([path]) => path)).toEqual([
      '/v1/library/songs/original', '/v1/library/songs/cover',
    ]);
    expect(moved.removed).toEqual(new Set(['original', 'stem', 'hidden-stem', 'cover']));
    expect(moved.restore).toEqual(['original', 'cover']);
    expect(moved.failed).toEqual([]);
  } finally {
    vi.unstubAllGlobals();
  }
});
