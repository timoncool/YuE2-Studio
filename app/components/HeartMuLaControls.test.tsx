import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { AudioCppModelInstall } from './HeartMuLaControls';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.hoisted(() => vi.stubGlobal('location', { protocol: 'https:', hostname: 'tauri.localhost' }));

it.each([
  ['heartmula', '/v1/music/audio-cpp/models', '/v1/music/audio-cpp/models/install', '/v1/music/audio-cpp/models/install'],
  ['hotstep-ace-step-1.5', '/v1/music/native/hotstep/profiles', '/v1/music/native/hotstep/ace-step-1.5/install', '/v1/music/native/hotstep/install'],
])('cancels the active %s model and can start it again', async (model, catalogue, installPath, cancelPath) => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  let installing = false;
  let cancelled = false;
  const mutations: string[] = [];
  vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
    const path = new URL(url).pathname;
    if (init?.method === 'POST' || init?.method === 'DELETE') {
      expect(JSON.parse(init.body as string)).toEqual({ model });
      mutations.push(`${init.method} ${path}`);
      installing = init.method === 'POST';
      cancelled = !installing;
      return Response.json(installing ? { installing } : { cancelled });
    }
    expect(path).toBe(catalogue);
    const rows = [{ id: model, ready: false, installing, download: {
      downloaded_bytes: 40, total_bytes: 100, done: !installing, error: cancelled ? 'cancelled' : undefined,
    } }];
    return Response.json({ [catalogue.endsWith('/profiles') ? 'profiles' : 'models']: rows });
  });
  const host = document.createElement('div');
  const root = createRoot(host);
  const press = (label: string) => act(async () =>
    [...host.querySelectorAll('button')].find(button => button.textContent?.includes(label))!.click());
  try {
    await act(async () => root.render(<AudioCppModelInstall model={model} bytes={0} language="en" onInstalled={vi.fn()}
      target={{ catalogue, installPath, label: model }} />));
    await press('Install ');
    expect(host.textContent).toContain('40%');
    await press('cancelDownload');
    expect(host.textContent).toContain('cancelled');
    await press('Install ');
    expect(host.textContent).toContain('40%');
    expect(mutations).toEqual([`POST ${installPath}`, `DELETE ${cancelPath}`, `POST ${installPath}`]);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});

it('installs a model identified by a compatible server catalogue route', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const installPath = '/v1/music/native/hotstep/new-model/install';
  const catalogue = '/v1/music/native/hotstep/profiles';
  const model = 'new-compatible-model';
  const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') {
      expect(url).toMatch(/\/v1\/music\/native\/hotstep\/new-model\/install$/);
      expect(JSON.parse(init.body as string)).toEqual({ model });
      return Response.json({ installing: true });
    }
    expect(url).toMatch(/\/v1\/music\/native\/hotstep\/profiles$/);
    return Response.json({ profiles: [{ id: model, ready: false }] });
  });
  vi.stubGlobal('fetch', fetcher);
  const host = document.createElement('div');
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AudioCppModelInstall model={model} bytes={0} language="en" onInstalled={vi.fn()}
      target={{ catalogue, installPath, label: 'New compatible model' }} />));
    expect(host.querySelector('button')?.textContent).toBe('Install New compatible model');
    await act(async () => host.querySelector('button')!.click());
    expect(fetcher.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(1);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});

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
    await act(async () => root.render(<AudioCppModelInstall model="heartmula" bytes={0} language="en" onInstalled={finished} />));
    await act(async () => host.querySelector('button')!.click());
    expect(host.textContent).toContain('40%');
    await act(async () => root.unmount());
    root = createRoot(host);
    await act(async () => root.render(<AudioCppModelInstall model="heartmula" bytes={0} language="en" onInstalled={finished} />));
    expect(host.querySelector('button')?.disabled).toBe(true);
    expect(host.textContent).toContain('40%');
    expect(starts).toBe(1);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
