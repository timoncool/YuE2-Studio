import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { expect, it, vi } from 'vitest';
import { EnginesAndUpdates } from './EnginesAndUpdates';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ t: (key: string) => key, language: 'en' }),
}));
vi.mock('./UpdateScreen', () => ({ UpdatePlanView: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it.each(['acestep-cpp', 'minimaxmusic-cpp'])('installs a verified Windows first-party package for %s and refreshes its selected version', async engine => {
  const localEngine = engine === 'acestep-cpp' ? 'minimaxmusic-cpp' : 'acestep-cpp';
  const installPath = `/v1/components/music-engines/${engine}/install`;
  const progressPath = `/v1/components/packages/music-engines/${engine}`;
  const platform = 'windows-x86_64';
  const entry = { id: engine, name: 'ACE-Step', version: 'signed-2', platform,
    protocol: 'music-native-windows-v2', native_runtime: { api_version: 2 }, files: [{ bytes: 2 ** 30 }] };
  let verified = false;
  let installed = false;
  let externalTransfer = false;
  let finish: ((response: Response) => void) | undefined;
  const request = vi.fn(async (url: string, options?: RequestInit) => {
    if (url === '/v1/components/music-engines') return Response.json(verified
      ? { source: 'signed', platform, engines: [entry, { ...entry, id: localEngine, version: 'incompatible', platform: 'linux-x86_64' }] }
      : { source: 'bundled', platform, engines: [] });
    if (url === '/v1/components/music-engines/refresh') {
      expect(options?.method).toBe('POST');
      verified = true;
      return Response.json({ source: 'signed', platform, engines: [entry] });
    }
    if (url === `/v1/music/native/${engine}/profiles`) return Response.json({
      engine_installed: installed, profiles: [],
      selected_executable: installed ? { source: 'signed', path: 'C:/signed/ace-server.exe', package: { version: 'signed-2' } } : null,
    });
    if (url === `/v1/music/native/${localEngine}/profiles`) return Response.json({
      engine_installed: true, selected_executable: { source: 'local', path: 'C:/local/mm-server.exe' },
    });
    if (url.endsWith('/runtime')) return Response.json({ supported: true, revision: 'baked' });
    if (url === installPath) {
      expect(options?.method).toBe('POST');
      expect(options?.body).toBeUndefined();
      return new Promise<Response>(resolve => {
        finish = response => { finish = undefined; resolve(response); };
      });
    }
    if (url === `${progressPath}/progress`) return Response.json({
      active: finish || externalTransfer ? { stage: 'downloading', downloaded_bytes: 25, reused_bytes: 25, total_bytes: 100 } : null,
    });
    if (url === `${progressPath}/cancel`) {
      expect(options?.method).toBe('POST');
      externalTransfer = false;
      finish?.(Response.json({ error: 'Package installation cancelled' }, { status: 409 }));
      return Response.json({ cancel_requested: true });
    }
    return Response.json({});
  });
  vi.stubGlobal('fetch', request);
  const host = document.createElement('div');
  const root = createRoot(host);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const settle = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); });
  const mount = () => act(async () => root.render(<QueryClientProvider client={client}>
    <EnginesAndUpdates focus={engine} onNavigate={vi.fn()} />
  </QueryClientProvider>));
  const poll = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 450)); });
  const remount = async () => {
    await act(async () => root.render(null));
    await mount();
    await settle();
  };
  try {
    await mount();
    await settle();
    expect(verified).toBe(false);
    await act(async () => [...host.querySelectorAll('button')].find(button => button.textContent === 'refresh')!.click());
    await settle();
    let card = host.querySelector(`[data-engine="${engine}"]`)!;
    const install = () => [...card.querySelectorAll('button')].find(button => button.textContent?.includes('Install · signed-2'))!;
    await settle();
    expect(install().textContent).toContain('1 GiB');
    expect(card.textContent).not.toContain('baked');
    expect(host.querySelector(`[data-engine="${localEngine}"]`)?.textContent).toContain('C:/local/mm-server.exe');
    expect(host.querySelector(`[data-engine="${localEngine}"]`)?.textContent).not.toContain('signed-2');

    await act(async () => install().click());
    await poll();
    expect(card.querySelector('[role="status"]')?.textContent).toContain('50%');
    await remount();
    card = host.querySelector(`[data-engine="${engine}"]`)!;
    expect(card.querySelector('[role="status"]')?.textContent).toContain('50%');
    expect(install().disabled).toBe(true);
    await act(async () => [...card.querySelectorAll('button')].find(button => button.textContent === 'cancelDownload')!.click());
    await settle();
    expect(card.querySelector('[role="alert"]')).toBeNull();
    expect(install().disabled).toBe(false);
    const probes = () => request.mock.calls.filter(([url]) => url === `${progressPath}/progress`).length;
    const idleProbes = probes();
    await poll();
    expect(probes()).toBe(idleProbes);

    await act(async () => install().click());
    await act(async () => finish!(Response.json({ error: 'Digest mismatch' }, { status: 422 })));
    expect(card.querySelector('[role="alert"]')?.textContent).toBe('Digest mismatch');
    await act(async () => install().click());
    await act(async () => finish!(Response.json({ error: 'Package installation cancelled' }, { status: 409 })));
    externalTransfer = true;
    await remount();
    card = host.querySelector(`[data-engine="${engine}"]`)!;
    expect(card.querySelector('[role="status"]')?.textContent).toContain('50%');
    installed = true;
    externalTransfer = false;
    await poll();
    await settle();
    expect(card.textContent).toContain('Managed package · signed-2 · C:/signed/ace-server.exe');
    expect(install()).toBeUndefined();
    expect(request.mock.calls.filter(([url]) => url.endsWith(`/${engine}/runtime`))
      .every(([, options]) => !options?.method || options.method === 'GET')).toBe(true);
  } finally {
    await act(async () => root.unmount());
    client.clear();
    vi.unstubAllGlobals();
  }
});
