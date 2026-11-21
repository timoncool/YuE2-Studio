// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { onSaving, saveFile } from './saveFile';

vi.mock('./externalLinks', () => ({ isDesktop: () => true }));

const place = { id: 'save-1', name: 'mix.wav', path: 'D:/exports/mix.wav' };

afterEach(() => vi.unstubAllGlobals());

it('returns cancellation without uploading or announcing a pending file', async () => {
  const fetch = vi.fn().mockResolvedValueOnce(Response.json({ cancelled: true }));
  vi.stubGlobal('fetch', fetch);
  const listener = vi.fn();
  const stop = onSaving(listener);
  try {
    expect(await saveFile(place.name, { blob: new Blob(['audio']) })).toBe('cancelled');
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(listener).not.toHaveBeenCalled();
  } finally {
    stop();
  }
});

it.each(['url', 'blob'] as const)('reports %s completion from HTTP without the event bridge', async (kind) => {
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(place))
    .mockResolvedValueOnce(Response.json({ path: place.path, written: 5 }));
  vi.stubGlobal('fetch', fetch);
  const listener = vi.fn();
  const stop = onSaving(listener);
  const blob = new Blob(['audio']);
  try {
    expect(await saveFile(place.name, kind === 'blob' ? { blob } : { url: '/v1/song.wav' })).toBe('saved');
    expect(fetch.mock.calls[1][0]).toMatch(/\/v1\/files\/save\/save-1$/);
    expect(fetch.mock.calls[1][1].body).toEqual(kind === 'blob' ? blob : JSON.stringify({ url: '/v1/song.wav' }));
    expect(listener).toHaveBeenNthCalledWith(1, expect.objectContaining({ id: place.id, state: 'saving' }));
    expect(listener).toHaveBeenNthCalledWith(2, {
      id: place.id, path: place.path, state: 'done', written: 5, total: 5,
    });
  } finally {
    stop();
  }
});

it('surfaces the service error as a failed save when the destination cannot be written', async () => {
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(place))
    .mockResolvedValueOnce(Response.json({ error: 'Disk is full' }, { status: 500 }));
  vi.stubGlobal('fetch', fetch);
  const listener = vi.fn();
  const stop = onSaving(listener);
  try {
    expect(await saveFile(place.name, { blob: new Blob(['audio']) })).toBe('failed');
    expect(listener).toHaveBeenLastCalledWith({
      id: place.id, name: place.name, state: 'error', error: 'Disk is full',
    });
  } finally {
    stop();
  }
});
