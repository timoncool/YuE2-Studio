import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { describe, expect, it, vi } from 'vitest';
import { I18nProvider } from '../context/I18nContext';
import type { Song } from '../types';
import { useGenerations, followsLocalProgress, keepingDeleted, rowsFor, rowTaken, stillWaiting } from './generations';

it.each([true, false])('cancels an unacknowledged request when the job ID arrives (accepted=%s)', async accepted => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const root = createRoot(document.createElement('div'));
  const notify = vi.fn();
  let release!: (response: Response) => void;
  const submitted = new Promise<Response>(resolve => { release = resolve; });
  const job = { id: 'cloud-job', engine_id: 'openrouter', submitted_at: 1, status: 'running', generation_settings: {} };
  const fetcher = vi.fn(async (path: string, init?: RequestInit) => {
    if (path === '/v1/music/jobs' && init?.method === 'POST') return submitted;
    if (path === '/v1/music/jobs/cloud-job' && init?.method === 'POST') {
      return accepted ? Response.json({ ...job, status: 'cancelled' }) : new Response(null, { status: 503 });
    }
    return Response.json([]);
  });
  vi.stubGlobal('fetch', fetcher);
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  let studio!: ReturnType<typeof useGenerations>;
  function Editor() {
    studio = useGenerations({ enabled: false, notify, onFinished: () => {} });
    return null;
  }
  try {
    await act(async () => root.render(createElement(QueryClientProvider, { client },
      createElement(I18nProvider, { children: createElement(Editor) }))));
    let generation!: Promise<void>;
    act(() => { generation = studio.generate({ style: 'piano', lyrics: '', music_source: { kind: 'openrouter', model: 'test/music' } }); });
    await act(async () => studio.cancelAll());
    expect(studio.isGenerating).toBe(true);
    await act(async () => { release(Response.json(job)); await generation; });
    expect(fetcher).toHaveBeenCalledWith('/v1/music/jobs/cloud-job', { method: 'POST' });
    expect(studio.isGenerating).toBe(!accepted);
    if (!accepted) expect(notify).toHaveBeenCalledWith(expect.stringContaining('503'), 'error');
  } finally {
    act(() => root.unmount());
    client.clear();
    vi.unstubAllGlobals();
  }
});

it('keeps cloud jobs and finished cards out of native progress updates', () => {
  const running = { jobId: 'job', isGenerating: true };
  expect(followsLocalProgress(song('native', running))).toBe(true);
  expect(followsLocalProgress(song('cloud', { ...running, ditModel: 'openrouter' }))).toBe(false);
  expect(followsLocalProgress(song('done', { ...running, isGenerating: false }))).toBe(false);
});

const song = (id: string, fields: Partial<Song> = {}): Song => ({
  id,
  title: id,
  style: '',
  lyrics: '',
  coverUrl: '',
  duration: '--:--',
  createdAt: new Date(0),
  tags: [],
  ...fields,
});

describe('the rows of a job of several tracks', () => {
  it('gives a job a row a track, the first of them the card its request began as', () => {
    expect(rowsFor('temp_1', 1)).toEqual(['temp_1']);
    expect(rowsFor('temp_1', 3)).toEqual(['temp_1', 'temp_1~2', 'temp_1~3']);
  });

  it('keeps a card for each track the library does not hold yet, the lower cards leaving first', () => {
    const rows = rowsFor('temp_1', 3);
    const cards = [...rows.map(id => song(id, { jobId: 'job' })), song('temp_2'), song('other', { jobId: 'single' })];
    const rowsOfJob = new Map([['job', rows], ['single', ['other']]]);
    const waiting = (made: Song[], single: Song[] = []) => stillWaiting(cards, new Map([['job', made], ['single', single]]), rowsOfJob).map(entry => entry.id);

    expect(waiting([])).toEqual(['temp_1', 'temp_1~2', 'temp_1~3', 'temp_2', 'other']);
    expect(waiting([song('first')])).toEqual(['temp_1', 'temp_1~2', 'temp_2', 'other']);
    expect(waiting([song('second'), song('first')])).toEqual(['temp_1', 'temp_2', 'other']);
    expect(waiting([song('third'), song('second'), song('first')])).toEqual(['temp_2', 'other']);
    expect(waiting([], [song('made')])).toEqual(['temp_1', 'temp_1~2', 'temp_1~3', 'temp_2']);
  });

  it('draws each song in the row of the card it took the place of, so no row moves as songs arrive', () => {
    const rows = rowsFor('temp_1', 3);
    const [first, second, third] = [song('first'), song('second'), song('third')];

    expect(rowTaken(rows, [first], first)).toBe('temp_1~3');
    expect(rowTaken(rows, [second, first], second)).toBe('temp_1~2');
    expect(rowTaken(rows, [second, first], first)).toBe('temp_1~3');
    expect(rowTaken(rows, [third, second, first], third)).toBe('temp_1');
    expect(rowTaken(['only'], [first], first)).toBe('only');
    expect(rowTaken(['only'], [second, first], second)).toBeUndefined();
  });

  it('keeps the place of a song deleted while its job still runs, so cards and rows do not shift', () => {
    const rows = rowsFor('temp_1', 3);
    const [a, b, c] = [song('a'), song('b'), song('c')];
    const known = new Map([['job', [b, a]], ['old', [song('x')]]]);

    // the first song was deleted: the library lists the second alone, then the third arrives
    const afterDelete = keepingDeleted(new Map([['job', [b]]]), known);
    expect(afterDelete.get('job')?.map(entry => entry.id)).toEqual(['b', 'a']);
    expect(rowTaken(rows, afterDelete.get('job')!, b)).toBe('temp_1~2');
    const cards = rows.map(id => song(id, { jobId: 'job' }));
    expect(stillWaiting(cards, afterDelete, new Map([['job', rows]])).map(entry => entry.id)).toEqual(['temp_1']);

    const withThird = keepingDeleted(new Map([['job', [c, b]]]), afterDelete);
    expect(withThird.get('job')?.map(entry => entry.id)).toEqual(['c', 'b', 'a']);
    expect(rowTaken(rows, withThird.get('job')!, c)).toBe('temp_1');
    expect(stillWaiting(cards, withThird, new Map([['job', rows]]))).toEqual([]);
    // a job whose songs are all deleted is still remembered
    expect(afterDelete.get('old')?.map(entry => entry.id)).toEqual(['x']);
  });
});
