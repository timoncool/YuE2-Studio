import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import bundled from '../../config/rvc-voice-catalog.json';
import { I18nProvider } from '../context/I18nContext';
import { StudioToolsPanel } from './StudioToolsPanel';

const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });

it('opens the curated catalog from Studio Tools and chooses an installed RVC voice', async () => {
  const high = bundled.voices.find(voice => voice.id === 'rvc-50ea3d733073550ef6f7')!;
  const drive = bundled.voices.find(voice => voice.id === 'rvc-5c72b2f2447908b9df55')!;
  const installed = { id: 'installed-tapok', model_name: 'voice.pth', index_name: 'voice.index',
    version: 2, origin: { name: high.name } };
  const fetchStudio = vi.fn(async (url: string) => {
    if (url === '/v1/rvc/voices') return Response.json([installed]);
    if (url === '/v1/rvc/catalog') {
      const list = bundled.voices.map(voice => ({
        ...voice, installed_id: voice.id === high.id ? installed.id : null,
      }));
      return Response.json(list);
    }
    if (url === '/v1/library/songs') return Response.json([]);
    if (url === '/v1/separation/status' || url === '/v1/midi') return Response.json({ run: null });
    return Response.json(null);
  });
  vi.stubGlobal('fetch', fetchStudio);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  localStorage.setItem('language', 'en');
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  try {
    act(() => root.render(<QueryClientProvider client={query}><I18nProvider><StudioToolsPanel /></I18nProvider></QueryClientProvider>));
    await flush();
    const section = host.querySelector<HTMLElement>('section[aria-label="RVC voice catalog"]')!;
    const details = section.querySelector('details')!;
    const installedSelect = section.querySelector('select')!;
    expect(installedSelect.value).toBe('');
    expect(fetchStudio.mock.calls.some(([path]) => path === '/v1/rvc/catalog')).toBe(false);
    expect(details.querySelector('summary')?.textContent).toBe('Curated voices');
    act(() => {
      details.open = true;
      details.dispatchEvent(new Event('toggle', { bubbles: true }));
    });
    await flush();
    expect(details.querySelectorAll('li')).toHaveLength(40);
    expect(details.textContent).toContain(`Showing 40 of ${bundled.voices.length.toLocaleString('en')} voices`);
    const search = details.querySelector<HTMLInputElement>('input[type="search"]')!;
    const setValue = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(search), 'value')!.set!;
    act(() => {
      setValue.call(search, 'Radio Tapok');
      search.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(details.textContent).toContain(high.name);
    expect(details.textContent).toContain(drive.name);
    const style = details.querySelectorAll('select')[0];
    act(() => {
      style.value = 'high';
      style.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(details.querySelectorAll('li')).toHaveLength(1);
    expect(details.textContent).toContain(high.name);
    expect(details.textContent).not.toContain(drive.name);
    const select = details.querySelector<HTMLButtonElement>('li button')!;
    expect(select.textContent).toBe('Select');
    expect(select.disabled).toBe(false);
    act(() => { select.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    expect(installedSelect.value).toBe(installed.id);
    expect(installedSelect.selectedOptions[0]?.textContent).toContain(high.name);
    expect(localStorage.getItem('studio.rvc.voiceId')).toBe(installed.id);
    expect(fetchStudio.mock.calls.some(([path]) => path === '/v1/rvc/catalog/install')).toBe(false);
  } finally {
    act(() => root.unmount());
    query.clear();
    host.remove();
    localStorage.removeItem('language');
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});
