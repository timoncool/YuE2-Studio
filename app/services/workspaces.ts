/**
 * Sessions of work, as the service keeps them.
 *
 * A workspace is the studio's session: the songs made in it, and whether it is
 * still open. One workspace is open at a time - what the studio calls the
 * current session - and a new track lands in whichever one is open.
 *
 * Sessions used to live in local storage, which meant the window and the agent
 * each had their own set and could not see each other's. The service owns them
 * now, so a session is one thing for both.
 *
 * The service speaks snake_case and seconds; the window speaks camelCase and
 * milliseconds, the way the rest of the interface measures time.
 */

export interface Workspace {
  id: string;
  name: string;
  /**
   * What the studio calls a session it keeps itself, or nothing for the person's own.
   * `import` marks the session that gathers tracks made outside any session: the mark is
   * what makes it one, so its name (which the window draws in the person's language) is
   * not what anyone looks it up by.
   */
  kind?: string | null;
  songIds: string[];
  openedAt: number | null;
  closedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export type RepeatMode = 'none' | 'all' | 'one' | 'stop';

export interface PlaybackMode {
  repeatMode: RepeatMode;
  shuffle: boolean;
}

interface NativeWorkspace {
  id: string;
  name: string;
  kind?: string | null;
  song_ids: string[];
  opened_at: string | null;
  closed_at: string | null;
  created_at: string;
  updated_at: string;
}

interface NativePlaybackMode {
  repeat_mode: string;
  shuffle: boolean;
  updated_at: string;
}

/** Seconds as the service stores them become milliseconds, or nothing. */
function millis(value: string | null): number | null {
  if (value === null || value === '') return null;
  const seconds = Number(value);
  return Number.isFinite(seconds) ? seconds * 1000 : null;
}

function mapWorkspace(native: NativeWorkspace): Workspace {
  return {
    id: native.id,
    name: native.name,
    kind: native.kind ?? null,
    songIds: Array.isArray(native.song_ids) ? native.song_ids : [],
    openedAt: millis(native.opened_at),
    closedAt: millis(native.closed_at),
    createdAt: millis(native.created_at) ?? 0,
    updatedAt: millis(native.updated_at) ?? 0,
  };
}

function mapPlaybackMode(native: NativePlaybackMode): PlaybackMode {
  const repeat = native.repeat_mode === 'all' || native.repeat_mode === 'one'
    ? native.repeat_mode
    : 'none';
  return { repeatMode: repeat, shuffle: Boolean(native.shuffle) };
}

async function send(path: string, init?: RequestInit): Promise<Response> {
  const response = await fetch(path, init);
  if (!response.ok) {
    throw new Error(`Studio service answered ${response.status} for ${path}`);
  }
  return response;
}

export async function loadWorkspaces(): Promise<Workspace[]> {
  const response = await send('/v1/workspaces');
  const native: NativeWorkspace[] = await response.json();
  return native.map(mapWorkspace);
}

/**
 * The order a session list is read in: the current one first, then the rest by the
 * chosen order. The person works in the open session, so hunting for it down a long
 * list is the one thing they must never have to do. The given list is not touched.
 */
/** What a list can be read by: the day it was made, the day it was last touched, or its name. */
export type SortBy = 'created' | 'updated' | 'name';

export interface SortOrder {
  by: SortBy;
  descending: boolean;
}

/**
 * The list of sessions as the person reads it: the current one always first,
 * then the rest by the chosen order - the day it was made, or its name.
 */
export function orderSessions<T extends { id: string; name: string; createdAt: number; updatedAt?: number }>(
  sessions: T[],
  activeSessionId: string | null,
  order: SortOrder = { by: 'created', descending: true },
): T[] {
  return [...sessions].sort((a, b) => {
    const current = Number(b.id === activeSessionId) - Number(a.id === activeSessionId);
    if (current !== 0) return current;
    const compared = order.by === 'name'
      ? a.name.localeCompare(b.name, 'ru')
      : order.by === 'updated'
        ? (a.updatedAt ?? a.createdAt) - (b.updatedAt ?? b.createdAt)
        : a.createdAt - b.createdAt;
    return order.descending ? -compared : compared;
  });
}

export async function activeWorkspace(): Promise<Workspace | null> {
  const response = await send('/v1/workspaces/active');
  const native: NativeWorkspace | null = await response.json();
  return native ? mapWorkspace(native) : null;
}

/**
 * What a session is called on screen. The studio's own sessions carry a mark instead of a
 * name, and the words for it come from the window's language - never from the library, and
 * never from a lookup by name, which is how two windows once made two of the same session.
 */
export function sessionTitle(session: { name: string; kind?: string | null }, importName: string): string {
  return session.kind === 'import' ? importName : session.name;
}

/**
 * Gather the tracks that belong to no session into the studio's own one. The service picks
 * that session and marks it, so two windows asking at once get the same one; `known` only
 * carries the names an older library may have stored, so the session it already has is
 * adopted instead of a second one being made.
 */
export async function adoptImportSession(known: string[]): Promise<Workspace> {
  const response = await send('/v1/workspaces/import', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ known }),
  });
  return mapWorkspace(await response.json());
}

export async function createWorkspace(name: string, songIds: string[] = []): Promise<Workspace> {
  const response = await send('/v1/workspaces', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, song_ids: songIds }),
  });
  return mapWorkspace(await response.json());
}

/**
 * Renaming a session and setting the songs it holds are the same call: the
 * service takes the whole session, the way it takes a whole playlist.
 */
export async function updateWorkspace(id: string, name: string, songIds: string[]): Promise<Workspace> {
  const response = await send(`/v1/workspaces/${encodeURIComponent(id)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, song_ids: songIds }),
  });
  return mapWorkspace(await response.json());
}

export async function deleteWorkspace(id: string): Promise<void> {
  await send(`/v1/workspaces/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

/** Opening a session closes whichever one was open before it. */
export async function openWorkspace(id: string): Promise<Workspace> {
  const response = await send(`/v1/workspaces/${encodeURIComponent(id)}/open`, { method: 'POST' });
  return mapWorkspace(await response.json());
}

export async function closeWorkspace(id: string): Promise<Workspace> {
  const response = await send(`/v1/workspaces/${encodeURIComponent(id)}/close`, { method: 'POST' });
  return mapWorkspace(await response.json());
}

/**
 * How a queue plays back. The context names the queue itself: `session:<id>`,
 * `playlist:<id>`, `library:all`, `library:liked`, `search`, `single`.
 * Nothing back means the context was never touched: the caller's default stands.
 */
export async function readPlaybackMode(context: string): Promise<PlaybackMode | null> {
  const response = await fetch(`/v1/playback-modes/${encodeURIComponent(context)}`);
  if (response.status === 404) return null;
  if (!response.ok) {
    throw new Error(`Studio service answered ${response.status} for the playback mode`);
  }
  const native: NativePlaybackMode | null = await response.json();
  return native ? mapPlaybackMode(native) : null;
}

export async function writePlaybackMode(context: string, mode: PlaybackMode): Promise<PlaybackMode> {
  const response = await send(`/v1/playback-modes/${encodeURIComponent(context)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ repeat_mode: mode.repeatMode, shuffle: mode.shuffle }),
  });
  return mapPlaybackMode(await response.json());
}
