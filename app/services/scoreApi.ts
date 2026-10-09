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

export type SectionBlock = {
  index: number;
  tag: string | null;
  new_tag: string | null;
  section: number | null;
  first_line_seconds: number | null;
  status: 'kept' | 'renamed' | 'merged' | 'unsure' | 'dropped';
};

export type SectionProposal = {
  lyrics: string;
  blocks: SectionBlock[];
  filled: { section: number; tag: string; copied_from: number | null }[];
  unchanged: boolean;
};

/** The cover's lyrics retagged with the score sections they are sung in, heard in the source recording. Nothing is applied. */
export async function matchSections(input: { abc: string; lyrics: string; songId?: string | null; file?: File | null }): Promise<Answer<{ proposal: SectionProposal }>> {
  const form = new FormData();
  form.append('abc', input.abc);
  form.append('lyrics', input.lyrics);
  if (input.file) form.append('audio', input.file, input.file.name);
  else if (input.songId) form.append('song_id', input.songId);
  const response = await fetch('/v1/score/sections', { method: 'POST', body: form });
  const payload = (await response.json().catch(() => null)) as (SectionProposal & { error?: string }) | null;
  if (!response.ok || !payload) return { ok: false, error: payload?.error ?? `HTTP ${response.status}` };
  return { ok: true, proposal: payload };
}

/** The vocal line moved by whole octaves (0 only measures it), with its middle against the range the model's own scores keep. */
export async function vocalOctave(abc: string, octaves: number): Promise<Answer<{ abc: string; middle: number | null; in_range: boolean; range: [number, number] }>> {
  return post('/v1/score/vocal-octave', { abc, octaves });
}
