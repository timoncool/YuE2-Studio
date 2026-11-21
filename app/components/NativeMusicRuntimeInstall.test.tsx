// @vitest-environment happy-dom
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { I18nProvider } from '../context/I18nContext';
import { NativeMusicRuntimeInstall } from './NativeMusicRuntimeInstall';

it('shows progress across setup, reports failure, and refreshes profiles on retry', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.useFakeTimers();
  localStorage.setItem('language', 'en');
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const onInstalled = vi.fn();
  const current: { supported: boolean; installed: boolean; bytes: number;
    download?: { done: boolean; downloaded_bytes: number; total_bytes: number; error?: string } }
    = { supported: true, installed: false, bytes: 2 ** 30 };
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    expect(url).toBe('/v1/music/native/acestep-cpp/runtime');
    if (options?.method === 'POST') {
      current.download = { done: false, downloaded_bytes: 512, total_bytes: 1024 };
      return Response.json({ started: true }, { status: 202 });
    }
    return Response.json(current);
  });
  vi.stubGlobal('fetch', fetch);
  const poll = () => act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  const button = () => host.querySelector('button')!;
  try {
    await act(async () => root.render(<I18nProvider>
      <NativeMusicRuntimeInstall engine="acestep-cpp" onInstalled={onInstalled} />
    </I18nProvider>));
    expect(button().textContent).toContain('1 GiB');
    await act(async () => button().click());
    await poll();
    expect(button().disabled).toBe(true);
    expect(button().textContent).toContain('50%');

    current.download = { done: true, downloaded_bytes: 512, total_bytes: 1024, error: 'Cannot write the CUDA dependency' };
    await poll();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Cannot write the CUDA dependency');
    expect(button().disabled).toBe(false);
    expect(onInstalled).not.toHaveBeenCalled();
    await act(async () => button().click());
    current.installed = true;
    delete current.download;
    await poll();
    expect(host.textContent).toBe('');
    expect(onInstalled).toHaveBeenCalledTimes(1);
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});
