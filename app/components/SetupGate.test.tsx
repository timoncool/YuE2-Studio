import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { OptionalGroup } from './SetupGate';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({ openRouterSettingsChanged: vi.fn() }));
vi.mock('./AssistantSettings', () => ({ AssistantExtras: () => null }));
vi.mock('./KaraokeSettings', () => ({ KaraokeExtras: () => null }));
vi.mock('./HubTelemetryPreview', () => ({ HubTelemetryPreview: () => null }));
vi.mock('../services/studioHub', () => ({ setTelemetry: vi.fn() }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
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

async function mount(ready = false, modelInstalled = true, withSet = true) {
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

describe('optional capability readiness', () => {
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
