import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { OptionalGroup, SetupGate, WHERE_IT_RUNS } from './SetupGate';
import { SettingsModal } from './SettingsModal';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { engineUpdateTarget } from '../i18n/enginePage';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({
  t: (key: string) => key === 'ramShort' ? 'Have {have} GB, need {need} GB' : key,
  language: 'en',
}) }));
vi.mock('../context/AuthContext', () => ({ useAuth: () => ({ user: { username: 'test' }, setDisplayName: vi.fn() }) }));
vi.mock('../services/studioQueries', () => ({ openRouterSettingsChanged: vi.fn(), useSetupStatus: () => ({}),
  useOpenRouterSettings: () => ({ data: { configured: false } }), SELF_READ: { 'x-hub-skip': '1' } }));
vi.mock('./EngineSettings', () => ({ EngineSettings: () => null }));
vi.mock('./ModelPreset', () => ({ ModelPreset: () => null }));
vi.mock('./ModelStorageCard', () => ({ ModelStorageCard: () => null }));
vi.mock('./OwnModelPanel', () => ({ OwnModelPanel: () => null }));
vi.mock('./AssistantSettings', () => ({ AssistantExtras: () => null }));
vi.mock('./KaraokeSettings', () => ({ KaraokeExtras: () => null }));
vi.mock('./HubTelemetryPreview', () => ({ HubTelemetryPreview: () => null }));
vi.mock('../services/studioHub', () => ({ setTelemetry: vi.fn() }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('opens upgrades from the notification target and reads per-engine readiness without starting jobs', async () => {
  const onlyRead = vi.fn(async (url: string, init?: RequestInit) => {
    expect(init?.method ?? 'GET').toBe('GET');
    if (url === '/v1/music/pair') return Response.json({ active: { kind: 'audio_cpp', model: 'yue2' } });
    if (url === '/engine/options') return Response.json({ executable: '/engines/yue-server' });
    if (url.includes('/acestep-cpp/profiles')) return Response.json({ engine_installed: true, profiles: [{ id: 'minimal', ready: false }] });
    if (url.includes('/minimaxmusic-cpp/profiles')) return Response.json({ engine_installed: false, profiles: [] });
    if (url === '/v1/music/audio-cpp/models') return Response.json({ engine_installed: true, models: [{ id: 'yue2', ready: true }] });
    if (url === '/v1/music/native/hotstep/profiles') return Response.json({}, { status: 503 });
    if (url === '/v1/rvc/native/runtime') return Response.json({ supported: false, reason: 'Unsupported GPU' });
    if (url === '/v1/components/updates') return Response.json({ mode: 'notify', updates: [] });
    return Response.json({});
  });
  vi.stubGlobal('fetch', onlyRead);
  const host = document.createElement('div');
  const root = createRoot(host);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  try {
    await act(async () => root.render(<QueryClientProvider client={client}>
      <SettingsModal isOpen target={engineUpdateTarget} onNavigate={vi.fn()} onClose={vi.fn()} theme="light" onToggleTheme={vi.fn()} />
    </QueryClientProvider>));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    expect(host.querySelector('h3')?.textContent).toBe('Engines and updates');
    expect(host.querySelector('section[aria-label="Signed component updates"]')).toBeTruthy();
    expect(host.querySelector('[data-engine="acestep-cpp"]')?.textContent).toContain('Installed · profileNotInstalled');
    expect(host.querySelector('[data-engine="audio.cpp"]')?.textContent).toContain('Installed · Active · profileReady');
    expect(host.querySelector('[data-engine="minimaxmusic-cpp"]')?.textContent).toContain('Not installed');
    expect(host.querySelector('[data-engine="hotstep"]')?.textContent).toContain('Status unavailable');
    expect(host.querySelector('[data-engine="rvc"]')?.textContent).toContain('Unsupported GPU');
    expect(host.querySelector('[data-engine="openrouter"]')?.textContent).toContain('openRouterNotConfigured');
  } finally {
    await act(async () => root.unmount());
    client.clear();
    vi.unstubAllGlobals();
  }
});
let root: Root;
let host: HTMLDivElement;
let calls: Array<{ url: string; init?: RequestInit }>;
let install: () => Promise<Response>;
let save: () => Promise<Response>;
let readStatus: (() => Promise<Response>) | undefined;
let status: Record<string, unknown>;

const engines = [{ id: 'managed', label: 'Text engine' }];
const response = (body: unknown, code = 200): Response => ({
  ok: code >= 200 && code < 300, status: code, json: async () => body,
}) as Response;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}

async function mount(ready = false, modelInstalled = true, withSet = true, pack?: Record<string, unknown>) {
  calls = [];
  install = async () => response({ started: true });
  save = async () => response({});
  readStatus = undefined;
  status = {
    provider: 'managed', runtime: 'cpu', chosen_model: 'text-model',
    assets: [
      { id: 'text-model', label: 'Text model', kind: 'model', bytes: 1000, installed: modelInstalled },
      { id: 'other-model', label: 'Other model', kind: 'model', bytes: 2000, installed: false },
      { id: 'native-runtime', label: 'Native runtime', kind: 'runtime', bytes: 10, installed: false },
    ],
    ...(withSet ? { set: { ready } } : {}),
    ...pack,
  };
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    if (url === '/install') return install();
    if (url === '/settings') return save();
    if (url === '/status') return readStatus ? readStatus() : response(structuredClone(status));
    return response({});
  }));
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<OptionalGroup title="Writing assistant" purpose="Write songs" statusUrl="/status" installUrl="/install" settingsUrl="/settings" removeUrl="/remove" cancelUrl="/cancel" engines={engines} />));
  await act(async () => host.querySelector<HTMLButtonElement>('button')!.click());
}

