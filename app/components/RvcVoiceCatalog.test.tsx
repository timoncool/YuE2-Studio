import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { RvcVoiceCatalog } from './RvcVoiceCatalog';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));

const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });

it('imports paired RVC files and selects the resulting catalog entry', async () => {
  const original = { id: 'one', model_name: 'Original.pth', index_name: null, version: 1 };
  const imported = { id: 'two', model_name: 'Singer.pt', index_name: 'Singer.index', version: 2 };
  const fetchVoice = vi.fn(async (url: string) => Response.json(url.endsWith('/import') ? imported : [original]));
  vi.stubGlobal('fetch', fetchVoice);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  localStorage.setItem('studio.rvc.voiceId', original.id);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    expect(host.querySelector('select')!.value).toBe(original.id);
    const [model, index] = host.querySelectorAll('input');
    const button = host.querySelector('button')!;
    expect(button.disabled).toBe(true);
    function selectFile(input: HTMLInputElement, name: string) {
      Object.defineProperty(input, 'files', { configurable: true, value: [new File(['fixture'], name)] });
      act(() => { input.dispatchEvent(new Event('change', { bubbles: true })); });
    }
    selectFile(model, imported.model_name);
    selectFile(index, imported.index_name);
    expect(button.disabled).toBe(false);
    act(() => { button.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    await flush();

    const [, request] = vi.mocked(fetch).mock.calls.find(([url]) => String(url).endsWith('/import'))!;
    const form = request?.body as FormData;
    expect((form.get('model') as File).name).toBe(imported.model_name);
    expect((form.get('index') as File).name).toBe(imported.index_name);
    expect(host.querySelector('select')!.value).toBe(imported.id);
    expect(host.textContent).toContain('Singer.pt · v2 · Singer.index');
    expect(localStorage.getItem('studio.rvc.voiceId')).toBe(imported.id);
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});

it('retries a failed catalog request and restores the selected voice on remount', async () => {
  const voice = { id: 'selected', model_name: 'Saved.pth', index_name: null, version: 1 };
  const request = vi.fn<() => Promise<Response>>()
    .mockResolvedValueOnce(Response.json({ error: 'Catalog unavailable' }, { status: 503 }))
    .mockImplementation(async () => Response.json([voice]));
  vi.stubGlobal('fetch', request);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  let root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Catalog unavailable');
    const retry = [...host.querySelectorAll('button')].find(button => button.textContent === 'Retry loading voices')!;
    act(() => { retry.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    await flush();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    const select = host.querySelector('select')!;
    expect(select.disabled).toBe(false);
    act(() => {
      select.value = voice.id;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    act(() => root.unmount());
    root = createRoot(host);
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    expect(host.querySelector('select')!.value).toBe(voice.id);
    expect(request).toHaveBeenCalledTimes(3);
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});

it('lists curated voices and installs the selected ID into the local catalog', async () => {
  const entry = { id: 'curated-one', name: 'Example Voice', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 },
    index: null, installed_id: null };
  const voice = { id: 'installed-id', model_name: 'voice.pth', index_name: null, version: 2 };
  const request = vi.fn(async (url: string, _options?: RequestInit) => {
    if (url.endsWith('/catalog/install')) return Response.json(voice);
    if (url.endsWith('/catalog')) return Response.json([entry]);
    return Response.json([]);
  });
  vi.stubGlobal('fetch', request);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    const details = host.querySelector('details')!;
    act(() => {
      details.open = true;
      details.dispatchEvent(new Event('toggle', { bubbles: true }));
    });
    await flush();
    expect(details.textContent).toContain('License unspecified');
    expect(details.querySelector('a')?.href).toBe(entry.source_url);
    const install = details.querySelector('button')!;
    expect(install.disabled).toBe(false);
    act(() => { install.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    await flush();
    const [, options] = request.mock.calls.find(([url]) => url.endsWith('/catalog/install'))!;
    expect(JSON.parse(String(options?.body))).toEqual({ id: entry.id });
    expect(host.querySelector('select')!.value).toBe(voice.id);
    expect(install.disabled).toBe(true);
    expect(install.textContent).toBe('Installed');
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});

it('finds Radio Tapok by source label and combines vocal, model, sample-rate and index filters', async () => {
  const voice = { id: 'voice', name: 'Acoustic Singer', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 },
    installed_id: null };
  const list = [
    { ...voice, id: 'high', name: 'Tapok Concert', index: { size: 8_000_000 },
      metadata: { source_labels: ['Radio Tapok'], vocal_style: 'high', rvc_version: 'v2', sample_rate_hz: 40000 } },
    { ...voice, id: 'low', name: 'Tapok Studio', index: null,
      metadata: { source_labels: ['Radio Tapok'], vocal_style: 'low', rvc_version: 'v1', sample_rate_hz: 48000 } },
    { ...voice, id: 'legacy', name: 'Unclassified Voice', index: null, metadata: null },
  ];
  const request = vi.fn(async (url: string) => Response.json(url.endsWith('/catalog') ? list : []));
  vi.stubGlobal('fetch', request);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    const details = host.querySelector('details')!;
    act(() => {
      details.open = true;
      details.dispatchEvent(new Event('toggle', { bubbles: true }));
    });
    await flush();
    const search = details.querySelector<HTMLInputElement>('input[type="search"]')!;
    const valueSetter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(search), 'value')!.set!;
    act(() => {
      valueSetter.call(search, 'Radio Tapok');
      search.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(details.querySelectorAll('li')).toHaveLength(2);
    const [style, version, sampleRate, index] = details.querySelectorAll('select');
    function pick(input: HTMLSelectElement, value: string) {
      act(() => {
        input.value = value;
        input.dispatchEvent(new Event('change', { bubbles: true }));
      });
    }
    pick(style, 'high');
    pick(version, 'v2');
    pick(sampleRate, '40000');
    pick(index, 'present');
    expect(details.querySelectorAll('li')).toHaveLength(1);
    expect(details.querySelector('li')?.textContent).toContain('Tapok Concert');
    pick(index, 'absent');
    expect(details.textContent).toContain('No voices match these filters.');
    const clear = [...details.querySelectorAll('button')].find(button => button.textContent === 'Clear filters')!;
    act(() => { clear.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    expect(details.querySelectorAll('li')).toHaveLength(3);
    expect(details.textContent).toContain('Unclassified Voice');
  } finally {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
