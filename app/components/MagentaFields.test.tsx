import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { MagentaFields, restoreMagentaOptions } from './MagentaFields';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));
vi.mock('../services/studioQueries', () => ({ useLibrarySongs: () => ({ isSuccess: true, data: [{ id: 'new', title: 'Studio take', audioUrl: '/new.wav' }] }) }));

const pick = (host: HTMLElement, title: string) => {
  act(() => host.querySelector<HTMLInputElement>('input[role="combobox"]')!.click());
  act(() => [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(option => option.textContent?.startsWith(title))!.click());
};

it('restores an unavailable reference and can switch between audio and text', () => {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const changed = vi.fn();
  function Form() {
    const [value, setValue] = useState(restoreMagentaOptions({ reference_song_id: 'old', ignored: true }));
    return <MagentaFields value={value} onChange={next => { setValue(next); changed(next); }}
      tracks={[{ id: 'new', title: 'Studio take' }]} />;
  }
  try {
    act(() => root.render(<Form />));
    expect(host.querySelector<HTMLInputElement>('input[role="combobox"]')!.value).toBe('Reference unavailable');
    pick(host, 'Studio take');
    expect(changed).toHaveBeenLastCalledWith({ reference_song_id: 'new' });
    pick(host, 'Use text style');
    expect(changed).toHaveBeenLastCalledWith({});
    expect(restoreMagentaOptions({ reference_song_id: '' })).toEqual({});
  } finally {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  }
});