const button = (title: string) => host.querySelector<HTMLButtonElement>(`button[title="${title}"]`);
const start = async () => { await act(async () => button('download')!.click()); };
const selectModel = async () => {
  await act(async () => {
    const select = host.querySelector('select')!;
    select.value = 'other-model';
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
};

function expectReady(ready: boolean) {
  if (ready) {
    expect(host.textContent).not.toContain('notInstalledSuffix');
    expect(button('remove')).not.toBeNull();
  } else {
    expect(host.textContent?.match(/notInstalledSuffix/g)).toHaveLength(2);
    expect(button('remove')).toBeNull();
  }
}

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('first-run memory recommendation', () => {
  async function mountFirstRun(totalRamGb: number) {
    const components = [
      { id: 'backbone-q5', kind: 'backbone', filename: 'YuE2-Q5_K_M.gguf', bytes: 1 },
      { id: 'backbone-q8', kind: 'backbone', filename: 'YuE2-Q8_0.gguf', bytes: 1 },
      { id: 'vae-f32', kind: 'vae', filename: 'Vae-F32.gguf', bytes: 1 },
    ];
    const profiles = [
      { id: 'light', label: 'Test light set', components: ['backbone-q5', 'vae-f32'], recommended: true, installable: true, total_bytes: 2, ram_needed_gb: 9 },
      { id: 'heavy', label: 'Test heavy set', components: ['backbone-q8', 'vae-f32'], recommended: false, installable: true, total_bytes: 2, ram_needed_gb: 14 },
    ];
    calls = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      if (url === '/setup/status') return response({
        ready: false, engine_ready: false, first_run: true, recommended_profile_id: 'light',
        active: null, installed_components: [], hardware: { totalRamGb },
      });
      if (url === '/setup/catalog') return response({ engine_id: 'yue2-cpp', recommended_profile_id: 'light', profiles, components });
      return response({});
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<SetupGate />));
  }

  it('keeps the 7 GB / 9 GB warning and lets the user pick a set without recommending it', async () => {
    await mountFirstRun(7);
    expect(host.textContent).toContain('Have 7 GB, need 9 GB');
    expect(host.textContent).not.toContain('recommendedForMachine');
    expect(host.textContent).not.toContain('recommendedBadge');
    const heavy = [...host.querySelectorAll('button')].find(element => element.textContent?.includes('Test heavy set'))!;
    expect(heavy.disabled).toBe(false);
    await act(async () => heavy.click());
    expect(heavy.className).toContain('border-pink-400');
    expect(calls.some(({ url }) => url === '/setup/download')).toBe(false);
  });

  it('keeps the machine recommendation and card badge when the recommended set fits', async () => {
    await mountFirstRun(12);
    expect(host.textContent).toContain('recommendedForMachine');
    expect(host.textContent).toContain('recommendedBadge');
    expect(host.textContent).not.toContain('Have 12 GB, need 9 GB');
    expect(host.textContent).toContain('Have 12 GB, need 14 GB');
  });
});

