import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { expect, it, vi } from 'vitest';
import { signedUpdateNoticeChanged, useSignedUpdateNotice } from './signedUpdateNotice';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('announces newly found updates once in notify mode and keeps auto and empty checks quiet', async () => {
  let mode = 'auto';
  const change = { kind: 'music-engines', id: 'worker', from_version: '1', to_version: '2', automatic_updates_paused: false };
  let updates = [change];
  const fetcher = vi.fn(async (_path: string) => Response.json({ mode, updates }));
  vi.stubGlobal('fetch', fetcher);
  const announce = vi.fn();
  const client = new QueryClient();
  function Notice({ visible }: { visible: boolean }) {
    useSignedUpdateNotice(true, 'studio:0', visible, announce);
    return null;
  }
  const container = document.createElement('div');
  const root = createRoot(container);
  const render = async (visible: boolean) => {
    await act(async () => root.render(<QueryClientProvider client={client}><Notice visible={visible} /></QueryClientProvider>));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
  };
  const refresh = async () => {
    await act(async () => { signedUpdateNoticeChanged(); await new Promise(resolve => setTimeout(resolve, 10)); });
  };
  try {
    await render(true);
    expect(announce).not.toHaveBeenCalled();
    updates = [];
    await refresh();
    expect(announce).not.toHaveBeenCalled();
    mode = 'notify';
    await refresh();
    expect(announce).not.toHaveBeenCalled();
    await render(false);
    updates = [change, { ...change, id: 'paused', automatic_updates_paused: true }];
    await refresh();
    expect(announce).not.toHaveBeenCalled();
    await render(true);
    expect(announce).toHaveBeenCalledTimes(1);
    await refresh();
    expect(announce).toHaveBeenCalledTimes(1);
    updates = [{ ...change, to_version: '3' }];
    await refresh();
    expect(announce).toHaveBeenCalledTimes(2);
    mode = 'manual';
    updates = [{ ...change, to_version: '4' }];
    await refresh();
    expect(announce).toHaveBeenCalledTimes(2);
    expect(fetcher.mock.calls.every(([path]) => path === '/v1/components/updates')).toBe(true);
  } finally {
    act(() => root.unmount());
    client.clear();
    vi.unstubAllGlobals();
  }
});

it('reads the plan once per server revision, even across many setup polls', async () => {
  vi.useFakeTimers();
  const packageUpdate = { kind: 'music-models', id: 'weights', from_version: '1', to_version: '2' };
  let updates: typeof packageUpdate[] = [];
  const fetcher = vi.fn(async () => Response.json({ mode: 'notify', updates }));
  vi.stubGlobal('fetch', fetcher);
  const announce = vi.fn();
  const client = new QueryClient();
  const host = document.createElement('div');
  const root = createRoot(host);
  function Notice({ revision }: { revision: string }) {
    useSignedUpdateNotice(true, revision, true, announce);
    return null;
  }
  const render = async (revision: string) => {
    await act(async () => root.render(<QueryClientProvider client={client}><Notice revision={revision} /></QueryClientProvider>));
    await act(async () => { await vi.advanceTimersByTimeAsync(5); });
  };
  try {
    await render('studio:0');
    expect(fetcher).toHaveBeenCalledTimes(1);
    for (let tick = 0; tick < 30; tick++) {
      await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
      await render('studio:0');
    }
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(announce).not.toHaveBeenCalled();
    updates = [packageUpdate];
    await render('studio:1');
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(announce).toHaveBeenCalledTimes(1);
    await render('studio:1');
    expect(fetcher).toHaveBeenCalledTimes(2);
  } finally {
    act(() => root.unmount());
    client.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});
