import { afterEach, describe, expect, it, vi } from 'vitest';
import { djEngineReady, djStemsForEngine, installDjSeparationEngine, readDjNativeStems, startDjSeparation, type DjSeparationStatus } from './djSeparation';

afterEach(() => vi.unstubAllGlobals());

describe('DJ native stem separation', () => {
  it('chooses real six-stem or vocals/instrumental engine outputs', () => {
    expect(djStemsForEngine('htdemucs')).toEqual(['vocals', 'drums', 'bass', 'guitar', 'piano', 'other']);
    expect(djStemsForEngine('roformer')).toEqual(['vocals', 'instrumental']);
  });

  it('reports engine-specific readiness without confusing the selected engine with the other', () => {
    const status = {
      ready: false, engine: 'roformer', settings: { runtime: 'auto', stems: ['vocals', 'instrumental'] },
      run: null, engines: { htdemucs: { ready: true }, roformer: { ready: false } },
    } as DjSeparationStatus;
    expect(djEngineReady(status, 'htdemucs')).toBe(true);
    expect(djEngineReady(status, 'roformer')).toBe(false);
    expect(djEngineReady(null, 'roformer')).toBe(false);
  });

  it('one-click installs the chosen native engine using the existing backend route', async () => {
    const requests: Array<{ url: string; options?: RequestInit }> = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, options?: RequestInit) => {
      requests.push({ url, options });
      return new Response(JSON.stringify({ started: true }), { status: 200 });
    }));
    await installDjSeparationEngine('roformer');
    expect(requests).toHaveLength(1);
    expect(requests[0].url).toBe('/v1/separation/runtime/install');
    expect(requests[0].options?.method).toBe('POST');
    expect(JSON.parse(requests[0].options?.body as string)).toEqual({ asset_id: 'roformer' });
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
        ready: true, engine: 'roformer',
        engines: { htdemucs: { ready: false }, roformer: { ready: true } },
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