describe('saving a model set from Settings', () => {
  const previous = ['backbone-q8', 'vae-f32'];
  const next = ['backbone-q6', 'vae-f32'];
  const profiles = [
    { id: 'previous', label: 'Previous set', components: previous, installable: true, total_bytes: 2 },
    { id: 'next', label: 'Next set', components: next, installable: true, total_bytes: 2 },
  ];
  const catalog = {
    engine_id: 'yue2-cpp',
    recommended_profile_id: 'next',
    profiles,
    components: [
      { id: 'backbone-q8', kind: 'backbone', filename: 'YuE2-Q8_0.gguf', bytes: 1 },
      { id: 'backbone-q6', kind: 'backbone', filename: 'YuE2-Q6_K.gguf', bytes: 1 },
      { id: 'vae-f32', kind: 'vae', filename: 'Vae-F32.gguf', bytes: 1 },
    ],
  };

  async function mountSettings({ installed = true, selected = true, initialTarget = 'models' } = {}) {
    const close = vi.fn();
    calls = [];
    save = async () => response({});
    vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      if (url === '/setup/status') return response({
        ready: selected,
        engine_ready: true,
        first_run: false,
        recommended_profile_id: 'next',
        active: null,
        installed_components: installed ? [...previous, next[0]] : previous,
        selected_component_ids: selected ? previous : [],
      });
      if (url === '/setup/catalog') return response(catalog);
      if (url === '/v1/components/updates') return response({ mode: 'notify', updates: [] });
      if (url === '/setup/select') return save();
      return response({});
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    function SettingsWindow() {
      const [target, setTarget] = React.useState(initialTarget);
      return <>
        <button onClick={() => setTarget('models')}>Open models from the window bridge</button>
        <SettingsModal isOpen target={target} onNavigate={setTarget} onClose={close} theme="light" onToggleTheme={() => {}} />
      </>;
    }
    await act(async () => root.render(<SettingsWindow />));
    return close;
  }

  async function press(label: string) {
    const target = Array.from(host.querySelectorAll('button')).find(element => element.textContent?.includes(label));
    expect(target).toBeDefined();
    await act(async () => target!.click());
  }
  const mutations = () => calls.filter(({ url }) => url === '/setup/select' || url === '/setup/download');

  it('opens the component update panel for the notification action target', async () => {
    await mountSettings({ initialTarget: 'models:component-updates' });
    const panel = [...host.querySelectorAll('details')]
      .find(detail => detail.querySelector('summary')?.textContent?.includes('updateSettingsEntry'));
    expect(panel?.open).toBe(true);
    expect(panel?.querySelector('section[aria-label="Signed component updates"]')).toBeTruthy();
  });

  it('returns to the requested page after a manual tab change and retains model edits', async () => {
    await mountSettings();
    await press('Next set');
    await press('account');
    expect(host.querySelector('h3')?.textContent).toBe('account');
    await press('Open models from the window bridge');
    expect(host.querySelector('h3')?.textContent).toBe('modelsSection');
    await press('done');
    expect(JSON.parse(mutations()[0].init?.body as string)).toEqual({ component_ids: next });
  });

  it('waits for the selection to save before closing, even after changing tabs', async () => {
    const close = await mountSettings();
    const pending = deferred<Response>();
    save = () => pending.promise;
    await press('Next set');
    await press('account');
    await press('done');
    expect(mutations()).toHaveLength(1);
    const [{ url, init }] = mutations();
    expect(url).toBe('/setup/select');
    expect(init?.method).toBe('POST');
    expect(JSON.parse(init?.body as string)).toEqual({ component_ids: next });
    expect(close).not.toHaveBeenCalled();
    await act(async () => pending.resolve(response({})));
    expect(close).toHaveBeenCalledOnce();
  });

  it('leaves Settings open and preserves the error when selection fails', async () => {
    const close = await mountSettings();
    save = async () => response({ error: 'Cannot save selection' }, 409);
    await press('Next set');
    await press('done');
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Cannot save selection');
    expect(close).not.toHaveBeenCalled();
    expect(mutations().map(({ url }) => url)).toEqual(['/setup/select']);
  });

  it('leaves an edited set with missing weights open without starting a download', async () => {
    const close = await mountSettings({ installed: false });
    await press('Next set');
    await press('done');
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('downloadSelectedProfile');
    expect(close).not.toHaveBeenCalled();
    expect(mutations()).toEqual([]);
  });

  it('closes without persisting a prefilled recommendation that the user did not choose', async () => {
    const close = await mountSettings({ selected: false });
    await press('done');
    expect(close).toHaveBeenCalledOnce();
    expect(mutations()).toEqual([]);
  });
});

