// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from 'vitest';
import { composePlans, transcribe } from './transcription';

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

it('marks every plan against the submitted words despite edits while planning', async () => {
  vi.useFakeTimers();
  const bodies: Record<string, unknown>[] = [];
  const prompt = { style: 'Russian vocals', lyrics: '[Verse]\nOld words', cot: 'full' as const, count: 2 };
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/v1/scores') return new Response(JSON.stringify({ id: 'job' }));
    if (url === '/v1/scores/job') return new Response(JSON.stringify({ status: 'done', abc: 'first', plans: [{ abc: 'first', lm_seed: 7 }, { abc: 'second', lm_seed: 8 }] }));
    const body = JSON.parse(init!.body as string);
    bodies.push(body);
    return new Response(JSON.stringify({ ok: true, abc: `${body.abc}\n%yue2-words 0123456789abcdef` }));
  }));
  const pending = composePlans(prompt);
  prompt.style = 'English vocals'; prompt.lyrics = 'New words';
  await vi.advanceTimersByTimeAsync(1000);
  const plans = await pending;
  expect(bodies).toEqual(['first', 'second'].map(abc => ({ abc, style: 'Russian vocals', lyrics: '[Verse]\nOld words', cot: 'full', keep: false })));
  expect(plans).toEqual([{ abc: 'first\n%yue2-words 0123456789abcdef', lm_seed: 7 }, { abc: 'second\n%yue2-words 0123456789abcdef', lm_seed: 8 }]);
});

it('stops the submitted job when cancellation interrupts its status request', async () => {
  vi.useFakeTimers();
  const controller = new AbortController();
  const calls: string[] = [];
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    calls.push(`${init?.method ?? 'GET'} ${url}`);
    if (url === '/v1/transcriptions') return new Response(JSON.stringify({ id: 'old-source' }));
    if (init?.method === 'POST') return new Response('{}');
    controller.abort();
    throw new DOMException('aborted', 'AbortError');
  }));
  const stopped = expect(transcribe({ songId: 'first' }, true, controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
  await vi.advanceTimersByTimeAsync(1000);
  await stopped;
  expect(calls).toEqual(['POST /v1/transcriptions', 'GET /v1/transcriptions/old-source', 'POST /v1/transcriptions/old-source']);
});

it('never offers an unmarked plan when provenance marking fails', async () => {
  vi.useFakeTimers();
  vi.stubGlobal('fetch', vi.fn(async (url: string) => new Response(JSON.stringify(url === '/v1/scores' ? { id: 'job' } : url === '/v1/scores/job' ? { status: 'done', abc: 'score', plans: [{ abc: 'score' }] } : { ok: false, error: 'mark unavailable' }))));
  const pending = expect(composePlans({ style: 'pop', lyrics: 'words', cot: 'full', count: 1 })).rejects.toThrow('mark unavailable');
  await vi.advanceTimersByTimeAsync(1000);
  await pending;
});
