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
      method: 'POST', body: JSON.stringify({ path: 'X:/old', exclude: [] }),
    })]);
    expect(host.textContent).toContain('legacyLibraryResult');
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});

it('copies selected content and shows the successful count with each failed file', async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const request = vi.fn();
  vi.stubGlobal('fetch', request);
  const host = document.createElement('div');
  const root = createRoot(host);
  const preview = {
    path: 'X:/mixed', space: { total_bytes: 242, required_bytes: 242, available_bytes: 100 },
    songs: [{ id: 'good', title: 'Good', files: [], missing: [] }],
    playlists: [{ id: 'list', name: 'Together', song_ids: ['good', 'missing'] }],
  };
  try {
    await act(async () => root.render(<LegacyLibraryPreview />));
    request.mockResolvedValueOnce(Response.json(preview));
    await act(async () => host.querySelector('button')!.click());
    const submit = host.querySelectorAll('button')[1];
    expect(submit.disabled).toBe(true);
    const covers = [...host.querySelectorAll('label')].find(label => label.textContent?.includes('legacyLibraryCovers'))!;
    await act(async () => covers.querySelector('input')!.click());
    expect(submit.disabled).toBe(false);
    request.mockResolvedValueOnce(Response.json({
      added: 1, already_imported: 0, playlists_added: 0, playlists_already_imported: 0,
      errors: [
        { title: 'Broken', file: 'missing.wav', code: 'missing' },
        { title: 'Broken', file: 'cover.png', code: 'copy_failed', error: 'Permission denied' },
        { title: 'Together', file: 'Together', code: 'playlist_incomplete' },
      ],
    })).mockResolvedValueOnce(Response.json(preview));
    await act(async () => submit.click());
    expect(request.mock.calls[1][1].body).toBe(JSON.stringify({ path: 'X:/mixed', exclude: ['covers'] }));
    expect(host.textContent).toContain('legacyLibraryResult');
    const errors = host.querySelector('[role="alert"]')!;
    expect(errors.textContent).toContain('Broken · missing.wav');
    expect(errors.textContent).toContain('cover.png · Permission denied');
    expect(errors.textContent).toContain('Together · legacyLibraryPlaylistIncomplete');
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
