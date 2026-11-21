import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { HotStepCoverFields, type HotStepCoverDraft } from './HotStepCoverFields';

const mocks = vi.hoisted(() => ({ transcribe: vi.fn() }));
vi.mock('../services/transcription', () => ({ transcribe: mocks.transcribe }));
vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en' }) }));
vi.mock('../services/studioQueries', () => ({ useLibrarySongs: () => ({ data: [
  { id: 'old', title: 'Old track', audioUrl: '/old.wav' },
  { id: 'source', title: 'Source track', audioUrl: '/source.wav' },
] }) }));

it('only uses the melody and chords heard from the currently selected original', async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement('div');
  const root = createRoot(host);
  let finishOld: (abc: string) => void = () => undefined;
  mocks.transcribe.mockImplementationOnce(() => new Promise<string>(resolve => { finishOld = resolve; }));
  mocks.transcribe.mockResolvedValueOnce('Melody and chords of the current track');

  function Form() {
    const [draft, setDraft] = useState<HotStepCoverDraft>({ original: '', abc: '' });
    return <HotStepCoverFields draft={draft} onChange={setDraft} ready refresh={() => undefined} controlClassName="" />;
  }
  const select = (track: string) => act(() => {
    const picker = host.querySelector('select')!;
    picker.value = track;
    picker.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const hear = () => act(async () => host.querySelector('button')!.click());
  try {
    await act(async () => root.render(<Form />));
    select('old');
    await hear();
    select('source');
    await hear();
    expect(mocks.transcribe).toHaveBeenLastCalledWith({ songId: 'source' }, false, expect.any(AbortSignal));
    await act(async () => finishOld('Stale score'));
    expect(host.querySelector('textarea')?.value).toBe('Melody and chords of the current track');
  } finally {
    await act(async () => root.unmount());
  }
});
