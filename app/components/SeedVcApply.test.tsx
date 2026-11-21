import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { SeedVcApply } from './SeedVcApply';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));

it('uploads the sample, converts the selected source and exposes the separate take', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const requests = vi.fn(async (url: string, init?: RequestInit) => {
    if (url.endsWith('/processing/reference')) return Response.json({ upload_id: 'reference-42.wav' });
    if (init?.method === 'POST') return Response.json({ song_id: 'new-take', audio_url: '/v1/library/media/new-take' });
    return Response.json({ ready: true, model_ready: true, runtime_ready: true });
  });
  vi.stubGlobal('fetch', requests);
  const host = document.createElement('div');
  const root = createRoot(host);
  const onSaved = vi.fn();
  try {
    await act(async () => root.render(<SeedVcApply song={{ id: 'original', title: 'Original', stems: ['vocals', 'instrumental'] }}
      control="" onSaved={onSaved} />));
    await act(async () => host.querySelector('details')?.setAttribute('open', ''));
    const button = () => [...host.querySelectorAll('button')].find(item => item.textContent === 'Convert with Seed-VC')!;
    expect(button().disabled).toBe(true);
    const file = new File(['sample audio'], 'voice.wav', { type: 'audio/wav' });
    const input = host.querySelector('input')!;
    await act(async () => {
      Object.defineProperty(input, 'files', { value: [file] });
      input.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => button().click());
    const upload = requests.mock.calls.find(([url]) => url.endsWith('/processing/reference'))!;
    expect(((upload[1]?.body as FormData).get('audio') as File).name).toBe('voice.wav');
    const conversion = requests.mock.calls.find(([url, init]) => url.endsWith('/seed-vc') && init?.method === 'POST')!;
    expect(JSON.parse(String(conversion[1]?.body))).toEqual({ song_id: 'original', upload_id: 'reference-42.wav' });
    expect(host.querySelector('audio')?.getAttribute('src')).toMatch(/\/v1\/library\/media\/new-take$/);
    expect(onSaved).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
