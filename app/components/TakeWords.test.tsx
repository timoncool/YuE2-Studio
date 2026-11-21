import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { TakeWords } from './TakeWords';
import type { Song } from '../types';
import { recogniseLyrics } from '../services/karaoke';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../services/karaoke', async importOriginal => ({
  ...await importOriginal<typeof import('../services/karaoke')>(), recogniseLyrics: vi.fn(),
}));

it('compares independent transcripts on demand and drops an obsolete pair before sending B', async () => {
  const a = { id: 'one', lyrics: '[Chorus]\nCome back home', audioUrl: '/a' } as Song;
  const b = { ...a, id: 'two', audioUrl: '/b' };
  const host = document.createElement('div');
  const root = createRoot(host);
  const recognise = vi.mocked(recogniseLyrics);
  const choose = vi.fn();
  try {
    await act(async () => root.render(<TakeWords key="ab" left={a} right={b} onChoose={choose} />));
    expect(recognise).not.toHaveBeenCalled();
    recognise.mockResolvedValueOnce('come back home').mockResolvedValueOnce('come home');
    await act(async () => host.querySelector('button')!.click());
    expect(host.textContent).toContain('A: 100% · B: 67%');
    expect(recognise.mock.calls).toEqual([[{ songId: 'one' }], [{ songId: 'two' }]]);
    expect(choose).not.toHaveBeenCalled();
    recognise.mockResolvedValueOnce('come back home').mockResolvedValueOnce('come back home');
    await act(async () => host.querySelector('button')!.click());
    expect(host.textContent).toContain('A: 100% · B: 100%');
    expect(host.textContent).not.toContain('as preferred');
    recognise.mockRejectedValueOnce(new Error('karaoke.no-words')).mockResolvedValueOnce('come back home');
    await act(async () => host.querySelector('button')!.click());
    expect(host.textContent).toContain('A: No score · B: 100%');
    expect(host.textContent).not.toContain('as preferred');
    let finish!: (words: string) => void;
    recognise.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    await act(async () => host.querySelector('button')!.click());
    await act(async () => root.render(<TakeWords key="ba" left={b} right={a} />));
    await act(async () => finish('come back home'));
    expect(recognise).toHaveBeenCalledTimes(7);
    expect(host.textContent).not.toContain('100%');
  } finally {
    await act(async () => root.unmount());
    recognise.mockReset();
  }
});
