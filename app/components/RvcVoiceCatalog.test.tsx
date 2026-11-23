import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { RvcVoiceCatalog } from './RvcVoiceCatalog';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));

const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
const buttonNamed = (host: HTMLElement, text: string) => [...host.querySelectorAll('button')].find(button => button.textContent === text);

it('imports paired RVC files and selects the resulting catalog entry', async () => {
  const original = { id: 'one', model_name: 'Original.pth', index_name: null, version: 1 };
  const imported = { id: 'two', model_name: 'Singer.pt', index_name: 'Singer.index', version: 2 };
  const fetchVoice = vi.fn(async (url: string) => Response.json(url.endsWith('/import') ? imported : url.endsWith('/catalog') ? [] : [original]));
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
    const [model, index] = host.querySelectorAll<HTMLInputElement>('input[type="file"]');
    expect(document.getElementById(model.getAttribute('aria-describedby')!)?.textContent).toContain('(.pth)');
    expect(document.getElementById(index.getAttribute('aria-describedby')!)?.textContent).toContain('(.index)');
    const button = buttonNamed(host, 'Import voice')!;
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
    expect(host.textContent).toContain('Voice: Singer.pt');
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
  let voicesAsked = 0;
  const request = vi.fn(async (url: string, _init?: RequestInit) => {
    if (url.endsWith('/catalog')) return Response.json([]);
    if (url === '/v1/rvc/voices' && ++voicesAsked === 1) return Response.json({ error: 'Catalog unavailable' }, { status: 503 });
    return Response.json([voice]);
  });
  vi.stubGlobal('fetch', request);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  let root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Catalog unavailable');
    act(() => { buttonNamed(host, 'Retry loading voices')!.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
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
    expect(voicesAsked).toBe(3);
    expect(request).toHaveBeenCalledWith('/v1/rvc/selection', expect.objectContaining({
      method: 'PUT', body: JSON.stringify({ voice_id: voice.id, backend: 'auto' }),
    }));
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});

it('opens with the voice catalogue and installs the chosen voice into the local list', async () => {
  const entry = { id: 'curated-one', name: 'Example Voice', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 },
    index: null, installed_id: null };
  const voice = { id: 'installed-id', model_name: 'voice.pth', index_name: null, version: 2, origin: { name: entry.name } };
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
    const row = host.querySelector('li')!;
    expect(row.textContent).toContain('License unspecified');
    expect(row.querySelector('a')?.href).toBe(entry.source_url);
    act(() => { host.querySelector<HTMLButtonElement>('button[aria-label="Install Example Voice"]')!.click(); });
    await flush();
    const [, options] = request.mock.calls.find(([url]) => url.endsWith('/catalog/install'))!;
    expect(JSON.parse(String(options?.body))).toEqual({ id: entry.id });
    expect(host.querySelector('select')!.value).toBe(voice.id);
    expect(host.querySelector('select')!.selectedOptions[0]?.textContent).toContain(entry.name);
    expect(host.querySelector('button[aria-label="Select Example Voice"]')).not.toBeNull();
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});

it('finds voices by name or source label, filters by vocal style and pages through the rest', async () => {
  const voice = { id: 'voice', name: 'Acoustic Singer', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 },
    installed_id: null };
  const list = [
    { ...voice, id: 'high', name: 'Tapok Concert', weights: undefined, has_index: true, download_bytes: 23_000_000,
      metadata: { source_labels: ['Radio Tapok'], vocal_style: 'high' } },
    { ...voice, id: 'low', name: 'Tapok Studio', index: null, metadata: { source_labels: ['Radio Tapok'], vocal_style: 'low' } },
    { ...voice, id: 'legacy', name: 'Unclassified Voice', index: null, metadata: null },
    ...Array.from({ length: 81 }, (_, offset) => ({ ...voice, id: `extra-${offset}`, name: `Голос 音楽 ${offset}`, index: null })),
  ];
  vi.stubGlobal('fetch', vi.fn(async (url: string) => Response.json(url.endsWith('/catalog') ? list : [])));
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    const search = host.querySelector<HTMLInputElement>('input[type="search"]')!;
    const valueSetter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(search), 'value')!.set!;
    const type = (text: string) => act(() => {
      valueSetter.call(search, text);
      search.dispatchEvent(new Event('input', { bubbles: true }));
    });
    type('Radio Tapok');
    expect(host.querySelectorAll('li')).toHaveLength(2);
    const style = [...host.querySelectorAll('label')].find(label => label.textContent?.startsWith('Vocal style'))!.querySelector('select')!;
    act(() => { style.value = 'high'; style.dispatchEvent(new Event('change', { bubbles: true })); });
    expect(host.querySelectorAll('li')).toHaveLength(1);
    expect(host.querySelector('li')?.textContent).toContain('Tapok Concert');
    expect(host.querySelector('li')?.textContent).toContain('23 MB');
    act(() => { buttonNamed(host, 'Clear filters')!.click(); });
    expect(host.querySelectorAll('li')).toHaveLength(40);
    expect(host.textContent).toContain('Showing 40 of 84 voices');
    for (const expected of [80, 84]) {
      act(() => { buttonNamed(host, 'Show more')!.click(); });
      expect(host.querySelectorAll('li')).toHaveLength(expected);
    }
    expect(host.textContent).toContain('Unclassified Voice');
    type('голос 音楽');
    expect(host.querySelectorAll('li')).toHaveLength(40);
    expect(host.textContent).toContain('Showing 40 of 81 voices');
  } finally {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});

