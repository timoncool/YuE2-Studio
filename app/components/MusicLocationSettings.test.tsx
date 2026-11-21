import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MusicLocationSettings } from './MusicLocationSettings';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let host: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.unstubAllGlobals();
});

describe('Music location Settings', () => {
  it('picks a folder and queues migration for the next launch', async () => {
    const calls: Array<{ url: string; body?: string }> = [];
    const current = {
      directory: 'C:/Studio/data', default_directory: 'C:/Studio/data',
      is_default: true, pending_directory: null, restart_required: false, error: null,
    };
    vi.stubGlobal('fetch', vi.fn(async (url: string, options?: RequestInit) => {
      calls.push({ url: String(url), body: typeof options?.body === 'string' ? options.body : undefined });
      if (String(url).endsWith('/pick')) return new Response(JSON.stringify({ picked: true, directory: 'D:/AI Music' }), { status: 200 });
      if (options?.method === 'PUT') return new Response(JSON.stringify({
        ...current, pending_directory: 'D:/AI Music', restart_required: true,
      }), { status: 200 });
      return new Response(JSON.stringify(current), { status: 200 });
    }));

    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => { root?.render(<MusicLocationSettings />); });
    expect(host.textContent).toContain('C:/Studio/data');

    const choose = Array.from(host.querySelectorAll('button')).find(b => b.textContent?.includes('musicLocationChoose'));
    await act(async () => { choose?.click(); });
    expect(host.textContent).toContain('D:/AI Music');

    const confirm = Array.from(host.querySelectorAll('button')).find(b => b.textContent?.includes('musicLocationConfirm'));
    await act(async () => { confirm?.click(); });
    expect(calls.some(c => c.body?.includes('"directory":"D:/AI Music"'))).toBe(true);
    expect(host.textContent).toContain('musicLocationRestart');
  });
});
