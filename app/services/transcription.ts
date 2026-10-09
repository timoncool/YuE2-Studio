/**
 * Jobs whose answer is a score, through the studio service: SheetSage2 reading
 * a recording (a file the user picked or a library track), or YuE2 composing
 * one from a style and lyrics without singing it.
 */

import type { YueCot, YueHarmony, YueSampling } from '../types';
import { failed, markScore } from './scoreApi';

interface ScoreJob {
  id: string;
  status: 'running' | 'done' | 'failed' | 'cancelled';
  abc?: string;
  error?: string;
  plans?: ScorePlan[];
}

export interface ScorePlan { abc: string; lm_seed?: number | null }

const POLL_MS = 1000;

async function awaitScore(route: string, submitted: Response, signal?: AbortSignal): Promise<ScoreJob> {
  const job = (await submitted.json().catch(() => ({}))) as Partial<ScoreJob>;
  if (!submitted.ok || !job.id) throw new Error(job.error || `the engine refused the job (${submitted.status})`);
  const url = `${route}/${encodeURIComponent(job.id)}`;
  for (;;) {
    await new Promise(resolve => window.setTimeout(resolve, POLL_MS));
    if (signal?.aborted) {
      await fetch(url, { method: 'POST' }).catch(() => undefined);
      throw new DOMException('cancelled', 'AbortError');
    }
    const response = await fetch(url, { signal });
    const state = (await response.json().catch(() => ({}))) as Partial<ScoreJob>;
    if (!response.ok) throw new Error(state.error || `job status failed (${response.status})`);
    if (state.status === 'done' && state.abc) return state as ScoreJob;
    if (state.status === 'failed') throw new Error(state.error || 'the engine could not write this score');
    if (state.status === 'cancelled') throw new DOMException('cancelled', 'AbortError');
  }
}

export async function transcribe(source: { file?: File; songId?: string }, melodyOnly: boolean, signal?: AbortSignal): Promise<string> {
  const form = new FormData();
  if (source.file) form.append('audio', source.file, source.file.name);
  else if (source.songId) form.append('song_id', source.songId);
  else throw new Error('no audio to transcribe');
  if (melodyOnly) form.append('melody_only', '1');
  const submitted = await fetch('/v1/transcriptions', { method: 'POST', body: form, signal });
  return (await awaitScore('/v1/transcriptions', submitted, signal)).abc!;
}

/** The planning stage alone: the score YuE2 would sing this prompt from. */
export async function composeScore(
  prompt: { style: string; lyrics: string; cot: YueCot; lmSeed?: number; abcSampling?: YueSampling; harmony?: YueHarmony; opening?: string },
  signal?: AbortSignal,
): Promise<string> {
  const submitted = await fetch('/v1/scores', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ style: prompt.style, lyrics: prompt.lyrics, cot: prompt.cot, lm_seed: prompt.lmSeed, abc_sampling: prompt.abcSampling, harmony: prompt.harmony, abc: prompt.opening }),
    signal,
  });
  return (await awaitScore('/v1/scores', submitted, signal)).abc!;
}

export async function composePlans(prompt: { style: string; lyrics: string; cot: YueCot; lmSeed?: number; abcSampling?: YueSampling; harmony?: YueHarmony; opening?: string; count: number }, signal?: AbortSignal): Promise<ScorePlan[]> {
  // Keep the submitted words even if the form changes while the engine plans.
  const words = { style: prompt.style, lyrics: prompt.lyrics, cot: prompt.cot };
  const submitted = await fetch('/v1/scores', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ style: prompt.style, lyrics: prompt.lyrics, cot: prompt.cot, lm_seed: prompt.lmSeed, abc_sampling: prompt.abcSampling, harmony: prompt.harmony, abc: prompt.opening, lm_batch_size: prompt.count }), signal });
  const answer = await awaitScore('/v1/scores', submitted, signal);
  if (!answer.plans?.length) throw new Error('The composition returned no plans');
  const marked = await Promise.all(answer.plans.map(async plan => {
    const result = await markScore(plan.abc, words.style, words.lyrics, words.cot, false);
    if (failed(result)) throw new Error(result.error);
    return { ...plan, abc: result.abc };
  }));
  if (signal?.aborted) throw new DOMException('cancelled', 'AbortError');
  return marked;
}