it('keeps the selected voice on delete error and offers reinstall after success', async () => {
  const voice = { id: 'local-id', model_name: 'Singer.pth', index_name: 'Singer.index', version: 2 };
  const entry = { id: 'curated-id', name: 'Singer', source_repo: 'owner/repo', license: 'MIT',
    source_url: 'https://huggingface.co/owner/repo', installed_id: voice.id, download_bytes: 1000, has_index: true };
  let fail = true;
  let installed = true;
  const request = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'DELETE') {
      if (fail) return Response.json({ error: 'Voice files are locked' }, { status: 500 });
      installed = false;
      return Response.json({ id: voice.id, removed: true });
    }
    if (url.endsWith('/catalog/install')) return Response.json(voice, { status: 201 });
    if (url.endsWith('/catalog')) return Response.json([entry]);
    return Response.json(installed ? [voice] : []);
  });
  vi.stubGlobal('fetch', request);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  localStorage.setItem('studio.rvc.voiceId', voice.id);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    expect(host.querySelector('button[aria-label="Select Singer"]')).not.toBeNull();
    const remove = () => {
      const buttons = () => Array.from(host.querySelectorAll('button')).filter(button => button.textContent === 'Remove voice');
      act(() => buttons()[0].click());
      act(() => buttons()[1].click());
    };
    remove();
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Voice files are locked');
    expect(host.querySelector('select')!.value).toBe(voice.id);
    expect(localStorage.getItem('studio.rvc.voiceId')).toBe(voice.id);
    fail = false;
    remove();
    await flush();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector('select')).toBeNull();
    expect(localStorage.getItem('studio.rvc.voiceId')).toBeNull();
    act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Install Singer"]')!.click());
    await flush();
    expect(host.querySelector('select')!.value).toBe(voice.id);
    expect(request.mock.calls.filter(([, init]) => init?.method === 'DELETE').map(([url]) => url))
      .toEqual(['/v1/rvc/voices/local-id', '/v1/rvc/voices/local-id']);
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});

it('shows installation progress at the voice being installed', async () => {
  const entry = { id: 'download', name: 'Example Voice', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 },
    index: { size: 1_000_000 }, installed_id: null };
  const voice = { id: 'installed', model_name: 'voice.pth', index_name: 'voice.index', version: 2, origin: { name: entry.name } };
  let finish!: (response: Response) => void;
  const installation = new Promise<Response>(resolve => { finish = resolve; });
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    if (url.endsWith('/install')) return installation;
    if (url.endsWith('/progress/download')) return Response.json({
      stage: 'downloading_index', downloaded_bytes: 500_000, total_bytes: 1_000_000,
    });
    if (url.endsWith('/catalog')) return Response.json([entry]);
    return Response.json([]);
  }));
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  try {
    act(() => root.render(<RvcVoiceCatalog card="" control="" />));
    await flush();
    act(() => { host.querySelector<HTMLButtonElement>('button[aria-label="Install Example Voice"]')!.click(); });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 400)); });
    const progress = host.querySelector('li [role="progressbar"]')!;
    expect(progress.getAttribute('aria-label')).toBe('Downloading the retrieval index…');
    expect(progress.getAttribute('aria-valuenow')).toBe('50');
    finish(Response.json(voice));
    await flush();
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(host.textContent).toContain('Installed Example Voice');
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});
