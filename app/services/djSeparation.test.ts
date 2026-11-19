import { afterEach, describe, expect, it, vi } from 'vitest';
import { djStemsForEngine, readDjNativeStems, startDjSeparation } from './djSeparation';

afterEach(() => vi.unstubAllGlobals());

describe('DJ native stem separation', () => {
  it('chooses real six-stem or vocals/instrumental engine outputs', () => {
    expect(djStemsForEngine('htdemucs')).toEqual(['vocals', 'drums', 'bass', 'guitar', 'piano', 'other']);
    expect(djStemsForEngine('roformer')).toEqual(['vocals', 'instrumental']);
  });

  it('uses the latest completed run instead of leftover audio from older separations', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      stems: ['vocals', 'drums', 'bass', 'instrumental'],
      run: { song_id: 'song-a', done: true, stems: ['vocals', 'instrumental'] },
    }), { status: 200 })));
    expect(await readDjNativeStems('song-a')).toEqual(['vocals', 'instrumental']);
  });

  it('configures the native separator before starting a background run', async () => {
    const calls: Array<{ url: string; options?: RequestInit }> = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, options?: RequestInit) => {
      calls.push({ url, options });
      if (url === '/v1/separation/status') return new Response(JSON.stringify({
        ready: true, engine: 'htdemucs',
        settings: { runtime: 'auto', stems: ['vocals'], engine: 'htdemucs' },
        run: null,
      }), { status: 200 });
      return new Response('{}', { status: 200 });
    }));
    await startDjSeparation('song-a', 'roformer');
    expect(calls.map(call => call.url)).toEqual([
      '/v1/separation/status', '/v1/separation/settings',
      '/v1/separation/status', '/v1/library/songs/song-a/stems',
    ]);
    expect(JSON.parse(calls[1].options?.body as string)).toMatchObject({
      engine: 'roformer', stems: ['vocals', 'instrumental'], runtime: 'auto',
    });
    expect(calls[3].options?.method).toBe('POST');
  });
});
