import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { AbletonProject } from './AbletonProject';

vi.mock('../context/I18nContext', async () => {
  const { exportStrings } = await import('../i18n/exportDialog');
  const strings: Record<string, string> = exportStrings.en;
  return { useI18n: () => ({ t: (key: string) => strings[key] ?? key }) };
});

it('keeps the finished project available with a warning for omitted score MIDI', async () => {
  const fetch = vi.fn(async (_url: string, init?: RequestInit) => init?.method === 'POST'
    ? Response.json({
      id: 'ready',
      file_name: 'Project.zip',
      stems: 2,
      midi_parts: 0,
      tempo_from_score: false,
      score_midi_omitted: true,
      bpm: 120,
      meter: '4/4',
      tracks: ['Master', 'Vocals', 'Instrumental'],
      left_off: [],
    })
    : new Response(null, { status: 204 }));
  vi.stubGlobal('fetch', fetch);
  const host = document.createElement('div');
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AbletonProject songId="song" />));
    await act(async () => host.querySelector('button')?.click());
    expect(JSON.parse(String(fetch.mock.calls[0][1]?.body))).toEqual({ song_id: 'song' });
    expect(host.querySelector('[role="status"]')?.textContent).toContain('could not be included as MIDI');
    expect(host.textContent).toContain('Live’s default tempo');
    expect([...host.querySelectorAll('button')].some(button => button.textContent === 'Save…')).toBe(true);
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
