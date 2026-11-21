import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { MagentaFields, restoreMagentaOptions } from './MagentaFields';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));

it('restores an unavailable reference and can switch between audio and text', () => {
  const host = document.createElement('div');
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
    const select = host.querySelector('select')!;
    expect(select.value).toBe('old');
    expect(select.selectedOptions[0].disabled).toBe(true);
    act(() => {
      select.value = 'new';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(changed).toHaveBeenLastCalledWith({ reference_song_id: 'new' });
    act(() => {
      select.value = '';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(changed).toHaveBeenLastCalledWith({});
    expect(restoreMagentaOptions({ reference_song_id: '' })).toEqual({});
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
