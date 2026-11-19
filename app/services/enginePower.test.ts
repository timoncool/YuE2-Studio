import { afterEach, describe, expect, it, vi } from 'vitest';
import { changeEnginePower, readEnginePower } from './enginePower';

afterEach(() => vi.unstubAllGlobals());

describe('manual engine power management', () => {
  it('reports the real paused and process-running state', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ paused: true, running: false }), { status: 200 })));
    expect(await readEnginePower()).toEqual({ paused: true, running: false });
  });

  it('stops and restarts through the native service without touching library audio', async () => {
    const calls: Array<{ path: string; options: RequestInit }> = [];
    vi.stubGlobal('fetch', vi.fn(async (path: string, options: RequestInit) => {
      calls.push({ path, options });
      const requested: { running: boolean } = JSON.parse(options.body as string);
      return new Response(JSON.stringify({ paused: !requested.running, running: requested.running }), { status: 200 });
    }));
    expect(await changeEnginePower(false)).toEqual({ paused: true, running: false });
    expect(await changeEnginePower(true)).toEqual({ paused: false, running: true });
    expect(calls.map(call => JSON.parse(call.options.body as string))).toEqual([{ running: false }, { running: true }]);
    expect(calls.every(call => call.path === '/engine/power' && call.options.method === 'POST')).toBe(true);
  });

  it('propagates a refusal to stop a running generation rather than pretending VRAM was freed', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ error: 'A song is still running' }), { status: 409 })));
    await expect(changeEnginePower(false)).rejects.toThrow('A song is still running');
  });
});
