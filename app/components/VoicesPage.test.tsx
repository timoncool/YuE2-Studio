import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { VoicesPage } from './VoicesPage';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('./TrainingPanel', () => ({ TrainingPanel: () => <div>training</div> }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
let host: HTMLDivElement;
let root: Root;
const buttonNamed = (text: string) => [...host.querySelectorAll('button')].find(button => button.textContent === text);
const click = (element: Element) => act(() => { element.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
const openTab = (name: string) => click([...host.querySelectorAll('[role="tab"]')].find(tab => tab.textContent?.startsWith(name))!);
const render = async () => { act(() => root.render(<VoicesPage />)); await flush(); };

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  localStorage.removeItem('studio.rvc.voiceId');
  vi.unstubAllGlobals();
});

it('imports paired RVC files into my voices and selects the new voice', async () => {
  const original = { id: 'one', model_name: 'Original.pth', index_name: null, version: 1 };
  const imported = { id: 'two', model_name: 'Singer.pt', index_name: 'Singer.index', version: 2 };
  vi.stubGlobal('fetch', vi.fn(async (url: string) => Response.json(url.endsWith('/import') ? imported : [original])));
  localStorage.setItem('studio.rvc.voiceId', original.id);
  await render();
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe('My voices · 1');
  const [model, index] = host.querySelectorAll<HTMLInputElement>('input[type="file"]');
  const [modelPick, indexPick] = [...host.querySelectorAll<HTMLButtonElement>('button[aria-describedby]')];
  expect(document.getElementById(modelPick.getAttribute('aria-describedby')!)?.textContent).toContain('(.pth)');
  expect(document.getElementById(indexPick.getAttribute('aria-describedby')!)?.textContent).toContain('(.index)');
  const button = buttonNamed('Import voice')!;
  expect(button.disabled).toBe(true);
  const selectFile = (input: HTMLInputElement, name: string) => {
    Object.defineProperty(input, 'files', { configurable: true, value: [new File(['fixture'], name)] });
    act(() => { input.dispatchEvent(new Event('change', { bubbles: true })); });
  };
  selectFile(model, imported.model_name);
  selectFile(index, imported.index_name);
  click(button);
  await flush();
  const [, request] = vi.mocked(fetch).mock.calls.find(([url]) => String(url).endsWith('/import'))!;
  const form = request?.body as FormData;
  expect((form.get('model') as File).name).toBe(imported.model_name);
  expect((form.get('index') as File).name).toBe(imported.index_name);
  expect(host.textContent).toContain('My voices · 2');
  expect(host.textContent).toContain('Singer.pt');
  expect(host.textContent).toContain('with retrieval index');
  expect(localStorage.getItem('studio.rvc.voiceId')).toBe(imported.id);
});

it('retries a failed voices request and sends people without voices to the catalogue', async () => {
  let asked = 0;
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    if (url.endsWith('/catalog')) return Response.json([]);
    if (url === '/v1/rvc/voices' && ++asked === 1) return Response.json({ error: 'Catalog unavailable' }, { status: 503 });
    return Response.json([]);
  }));
  await render();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Catalog unavailable');
  click(buttonNamed('Retry loading voices')!);
  await flush();
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(host.textContent).toContain('No voices yet');
  click(buttonNamed('Open the catalogue')!);
  await flush();
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe('Catalogue');
});

it('installs a catalogue voice into my voices and shows no licence line where none is published', async () => {
  const entry = { id: 'curated-one', name: 'Example Voice', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 }, index: null, installed_id: null };
  const voice = { id: 'installed-id', model_name: 'voice.pth', index_name: null, version: 2, origin: { name: entry.name } };
  const request = vi.fn(async (url: string, _options?: RequestInit) => {
    if (url.endsWith('/catalog/install')) return Response.json(voice);
    if (url.endsWith('/catalog')) return Response.json([entry]);
    return Response.json([]);
  });
  vi.stubGlobal('fetch', request);
  await render();
  openTab('Catalogue');
  await flush();
  const row = host.querySelector('li')!;
  expect(row.textContent).not.toContain('License');
  expect(row.querySelector('button[aria-label="owner/voices"]')).not.toBeNull();
  click(host.querySelector<HTMLButtonElement>('button[aria-label="Install Example Voice"]')!);
  await flush();
  const [, options] = request.mock.calls.find(([url]) => url.endsWith('/catalog/install'))!;
  expect(JSON.parse(String(options?.body))).toEqual({ id: entry.id });
  expect(host.querySelector('button[aria-label="Select Example Voice"]')).not.toBeNull();
  openTab('My voices');
  expect(host.textContent).toContain('My voices · 1');
  expect(host.textContent).toContain('Example Voice');
  expect(localStorage.getItem('studio.rvc.voiceId')).toBe(voice.id);
});

