import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import type { Song } from '../types';
import { ProcessingModal } from './ProcessingModal';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('./AudioWaveform', () => ({ AudioWaveform: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('only posts a valid range when selected', async () => {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const requests = vi.fn(async (_url: string, _options?: RequestInit) => Response.json({ started: true }));
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
      onClose={() => {}} onKept={() => {}} />));
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
  } finally {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
