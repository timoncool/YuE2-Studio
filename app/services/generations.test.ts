import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { describe, expect, it, vi } from 'vitest';
import { I18nProvider } from '../context/I18nContext';
import type { Song, YueJob, YueRequest } from '../types';
import { useGenerations, followsLocalProgress, keepingDeleted, rowsFor, rowTaken, stillWaiting } from './generations';

it.each([
  { accepted: true, engine: 'openrouter', source: { kind: 'openrouter', model: 'test/music' } },
  { accepted: false, engine: 'openrouter', source: { kind: 'openrouter', model: 'test/music' } },
  { accepted: true, engine: 'audio.cpp', source: { kind: 'audio_cpp', model: 'minimax_music3' } },
] satisfies { accepted: boolean; engine: string; source: YueRequest['music_source'] }[])(
  'labels a $engine card before acknowledgement and cancels when the job ID arrives', async ({ accepted, engine, source }) => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const root = createRoot(document.createElement('div'));
  const notify = vi.fn();
  let release!: (response: Response) => void;
  const submitted = new Promise<Response>(resolve => { release = resolve; });
  const job = { id: 'fixture-job', engine_id: engine, submitted_at: 1, status: 'running', generation_settings: {} };
  const fetcher = vi.fn(async (path: string, init?: RequestInit) => {
    if (path === '/v1/music/jobs' && init?.method === 'POST') return submitted;
    if (path === '/v1/music/jobs/fixture-job' && init?.method === 'POST') {
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
    act(() => { generation = studio.generate({ style: 'piano', lyrics: 'verse', music_source: source }); });
    expect(studio.songs[0]).toMatchObject({ ditModel: engine, isGenerating: true, stage: 'stageGeneratingTrack' });
    await act(async () => studio.cancelAll());
    expect(studio.isGenerating).toBe(true);
    await act(async () => { release(Response.json(job)); await generation; });
    expect(fetcher).toHaveBeenCalledWith('/v1/music/jobs/fixture-job', { method: 'POST' });
    expect(studio.isGenerating).toBe(!accepted);
    if (!accepted) expect(notify).toHaveBeenCalledWith(expect.stringContaining('503'), 'error');
  } finally {
    act(() => root.unmount());
    client.clear();
    vi.unstubAllGlobals();
  }
});

it('keeps cloud, audio.cpp and finished cards out of native progress updates', () => {
  const running = { jobId: 'job', isGenerating: true };
  expect(followsLocalProgress(song('native', { ...running, ditModel: 'yue2-cpp' }))).toBe(true);
  for (const engine of ['hotstep', 'acestep-cpp', 'minimaxmusic-cpp', 'signed-custom']) {
    expect(followsLocalProgress(song(engine, { ...running, ditModel: engine }))).toBe(false);
  }
  expect(followsLocalProgress(song('signed', { ...running, ditModel: 'yue2-cpp', jobId: 'signed-fixture' }))).toBe(false);
  expect(followsLocalProgress(song('unknown', running))).toBe(false);
  expect(followsLocalProgress(song('cloud', { ...running, ditModel: 'openrouter' }))).toBe(false);
  expect(followsLocalProgress(song('music3', { ...running, ditModel: 'audio.cpp' }))).toBe(false);
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

it('updates two jobs independently and keeps an in-flight stop out of the YuE2 log', async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const root = createRoot(document.createElement('div'));
  const native: YueJob = {
    id: 'yue2-job', engine_id: 'yue2-cpp', submitted_at: 2, status: 'running', phase: 'running',
    message: '', style: '', lyrics: '', duration_seconds: 4, generation_settings: {},
  };
  const other: YueJob = {
    ...native, id: 'hotstep-job', engine_id: 'hotstep', submitted_at: 1, status: 'queued',
    phase: 'queued', message: 'Waiting for HOT-Step',
  };
  let listed = [other, native];
  let stop: YueJob = { ...other, status: 'running', message: 'Stopping HOT-Step' };
  const sources: ProgressSource[] = [];
  class ProgressSource {
    onmessage?: (event: { data: string }) => void;
    constructor() { sources.push(this); }
    close() {}
  }
  vi.stubGlobal('EventSource', ProgressSource);
  vi.stubGlobal('fetch', vi.fn(async (path: string, init?: RequestInit) => {
    if (path === '/v1/music/jobs') return Response.json(listed);
    if (path === '/v1/music/jobs/hotstep-job' && init?.method === 'POST') {
      listed = stop.status === 'cancelled' ? [native] : [stop, native];
      return Response.json(stop);
    }
    return Response.json([]);
  }));
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  let studio!: ReturnType<typeof useGenerations>;
  function Editor() {
    studio = useGenerations({ enabled: true, notify: () => {}, onFinished: () => {} });
    return null;
  }
  const row = (id: string) => studio.songs.find(entry => entry.jobId === id);
  try {
    await act(async () => root.render(createElement(QueryClientProvider, { client },
      createElement(I18nProvider, { children: createElement(Editor) }))));
    await act(async () => { await client.refetchQueries({ queryKey: ['music', 'jobs'] }); await new Promise(resolve => setTimeout(resolve, 10)); });
    expect(row(other.id)).toMatchObject({ stage: 'stageWaitingInQueue', stageDetail: 'Waiting for HOT-Step' });
    expect(sources).toHaveLength(1);
    act(() => sources[0].onmessage?.({ data: JSON.stringify({ stage: 'semantic', fraction: .4, detail: '4/10' }) }));
    expect(row(native.id)).toMatchObject({ progress: .4, stageDetail: '4/10' });
    expect(row(other.id)).toMatchObject({ stage: 'stageWaitingInQueue', progress: undefined });

    listed = [{ ...other, status: 'running', message: 'HOT-Step is rendering' }, native];
    await act(async () => { await client.refetchQueries({ queryKey: ['music', 'jobs'] }); await new Promise(resolve => setTimeout(resolve, 10)); });
    expect(row(other.id)).toMatchObject({ stage: 'stageGeneratingTrack', stageDetail: 'HOT-Step is rendering', progress: undefined });

    await act(async () => studio.cancel(other.id));
    expect(row(other.id)).toMatchObject({ isGenerating: true, stageDetail: 'Stopping HOT-Step' });
    stop = { ...stop, status: 'cancelled' };
    await act(async () => studio.cancel(other.id));
    expect(row(other.id)).toMatchObject({ isGenerating: false, stage: 'cancelled', stageDetail: undefined });
    act(() => sources[0].onmessage?.({ data: JSON.stringify({ stage: 'semantic', fraction: .5, detail: '5/10' }) }));
    expect(row(native.id)).toMatchObject({ isGenerating: true, progress: .5 });
    expect(row(other.id)).toMatchObject({ stage: 'cancelled', progress: undefined });
  } finally {
    act(() => root.unmount());
    client.clear();
    vi.unstubAllGlobals();
  }
});
