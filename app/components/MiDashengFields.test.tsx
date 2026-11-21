import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { MiDashengFields, miDashengPromptReady, restoreMiDashengPrompt } from './MiDashengFields';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));

it('restores each layer and lets music, effects and ambience submit without speech', () => {
  const host = document.createElement('div');
  const root = createRoot(host);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  const saved = {
    caption: 'Arcade', music: 'Synth chords', effects: 'Coin sounds', environment: 'Indoor arcade',
    speech_text: '', speech_style: 'Previous voice', ignored: true,
  };
  const initial = restoreMiDashengPrompt(JSON.parse(JSON.stringify(saved)));
  const updated = vi.fn();
  function Form() {
    const [value, setValue] = useState(initial);
    return <MiDashengFields value={value} onChange={next => { setValue(next); updated(next); }} />;
  }
  try {
    act(() => root.render(<Form />));
    const fields = [...host.querySelectorAll('textarea')];
    expect(fields.map(field => field.value)).toEqual([
      'Arcade', 'Synth chords', '', 'Previous voice', 'Coin sounds', 'Indoor arcade',
    ]);
    expect(miDashengPromptReady(initial)).toBe(true);
    expect(miDashengPromptReady(restoreMiDashengPrompt({ speech_style: 'A voice' }))).toBe(false);
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(fields[2], 'The lights are still on.');
      fields[2].dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(updated).toHaveBeenCalledWith({ ...initial, speech_text: 'The lights are still on.' });
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
