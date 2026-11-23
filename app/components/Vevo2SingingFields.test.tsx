import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { Vevo2SingingFields, type Vevo2References } from './Vevo2SingingFields';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));
vi.mock('../services/studioQueries', () => ({
  useLibrarySongs: () => ({ isSuccess: true, data: [
    { id: 'voice', title: 'Dry voice', audioUrl: '/voice.wav' },
    { id: 'melody', title: 'Melody', audioUrl: '/melody.wav' },
    { id: 'no-audio', title: 'Lyrics only' },
    { id: 'working', title: 'Generating', isGenerating: true, audioUrl: '/working.wav' },
  ] }),
}));

it('requires an audio-backed voice, restores references and can clear the melody', async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const changed = vi.fn();

  function Form() {
    const [value, setValue] = useState<Vevo2References>({ voice_song_id: 'voice', melody_song_id: 'melody' });
    return <Vevo2SingingFields value={value} controlClassName=""
      onChange={next => { changed(next); setValue(next); }} />;
  }

  try {
    await act(async () => root.render(<Form />));
    const [voice, melody] = host.querySelectorAll<HTMLInputElement>('input[role="combobox"]');
    expect(voice.value).toBe('Dry voice');
    expect(melody.value).toBe('Melody');
    act(() => voice.click());
    expect([...document.querySelectorAll('[role="option"]')].map(option => option.textContent)).toEqual(['Dry voice', 'Melody']);
    act(() => voice.click());
    act(() => melody.click());
    await act(async () => [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(option => option.textContent === 'No melody reference')!.click());
    expect(changed).toHaveBeenCalledWith({ voice_song_id: 'voice', melody_song_id: undefined });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
