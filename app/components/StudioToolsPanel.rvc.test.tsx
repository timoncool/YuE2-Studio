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
  let refreshes = 0;
  const updatedName = `${high.name} (updated)`;
  const fetchStudio = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/v1/rvc/catalog/refresh') {
      expect(init?.method).toBe('POST');
      refreshes += 1;
      return refreshes === 1 ? Response.json({ voices: bundled.voices.length, source: 'signed' })
        : Response.json({ error: 'Offline' }, { status: 503 });
    }
    if (url === '/v1/rvc/voices') return Response.json([installed]);
    if (url === '/v1/rvc/catalog') {
      const list = bundled.voices.map(voice => ({
        ...voice, installed_id: voice.id === high.id ? installed.id : null,
        name: refreshes && voice.id === high.id ? updatedName : voice.name,
        sample_audio_url: 'sample' in voice ? `/v1/rvc/catalog/sample/${voice.id}` : null,
      }));
      return Response.json(list);
    }
    if (url === '/v1/library/songs') return Response.json([]);
    if (url === '/v1/separation/status') return Response.json({ run: null, model: { installed: false }, stems: [] });
    if (url === '/v1/midi') return Response.json({ run: null, sizes: [] });
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
    expect(details.textContent).toContain('No published demo');
    const select = details.querySelector<HTMLButtonElement>('li button')!;
    expect(select.textContent).toBe('Select');
    expect(select.disabled).toBe(false);
    act(() => { select.dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    expect(installedSelect.value).toBe(installed.id);
    expect(installedSelect.selectedOptions[0]?.textContent).toContain(high.name);
    expect(localStorage.getItem('studio.rvc.voiceId')).toBe(installed.id);
    expect(fetchStudio.mock.calls.some(([path]) => path === '/v1/rvc/catalog/install')).toBe(false);
    const refresh = [...details.querySelectorAll('button')].find(button => button.textContent === 'Update catalog')!;
    act(() => refresh.click());
    await flush();
    expect(details.textContent).toContain('Catalog updated');
    expect(details.querySelectorAll('li')).toHaveLength(1);
    expect(details.textContent).toContain(updatedName);
    act(() => refresh.click());
    await flush();
    expect(details.querySelector('[role=alert]')?.textContent).toContain('Could not update the catalog.');
    expect(details.querySelectorAll('li')).toHaveLength(1);
    expect(details.textContent).toContain(updatedName);

    const clear = [...details.querySelectorAll('button')].find(button => button.textContent === 'Clear filters')!;
    act(() => clear.click());
    act(() => details.querySelector<HTMLInputElement>('input[type=checkbox]')!.click());
    expect(details.querySelectorAll('li')).toHaveLength(37);
    expect(details.querySelector('audio')).toBeNull();
    const play = [...details.querySelectorAll('button')].find(button => button.textContent === 'Play source demo')!;
    act(() => play.click());
    const audio = details.querySelector('audio')!;
    expect(audio.getAttribute('src')).toContain('/v1/rvc/catalog/sample/');
    act(() => audio.dispatchEvent(new Event('error')));
    expect(details.textContent).toContain('Demo unavailable. Try again.');
    const retry = [...details.querySelectorAll('button')].find(button => button.textContent === 'Play source demo')!;
    act(() => retry.click());
    expect(details.querySelector('audio')).not.toBeNull();
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

it('applies the selected RVC voice to the current Studio Tools song', async () => {
  const track = { id: 'source-song', audio_path: 'source.wav', title: 'RVC source song', engine_id: 'manual',
    caption: '', lyrics: '', metadata: {}, generation_settings: {}, source: 'manual' };
  const voice = { id: 'installed-voice', model_name: 'voice.pth', index_name: null, version: 1 };
  const fetchStudio = vi.fn(async (url: string, _init?: RequestInit) => {
    if (url === '/v1/rvc/voices') return Response.json([voice]);
    if (url === '/v1/library/songs') return Response.json([track]);
    if (url === '/v1/library/songs/source-song/stems') return Response.json({ stems: ['vocals', 'instrumental'] });
    if (url === '/v1/separation/status') return Response.json({ run: null, model: { installed: false }, stems: [] });
    if (url === '/v1/midi') return Response.json({ run: null, sizes: [] });
    if (url === '/v1/rvc/apply/setup') return Response.json({ effective_backend: 'audio.cpp', backends: {
      'vocal-fast-rs': { ready: false }, 'audio.cpp': { ready: true },
    } });
    if (url === '/v1/rvc/apply') return Response.json({
      song_id: 'new-take', audio_url: '/v1/library/media/new-take', requested_backend: 'auto', effective_backend: 'audio.cpp',
    });
    return Response.json(null);
  });
  vi.stubGlobal('fetch', fetchStudio);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  localStorage.setItem('language', 'en');
  localStorage.setItem('studio.rvc.voiceId', voice.id);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const query = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  try {
    act(() => root.render(<QueryClientProvider client={query}><I18nProvider>
      <StudioToolsPanel initialSongId={track.id} />
    </I18nProvider></QueryClientProvider>));
    await flush();
    await flush();
    const section = host.querySelector<HTMLElement>('section[aria-label="RVC voice catalog"]')!;
    expect(section.textContent).toContain(track.title);
    const button = [...section.querySelectorAll('button')]
      .find(button => button.textContent === 'Save a new take with this voice')!;
    expect(button.disabled).toBe(false);
    act(() => button.click());
    await flush();
    const [, request] = fetchStudio.mock.calls.find(([url]) => url === '/v1/rvc/apply')!;
    expect(JSON.parse(String(request?.body))).toMatchObject({ song_id: track.id, voice_id: voice.id, backend: 'auto' });
    expect(section.textContent).toContain('Saved a new take with audio.cpp.');
    expect(section.querySelector('audio')?.getAttribute('src')).toContain('/v1/library/media/new-take');
  } finally {
    act(() => root.unmount());
    query.clear();
    host.remove();
    localStorage.removeItem('language');
    localStorage.removeItem('studio.rvc.voiceId');
    vi.unstubAllGlobals();
  }
});
