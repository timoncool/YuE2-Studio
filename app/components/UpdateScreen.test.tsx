import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));
vi.mock('../services/studioQueries', () => ({ SELF_READ: { 'x-hub-skip': '1' } }));

const { UpdatePlanView } = await import('./UpdateScreen');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('UpdatePlanView', () => {
  it('keeps the signed update policy and installs only an explicitly selected installed-package update', async () => {
    const plan = { version: '5.0.0', updated: [], added: [], retired: [], download: 0, freed: 0, show: false };
    let mode = 'manual';
    let updates = [
      { kind: 'music-engines', id: 'engine-fixture', from_version: '1.0', to_version: '1.1', changed_bytes: 4096 },
      { kind: 'music-models', id: 'model-fixture', from_version: '2.0', to_version: '2.1', changed_bytes: 2 ** 30 },
    ];
    const api = vi.fn(async (url: string, init?: RequestInit) => {
      if (url === '/setup/update') return Response.json({ plan, job: null });
      if (url === '/v1/components/updates') return Response.json({ mode, updates });
      if (url === '/v1/components/updates/mode') {
        expect(init?.method).toBe('POST');
        mode = JSON.parse(String(init?.body)).mode;
        return Response.json({ mode });
      }
      if (url === '/v1/components/music-engines/refresh') {
        expect(init?.method).toBe('POST');
        return Response.json({ source: 'signed', engines: [] });
      }
      if (url === '/v1/components/music-models/model-fixture/install') {
        expect(init?.method).toBe('POST');
        updates = updates.filter(update => update.id !== 'model-fixture');
        return Response.json({ downloaded_bytes: 2 ** 30, reused_bytes: 0 });
      }
      throw new Error(url);
    });
    vi.stubGlobal('fetch', api);
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<UpdatePlanView />));
    const section = host.querySelector('section[aria-label="Signed component updates"]')!;
    const select = section.querySelector('select')!;
    expect(select.value).toBe('manual');
    expect([...select.options].map(option => option.value)).toEqual(['manual', 'notify', 'auto']);
    expect(section.textContent).toContain('4 KiB');
    expect(section.textContent).toContain('1 GiB');
    expect(api.mock.calls.some(([, init]) => init?.method === 'POST')).toBe(false);
    for (const value of ['notify', 'auto']) {
      await act(async () => {
        select.value = value;
        select.dispatchEvent(new Event('change', { bubbles: true }));
      });
      expect(select.value).toBe(value);
    }
    const click = async (button: Element) => {
      await act(async () => button.dispatchEvent(new MouseEvent('click', { bubbles: true })));
    };
    await click([...section.querySelectorAll('button')].find(button => button.textContent === 'Check for updates')!);
    await click(section.querySelector('[aria-label="Update model-fixture"]')!);
    expect(section.querySelector('[aria-label="Update engine-fixture"]')).toBeTruthy();
    expect(section.querySelector('[aria-label="Update model-fixture"]')).toBeNull();
    expect(api.mock.calls.filter(([, init]) => init?.method === 'POST').map(([url]) => url)).toEqual([
      '/v1/components/updates/mode',
      '/v1/components/updates/mode',
      '/v1/components/music-engines/refresh',
      '/v1/components/music-models/model-fixture/install',
    ]);
    await act(async () => root.render(null));
    await act(async () => root.render(<UpdatePlanView />));
    expect(host.querySelector('section select')?.getAttribute('disabled')).toBeNull();
    expect(host.querySelector<HTMLSelectElement>('section select')?.value).toBe('auto');
  });

  it('shows the exact recovery archive and local destination from the failed update', async () => {
    const archive = { url: 'https://example.org/releases/onnx.zip', path: 'D:/portable/tools/onnx.zip' };
    const plan = { version: '4.1.1', updated: [], added: [], retired: [], download: 0, freed: 0, show: true };
    const job = { status: 'failed', error: 'connection timed out', manual_archive: archive, updated: [], added: [] };
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: async () => ({ plan, job }) })));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<UpdatePlanView />));
    expect(host.querySelector('a')?.getAttribute('href')).toBe(archive.url);
    expect(host.querySelector('code')?.textContent).toBe(archive.path);
  });

  it('asks for the plan again only after the last answer, not on a timer over a slow one', async () => {
    vi.useFakeTimers();
    const plan = { version: '3.9.0', updated: [], added: [], retired: [], download: 0, freed: 0, show: true };
    const running = { status: 200, ok: true, json: async () => ({ plan, job: { status: 'running', error: null, updated: [], added: [] } }) } as Response;
    let asked = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url !== '/setup/update') return running;
      asked += 1;
      // the first answer comes; the service then answers no more within the test
      return asked === 1 ? running : new Promise<Response>(() => undefined);
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<UpdatePlanView />));
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(asked).toBe(2);
  });
});