it('finds catalogue voices by name or source label, filters by vocal style and pages through the rest', async () => {
  const voice = { id: 'voice', name: 'Acoustic Singer', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 }, installed_id: null };
  const list = [
    { ...voice, id: 'high', name: 'Tapok Concert', weights: undefined, has_index: true, download_bytes: 23_000_000,
      metadata: { source_labels: ['Radio Tapok'], vocal_style: 'high' } },
    { ...voice, id: 'low', name: 'Tapok Studio', index: null, metadata: { source_labels: ['Radio Tapok'], vocal_style: 'low' } },
    { ...voice, id: 'legacy', name: 'Unclassified Voice', index: null, metadata: null },
    ...Array.from({ length: 81 }, (_, offset) => ({ ...voice, id: `extra-${offset}`, name: `Голос 音楽 ${offset}`, index: null })),
  ];
  vi.stubGlobal('fetch', vi.fn(async (url: string) => Response.json(url.endsWith('/catalog') ? list : [])));
  await render();
  openTab('Catalogue');
  await flush();
  const search = host.querySelector<HTMLInputElement>('input[type="search"]')!;
  const valueSetter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(search), 'value')!.set!;
  const type = (text: string) => act(() => {
    valueSetter.call(search, text);
    search.dispatchEvent(new Event('input', { bubbles: true }));
  });
  type('Radio Tapok');
  expect(host.querySelectorAll('li')).toHaveLength(2);
  click([...host.querySelectorAll('[role="radio"]')].find(radio => radio.textContent === 'High')!);
  expect(host.querySelectorAll('li')).toHaveLength(1);
  expect(host.querySelector('li')?.textContent).toContain('Tapok Concert');
  expect(host.querySelector('li')?.textContent).toContain('23 MB');
  click(buttonNamed('Clear filters')!);
  expect(host.querySelectorAll('li')).toHaveLength(40);
  expect(host.textContent).toContain('Showing 40 of 84 voices');
  for (const expected of [80, 84]) {
    click(buttonNamed('Show more')!);
    expect(host.querySelectorAll('li')).toHaveLength(expected);
  }
  type('голос 音楽');
  expect(host.textContent).toContain('Showing 40 of 81 voices');
});

it('keeps the voice on a failed delete and offers it again in the catalogue after a successful one', async () => {
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
    if (url.endsWith('/catalog')) return Response.json([{ ...entry, installed_id: installed ? voice.id : null }]);
    return Response.json(installed ? [voice] : []);
  });
  vi.stubGlobal('fetch', request);
  localStorage.setItem('studio.rvc.voiceId', voice.id);
  await render();
  const remove = () => {
    click(host.querySelector('button[aria-label="Remove voice: Singer.pth"]')!);
    click([...document.querySelectorAll('button')].filter(button => button.textContent === 'Remove voice').at(-1)!);
  };
  remove();
  await flush();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Voice files are locked');
  expect(host.textContent).toContain('My voices · 1');
  expect(localStorage.getItem('studio.rvc.voiceId')).toBe(voice.id);
  fail = false;
  remove();
  await flush();
  expect(host.textContent).toContain('My voices · 0');
  expect(localStorage.getItem('studio.rvc.voiceId')).toBeNull();
  openTab('Catalogue');
  await flush();
  click(host.querySelector<HTMLButtonElement>('button[aria-label="Install Singer"]')!);
  await flush();
  expect(host.textContent).toContain('My voices · 1');
  expect(request.mock.calls.filter(([, init]) => init?.method === 'DELETE').map(([url]) => url))
    .toEqual(['/v1/rvc/voices/local-id', '/v1/rvc/voices/local-id']);
});

it('shows installation progress at the catalogue voice being installed', async () => {
  const entry = { id: 'download', name: 'Example Voice', source_repo: 'owner/voices',
    source_url: 'https://huggingface.co/owner/voices', license: null, weights: { size: 15_000_000 },
    index: { size: 1_000_000 }, installed_id: null };
  const voice = { id: 'installed', model_name: 'voice.pth', index_name: 'voice.index', version: 2, origin: { name: entry.name } };
  let finish!: (response: Response) => void;
  const installation = new Promise<Response>(resolve => { finish = resolve; });
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    if (url.endsWith('/install')) return installation;
    if (url.endsWith('/progress/download')) return Response.json({ stage: 'downloading_index', downloaded_bytes: 500_000, total_bytes: 1_000_000 });
    if (url.endsWith('/catalog')) return Response.json([entry]);
    return Response.json([]);
  }));
  await render();
  openTab('Catalogue');
  await flush();
  click(host.querySelector<HTMLButtonElement>('button[aria-label="Install Example Voice"]')!);
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 400)); });
  const progress = host.querySelector('li [role="progressbar"]')!;
  expect(progress.getAttribute('aria-label')).toBe('Downloading the retrieval index…');
  expect(progress.getAttribute('aria-valuenow')).toBe('50');
  finish(Response.json(voice));
  await flush();
  expect(host.querySelector('[role="progressbar"]')).toBeNull();
  expect(host.textContent).toContain('Installed Example Voice');
});
