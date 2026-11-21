import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { RvcApplyVoice } from './RvcApplyVoice';
import { RvcNativeRuntime } from './RvcNativeRuntime';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });

it('keeps explicit RVC backend selection and saves an audio.cpp take from the selected song', async () => {
  const source = { id: 'original', title: 'Original song', stems: ['vocals', 'instrumental'] };
  const voice = { id: 'voice', model_name: 'voice.pth', index_name: null, version: 2 };
  const ready = { effective_backend: 'audio.cpp', backends: {
    'vocal-fast-rs': { ready: false, reason: 'Unavailable on this machine.' },
    'audio.cpp': { ready: true, missing_bytes: 0 },
  } };
  const fetchRvc = vi.fn(async (url: string, _init?: RequestInit) => Response.json(url.endsWith('/setup') ? ready : {
    song_id: 'new-take', audio_url: '/v1/library/media/new-take',
    requested_backend: 'audio.cpp', effective_backend: 'audio.cpp',
  }));
  vi.stubGlobal('fetch', fetchRvc);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const saved = vi.fn();
  try {
    act(() => root.render(<RvcApplyVoice song={source} voice={voice} control="" onSaved={saved} />));
    await flush();
    const select = host.querySelector('select')!;
    const apply = host.querySelector('button')!;
    expect(host.textContent).toContain('The original remains in the library.');
    const choose = (value: string) => act(() => {
      select.value = value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    choose('vocal-fast-rs');
    expect(apply.disabled).toBe(true);
    expect(host.textContent).toContain('Unavailable on this machine.');
    choose('audio.cpp');
    expect(localStorage.getItem('studio.rvc.backend')).toBe('audio.cpp');
    expect(apply.disabled).toBe(false);
    act(() => { apply.click(); });
    await flush();
    const [, init] = fetchRvc.mock.calls.find(([url]) => url === '/v1/rvc/apply')!;
    expect(JSON.parse(String(init?.body))).toEqual({
      song_id: source.id, voice_id: voice.id, semitone_shift: 0, retrieval_blend: 0, backend: 'audio.cpp',
    });
    expect(saved).toHaveBeenCalledOnce();
    expect(host.textContent).toContain('Saved a new take with audio.cpp.');
    expect(host.querySelector('audio')?.getAttribute('src')).toContain('/v1/library/media/new-take');
  } finally {
    act(() => root.unmount());
    host.remove();
    localStorage.removeItem('studio.rvc.backend');
    vi.unstubAllGlobals();
  }
});

it('starts native runtime installation, reports progress, and refreshes conversion availability', async () => {
  let installing = false;
  let installed = false;
  vi.useFakeTimers();
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => {
    if (init?.method === 'POST') {
      installing = true;
      return Response.json({ started: true }, { status: 202 });
    }
    return Response.json({
      supported: true, installed, reason: null, missing_bytes: installed ? 0 : 200_000_000,
      download: installing ? { downloaded_bytes: 50_000_000, total_bytes: 200_000_000, done: installed } : null,
    });
  }));
  const host = document.createElement('div');
  const root = createRoot(host);
  const onInstalled = vi.fn();
  try {
    await act(async () => root.render(<RvcNativeRuntime onInstalled={onInstalled} disabled={false} />));
    expect(host.querySelector('button')?.textContent).toContain('200');
    await act(async () => host.querySelector('button')!.click());
    expect(host.querySelector('progress')?.value).toBe(50_000_000);
    installed = true;
    await act(async () => vi.advanceTimersByTimeAsync(500));
    expect(onInstalled).toHaveBeenCalledOnce();
    expect(host.querySelector('button')).toBeNull();
  } finally {
    act(() => root.unmount());
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});
