import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { LegacyLibraryPreview } from './LegacyLibraryPreview';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ t: (key: string) => key, songCount: (count: number) => String(count) }),
}));

it('keeps the current preview when a second folder selection is cancelled', async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const request = vi.fn();
  vi.stubGlobal('fetch', request);
  const host = document.createElement('div');
  const root = createRoot(host);
  try {
    await act(async () => root.render(<LegacyLibraryPreview />));
    const button = host.querySelector('button')!;
    let finish!: (response: Response) => void;
    request.mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve; }));
    await act(async () => button.click());
    expect(button.disabled).toBe(true);
    expect(request.mock.calls[0][1].body).toBe('{"pick":true}');
    await act(async () => finish(Response.json({
      path: 'X:/old', studio: 'MiniMax Music3 Studio', space: { total_bytes: 10, required_bytes: 10, available_bytes: 100 }, songs: [{ id: '1', engine_id: 'minimaxmusic-cpp', title: 'Bass', files: [], missing: [{ kind: 'audio', name: 'bass.wav' }] }],
      playlists: [{ id: 'one', name: 'Set list', song_ids: ['1'] }],
    })));
    expect(host.querySelector('[role="status"]')?.textContent).toContain('MiniMax Music3 Studio');
    expect(host.textContent).toContain('bass.wav');
    expect(host.textContent).toContain('Set list');
    request.mockResolvedValueOnce(Response.json({ cancelled: true }));
    await act(async () => button.click());
    expect(host.textContent).toContain('X:/old');
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(button.disabled).toBe(false);
    request.mockResolvedValueOnce(Response.json({ added: 1, already_imported: 0, playlists_added: 1, playlists_already_imported: 0 }));
    const copy = host.querySelectorAll('button')[1];
    await act(async () => copy.click());
    expect(request.mock.calls).toContainEqual(['/v1/library/migration/import', expect.objectContaining({
      method: 'POST', body: JSON.stringify({ path: 'X:/old' }),
    })]);
    expect(host.textContent).toContain('legacyLibraryResult');
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