describe('optional capability readiness', () => {
  it('keeps the network error and shows the pinned archive and its exact local path', async () => {
    const url = 'https://github.com/timoncool/transcribe.cpp/releases/download/engine-0.2.1/transcribe-windows-cuda13-x64.zip';
    const path = 'D:\\Studio Data\\karaoke\\runtime\\transcribe-windows-cuda13-x64.zip';
    const error = 'Connection failed: invalid certificate';
    await mount(false, true, true, {
      active_download: { done: true, error, manual_archive: { url, path } },
    });
    expect(host.textContent).toContain(error);
    expect(host.textContent).toContain(path);
    expect(host.textContent).toContain('manualRuntimeHint');
    expect(host.querySelector('a[href="' + url + '"]')).not.toBeNull();
    expect(button('download')).not.toBeNull();
  });
  it('offers retry immediately when the accepted download fails before the first poll', async () => {
    await mount();
    install = async () => {
      status.active_download = { done: true, error: 'invalid certificate' };
      return response({ started: true });
    };
    await start();
    expect(host.textContent).toContain('invalid certificate');
    expect(button('download')).not.toBeNull();
    expect(button('cancelDownload')).toBeNull();
  });

  it('shows only the captioner download size when MOSS weights already exist', async () => {
    const weights = 5_188_223_648;
    const captioner = 4_268_241;
    await mount(false, true, true, {
      assets: [
        { id: 'moss-lm-q4', label: 'MOSS-Music 8B', kind: 'runtime', bytes: weights, installed: true },
        { id: 'ace-caption', label: 'Captioner', kind: 'runtime', bytes: captioner, installed: false },
      ],
      set: { bytes: weights + captioner, installed_bytes: weights, ready: false, files: 2 },
    });
    expect(host.textContent).toContain('download · 4 MB');
    expect(host.textContent).not.toContain('4.8 GB');
  });

  it('does not mistake installed weights for a usable native runtime', async () => {
    await mount();
    expectReady(false);
  });

  it('accepts authoritative readiness for a custom GGUF without installed catalog weights or byte totals', async () => {
    await mount(true, false);
    expectReady(true);
  });

  it('retains the byte-based row fallback for servers without set readiness', async () => {
    await mount(false, true, false);
    expectReady(true);
  });

  it.each([false, true])('masks stale readiness until model save/reload returns readiness %s', async ready => {
    await mount(true);
    const write = deferred<Response>();
    const reload = deferred<Response>();
    save = () => write.promise;
    await selectModel();
    expectReady(false);
    expect(host.querySelector('select')!.disabled).toBe(true);
    expect(button('download')!.disabled).toBe(true);
    expect(JSON.parse(calls.find(call => call.url === '/settings')!.init!.body as string)).toEqual({ managed_model: 'other-model' });
    readStatus = () => reload.promise;
    await act(async () => write.resolve(response({})));
    expectReady(false);
    expect(host.querySelector('select')!.disabled).toBe(true);
    status.chosen_model = 'other-model';
    status.set = { ready };
    await act(async () => reload.resolve(response(structuredClone(status))));
    expectReady(ready);
    expect(host.querySelector('select')!.disabled).toBe(false);
    expect(host.querySelector('select')!.value).toBe('other-model');
  });

  it.each(['before save', 'during save'])('ignores a stale poll begun %s after the saved model refresh completes', async timing => {
    vi.useFakeTimers();
    await mount(true);
    // Starting installation enables polling, but the model selector remains
    // available while the installation request is still being acknowledged.
    const installReply = deferred<Response>();
    install = () => installReply.promise;
    await start();
    const stale = deferred<Response>();
    const oldStatus = structuredClone(status);
    readStatus = () => stale.promise;
    if (timing === 'before save') {
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    }
    const write = deferred<Response>();
    save = () => write.promise;
    await selectModel();
    if (timing === 'during save') {
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    }
    expect(calls.filter(call => call.url === '/status')).toHaveLength(2);
    status.chosen_model = 'other-model';
    status.set = { ready: false };
    readStatus = undefined;
    await act(async () => write.resolve(response({})));
    expectReady(false);
    expect(host.querySelector('select')!.disabled).toBe(false);
    await act(async () => stale.resolve(response(oldStatus)));
    expectReady(false);
    expect(host.querySelector('select')!.value).toBe('other-model');
    await act(async () => installReply.resolve(response({ started: true })));
    expectReady(false);
  });

  it.each(['HTTP', 'network'])('does not reuse old readiness when model status refresh has a %s failure', async kind => {
    await mount(true);
    readStatus = kind === 'HTTP'
      ? async () => response({ error: 'Status unavailable' }, 503)
      : async () => { throw new Error('Status unreachable'); };
    await selectModel();
    expect(host.textContent).toContain(kind === 'HTTP' ? 'Status unavailable' : 'Status unreachable');
    expectReady(false);
    expect(host.querySelector('select')!.disabled).toBe(false);
    expect(button('download')!.disabled).toBe(false);
  });

  it.each(['HTTP', 'network'])('shows a model save %s failure, refreshes status, and restores the saved selection', async kind => {
    await mount(true);
    save = kind === 'HTTP'
      ? async () => response({ error: 'Model refused' }, 400)
      : async () => { throw new Error('Save unreachable'); };
    await selectModel();
    expect(host.textContent).toContain(kind === 'HTTP' ? 'Model refused' : 'Save unreachable');
    expect(calls.filter(call => call.url === '/status')).toHaveLength(2);
    expect(host.querySelector('select')!.disabled).toBe(false);
    expect(host.querySelector('select')!.value).toBe('text-model');
    expectReady(true);
  });
});

