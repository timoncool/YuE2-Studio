import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { expect, it, vi } from 'vitest';
import { SeedVcApply } from './SeedVcApply';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));

it('takes the sample as a library song, shows the piece taken and converts with it into a separate take', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  localStorage.setItem('studio.soundSource.voice-sample', 'file');
  const requests = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/v1/tools/devices') return Response.json({ tools: { voice: { device: 'auto', build: 'CUDA 13', error: null, auto: 'CUDA 13', card: 'CUDA 13' } } });
    if (url.endsWith('/v1/library/import')) return Response.json({ id: 'sample-song', title: 'voice' });
    if (url.endsWith('/reference/from-song')) return Response.json({ upload_id: 'reference-42.wav', from: 'track', start: 0, end: 12, seconds: 12, total_seconds: 12 });
    if (url.includes('/v1/library/songs')) return Response.json([]);
    if (init?.method === 'POST') return Response.json({ song_id: 'new-take', audio_url: '/v1/library/media/new-take' });
    return Response.json({ ready: true, model_ready: true, runtime_ready: true });
  });
  vi.stubGlobal('fetch', requests);
  const host = document.createElement('div');
  const root = createRoot(host);
  const onSaved = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  try {
    await act(async () => root.render(<QueryClientProvider client={client}><SeedVcApply song={{ id: 'original', title: 'Original', stems: ['vocals', 'instrumental'] }}
      converter="seed_vc" onSaved={onSaved} /></QueryClientProvider>));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const button = () => [...host.querySelectorAll('button')].find(item => item.textContent === 'Replace the voice')!;
    expect(button().disabled).toBe(true);
    const input = host.querySelector<HTMLInputElement>('input[type=file]')!;
    await act(async () => {
      Object.defineProperty(input, 'files', { value: [new File(['sample audio'], 'voice.wav', { type: 'audio/wav' })] });
      input.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const upload = requests.mock.calls.find(([url]) => url.endsWith('/v1/library/import'))!;
    expect(((upload[1]?.body as FormData).get('audio') as File).name).toBe('voice.wav');
    const taken = requests.mock.calls.find(([url]) => url.endsWith('/reference/from-song'))!;
    expect(JSON.parse(String(taken[1]?.body))).toEqual({ song_id: 'sample-song' });
    expect(host.textContent).toContain('Shorter than 0:25 — we take it whole.');
    await act(async () => button().click());
    const conversion = requests.mock.calls.find(([url, init]) => url.endsWith('/seed-vc') && init?.method === 'POST')!;
    expect(JSON.parse(String(conversion[1]?.body))).toEqual({ song_id: 'original', upload_id: 'reference-42.wav', model: 'seed_vc' });
    expect(host.querySelector('audio')?.getAttribute('src')).toMatch(/\/v1\/library\/media\/new-take$/);
    expect(onSaved).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    localStorage.removeItem('studio.soundSource.voice-sample');
    vi.unstubAllGlobals();
  }
});
