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
    useSignedUpdateNotice(true, visible, announce);
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
