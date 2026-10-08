type Failure = { ok: false; error: string };

/** What a score route answers: a problem with the score is `ok: false` and the reason, to show as it is. */
type Answer<T> = ({ ok: true } & T) | Failure;

export function failed<T>(answer: Answer<T>): answer is Failure {
  return !answer.ok;
}

async function post<T>(path: string, body: unknown): Promise<Answer<T>> {
  const response = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  const payload = (await response.json().catch(() => null)) as Answer<T> | null;
  if (payload && typeof payload === 'object' && 'ok' in payload) return payload;
  return { ok: false, error: `HTTP ${response.status}` };
}

/** The score as a MIDI file: voice, instrument and chords on tracks of their own, with the tempo, meters, keys and sections. */
export async function scoreMidi(abc: string): Promise<Answer<{ bytes: Uint8Array }>> {
  const answer = await post<{ data: string }>('/v1/score/midi', { abc });
  if (failed(answer)) return answer;
  const raw = atob(answer.data);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i += 1) bytes[i] = raw.charCodeAt(i);
  return { ok: true, bytes };
}

/** The score made instrumental: every Vocal note moved to Ins, Vocal left with its rests and chords. */
export async function instrumentalScore(abc: string): Promise<Answer<{ abc: string; moved: number; trimmed: number; dropped: number }>> {
  return post('/v1/score/instrumental', { abc });
}

export async function markScore(abc: string, style: string, lyrics: string, cot: string, keep: boolean): Promise<Answer<{ abc: string }>> {
  return post('/v1/score/mark', { abc, style, lyrics, cot, keep });
}
