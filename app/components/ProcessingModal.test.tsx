import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import type { Song } from '../types';
import { ProcessingModal } from './ProcessingModal';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('./AudioWaveform', () => ({ AudioWaveform: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('posts the selected range and returns the saved library take', async () => {
  vi.useFakeTimers();
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const openTake = vi.fn();
  const requests = vi.fn(async (url: string, _options?: RequestInit) => {
    if (url === '/v1/processing') return Response.json({
      run: { done: true, preview_ready: true, stages: ['normalize'], song_id: 'source/1' },
    });
    if (url === '/v1/processing/keep') return Response.json({
      id: 'new-take', title: 'Edited take', audio_path: 'edited.wav', created_at: '2026-11-21T00:00:00Z',
      metadata: { derived: { from: 'source/1', tool: 'processing' } },
    });
    return Response.json({ started: true });
  });
  vi.stubGlobal('fetch', requests);
  const toggle = (label: string) => act(() => {
    host.querySelector<HTMLButtonElement>(`[role="switch"][aria-label="${label}"]`)!.click();
  });
  const setNumber = (label: string, value: string) => act(() => {
    const target = [...host.querySelectorAll('label')].find(item => item.textContent === label)!.querySelector('input')!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(target, value);
    target.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const start = () => [...host.querySelectorAll('button')].find(button => button.textContent?.trim() === 'processStart')!;
  try {
    await act(async () => root.render(<ProcessingModal
      song={{ id: 'source/1', title: 'Source recording', audioUrl: '/source.wav' } as Song}
      onClose={() => {}} onKept={openTake} />));
    toggle('processNormalize');
    expect(start().disabled).toBe(false);
    toggle('processRegion');
    expect(start().disabled).toBe(true);
    setNumber('processRegionStart', '2.5');
    setNumber('processRegionEnd', '2');
    expect(start().disabled).toBe(true);
    setNumber('processRegionEnd', '3.75');
    expect(start().disabled).toBe(false);
    await act(async () => start().click());
    const [, post] = requests.mock.calls.find(([url]) => url === '/v1/library/songs/source%2F1/process')!;
    expect(JSON.parse(String(post?.body))).toEqual({
      normalize: { peak_clip: 10 }, region: { start_seconds: 2.5, end_seconds: 3.75 },
    });
    await act(async () => vi.advanceTimersByTimeAsync(500));
    const keep = [...host.querySelectorAll('button')].find(button => button.textContent?.trim() === 'processKeep')!;
    await act(async () => keep.click());
    expect(openTake).toHaveBeenCalledWith(expect.objectContaining({
      id: 'new-take', audioUrl: '/v1/library/media/new-take', derived: expect.objectContaining({ from: 'source/1' }),
    }));
  } finally {
    act(() => root.unmount());
    vi.useRealTimers();
    host.remove();
    vi.unstubAllGlobals();
  }
});