describe('optional capability installation', () => {
  it('ends starting after eight seconds despite repeated inactive polls', async () => {
    vi.useFakeTimers();
    await mount();
    await start();
    expect(button('cancelDownload')).not.toBeNull();
    for (let seconds = 0; seconds < 8; seconds++) {
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    }
    expect(calls.filter(call => call.url === '/status').length).toBeGreaterThan(4);
    expect(button('cancelDownload')).toBeNull();
    expect(button('download')).not.toBeNull();
  });

  it.each(['HTTP', 'network'])('ends starting immediately on an install %s failure', async kind => {
    await mount();
    install = kind === 'HTTP'
      ? async () => response({ error: 'Install refused' }, 400)
      : async () => { throw new Error('Install unreachable'); };
    await start();
    expect(host.textContent).toContain(kind === 'HTTP' ? 'Install refused' : 'Install unreachable');
    expect(button('cancelDownload')).toBeNull();
    expect(button('download')).not.toBeNull();
  });

  it('hands starting to real progress and then to authoritative readiness', async () => {
    vi.useFakeTimers();
    await mount();
    status.active_download = { asset_id: 'native-runtime', downloaded_bytes: 25, total_bytes: 100, done: false };
    await start();
    expect(host.textContent).toContain('25%');
    await act(async () => { await vi.advanceTimersByTimeAsync(9000); });
    expect(button('cancelDownload')).not.toBeNull();
    expect(host.textContent).toContain('25%');
    status.active_download = { asset_id: 'native-runtime', downloaded_bytes: 100, total_bytes: 100, done: true };
    status.set = { ready: true };
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(button('cancelDownload')).toBeNull();
    expectReady(true);
  });

  it('ends starting immediately when an install completes before progress appears', async () => {
    await mount();
    status.set = { ready: true };
    await start();
    expect(button('cancelDownload')).toBeNull();
    expectReady(true);
  });
});

describe('optional capability reads', () => {
  it('a page drawn again with the same engines does not ask the group again', async () => {
    let asked = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url === '/status') asked += 1;
      return response({ provider: 'managed', runtime: 'cpu', assets: [] });
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    const draw = () => root.render(<OptionalGroup title="Writing assistant" purpose="Write songs" statusUrl="/status" installUrl="/install" engines={[{ id: 'managed', label: 'Text engine' }]} embedded />);
    await act(async () => draw());
    const first = asked;
    expect(first).toBeGreaterThanOrEqual(1);
    for (let again = 0; again < 5; again++) await act(async () => draw());
    expect(asked).toBe(first);
  });
});

describe('where each part runs', () => {
  it('says HT-Demucs and Mel-Band RoFormer compute on AMD and Intel cards through Vulkan', () => {
    const row = (name: string) => WHERE_IT_RUNS.find(entry => entry.name === name);
    expect(row('HT-Demucs')?.other).toBe('vulkan');
    expect(row('Mel-Band RoFormer')?.other).toBe('vulkan');
  });
});
