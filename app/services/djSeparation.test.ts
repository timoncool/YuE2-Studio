import { afterEach, describe, expect, it, vi } from 'vitest';
import { djEngineReady, installDjSeparationEngine, readDjNativeStems, startDjSeparation, type DjSeparationStatus } from './djSeparation';

afterEach(() => vi.unstubAllGlobals());

describe('DJ native stem separation', () => {
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

  it('avoids mixing stems left by different engines after restart', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => Response.json({
      stems: ['vocals', 'drums', 'bass', 'instrumental'], run: null,
    })));
    expect(await readDjNativeStems('song-a')).toEqual([]);
  });

  it('chooses the engine for one separation without changing the saved settings', async () => {
    const fetcher = vi.fn(async (_url: string, _options?: RequestInit) => Response.json({ started: true }));
    vi.stubGlobal('fetch', fetcher);
    await startDjSeparation('song-a', 'roformer');
    expect(fetcher).toHaveBeenCalledTimes(1);
    const [url, options] = fetcher.mock.calls[0];
    expect(url).toBe('/v1/library/songs/song-a/stems');
    expect(options?.method).toBe('POST');
    expect(JSON.parse(String(options?.body))).toEqual({ engine: 'roformer' });
  });
});
