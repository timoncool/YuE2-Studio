import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { HeartMuLaInstall } from './HeartMuLaControls';

vi.hoisted(() => vi.stubGlobal('location', { protocol: 'https:', hostname: 'tauri.localhost' }));

it('uses the desktop service address and observes the same installation after remount', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  let installing = false;
  let starts = 0;
  const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
    expect(url).toMatch(/^http:\/\/127\.0\.0\.1:8791\/v1\/music\/audio-cpp\/models/);
    if (init?.method === 'POST') {
      starts++;
      installing = true;
      return Response.json({ started: true, installing: true });
    }
    return Response.json({models: [{id: 'heartmula', ready: false, installing,
      download: installing ? {downloaded_bytes: 40, total_bytes: 100, done: false} : null}]});
  });
  vi.stubGlobal('fetch', fetcher);
  const host = document.createElement('div');
  const finished = vi.fn();
  let root = createRoot(host);
  try {
    await act(async () => root.render(<HeartMuLaInstall bytes={0} language="en" onInstalled={finished} />));
    await act(async () => host.querySelector('button')!.click());
    expect(host.textContent).toContain('40%');
    await act(async () => root.unmount());
    root = createRoot(host);
    await act(async () => root.render(<HeartMuLaInstall bytes={0} language="en" onInstalled={finished} />));
    expect(host.querySelector('button')?.disabled).toBe(true);
    expect(host.textContent).toContain('40%');
    expect(starts).toBe(1);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
