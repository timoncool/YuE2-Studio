/** The DJ UI uses the studio's existing native stem-separation endpoints. */
export type DjSeparationEngine = 'htdemucs' | 'roformer';
export interface DjSeparationStatus {
  ready: boolean;
  engine: DjSeparationEngine;
  settings: {
    engine?: DjSeparationEngine;
    stems: string[];
    runtime: 'auto' | 'cuda' | 'cpu';
  };
  run: {
    song_id: string;
    started_ms: number;
    done: boolean;
    progress: number;
    error: string | null;
    stems: string[];
  } | null;
}

export async function readDjSeparationStatus(): Promise<DjSeparationStatus> {
  const response = await fetch('/v1/separation/status');
  if (!response.ok) throw new Error('Cannot contact YuE2 native separator.');
  return response.json() as Promise<DjSeparationStatus>;
}

export async function readDjNativeStems(songId: string): Promise<string[]> {
  const response = await fetch('/v1/library/songs/' + encodeURIComponent(songId) + '/stems');
  if (!response.ok) throw new Error('Could not load native stem information.');
  const body: { stems?: string[]; run?: DjSeparationStatus['run'] } = await response.json();
  // A re-separation can replace a previous six-stem job with a two-stem job.
  // Prefer the latest completed job's files to avoid summing stale channels.
  if (body.run?.done && body.run.song_id === songId && body.run.stems?.length) return body.run.stems;
  return Array.isArray(body.stems) ? body.stems : [];
}

export function djStemsForEngine(engine: DjSeparationEngine): string[] {
  return engine === 'roformer'
    ? ['vocals', 'instrumental']
    : ['vocals', 'drums', 'bass', 'guitar', 'piano', 'other'];
}

export async function startDjSeparation(songId: string, engine: DjSeparationEngine): Promise<void> {
  const status = await readDjSeparationStatus();
  if (status.run && !status.run.done) throw new Error('A track is already being separated. Finish that job first.');
  const stems = djStemsForEngine(engine);
  const settingsResponse = await fetch('/v1/separation/settings', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...status.settings, engine, stems }),
  });
  if (!settingsResponse.ok) throw new Error('Cannot configure the native separator.');
  const readiness = await readDjSeparationStatus();
  if (!readiness.ready) {
    const name = engine === 'roformer' ? 'Mel-Band RoFormer' : 'HT-Demucs';
    throw new Error(name + ' is not installed. Open Settings > Models > Stems, install the model/runtime, then try again.');
  }
  const response = await fetch('/v1/library/songs/' + encodeURIComponent(songId) + '/stems', { method: 'POST' });
  if (!response.ok) {
    const body: { error?: string } | null = await response.json().catch(() => null);
    throw new Error(body?.error || 'Separation could not start (' + response.status + ')');
  }
}
