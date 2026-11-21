/** The DJ UI uses the studio's existing native stem-separation endpoints. */
export type DjSeparationEngine = 'htdemucs' | 'roformer';
export interface DjSeparationStatus {
  ready: boolean;
  engine: DjSeparationEngine;
  /** Native engine readiness is reported independently for both DJ install buttons. */
  engines?: Record<DjSeparationEngine, { ready: boolean }>;
  download?: { asset_id: string; scope?: string; downloaded_bytes: number; total_bytes: number; done: boolean; error: string | null } | null;
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
  if (body.run?.done && !body.run.error && body.run.song_id === songId && body.run.stems?.length) return body.run.stems;
  const stems = Array.isArray(body.stems) ? body.stems : [];
  // If both engines left files, prefer the deck's grouped library takes over an ambiguous blend.
  return stems.includes('instrumental') && stems.some(name => name !== 'instrumental' && name !== 'vocals') ? [] : stems;
}

/** True only when the native backend confirms this exact engine is installed. */
export function djEngineReady(status: DjSeparationStatus | null, engine: DjSeparationEngine): boolean {
  return status?.engines?.[engine]?.ready ?? (status?.engine === engine && status.ready);
}

/** One click installs the selected engine's model and required native runtime. */
export async function installDjSeparationEngine(engine: DjSeparationEngine): Promise<void> {
  const response = await fetch('/v1/separation/runtime/install', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ asset_id: engine }),
  });
  if (!response.ok) {
    const body: { error?: string } | null = await response.json().catch(() => null);
    throw new Error(body?.error || 'Could not start the ' + engine + ' download (' + response.status + ')');
  }
}

export async function startDjSeparation(songId: string, engine: DjSeparationEngine): Promise<void> {
  const response = await fetch('/v1/library/songs/' + encodeURIComponent(songId) + '/stems', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ engine }),
  });
  if (!response.ok) {
    const body: { error?: string } | null = await response.json().catch(() => null);
    throw new Error(body?.error || 'Separation could not start (' + response.status + ')');
  }
}
