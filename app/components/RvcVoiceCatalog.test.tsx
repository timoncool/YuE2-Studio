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
