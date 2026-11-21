import type { Song } from '../types';
import { saveFile, type SaveOutcome } from './saveFile';
import { stemOf } from './songStems';
import { librarySongsNow } from './studioQueries';

const EXTENSIONS: [string, string][] = [
  ['wav', 'wav'],
  ['flac', 'flac'],
  ['ogg', 'ogg'],
  ['mp4', 'm4a'],
  ['aac', 'm4a'],
  ['mpeg', 'mp3'],
];

const MODES: Record<string, string> = { '': 'major', maj: 'major', m: 'minor', min: 'minor', dor: 'dorian', phr: 'phrygian', lyd: 'lydian', mix: 'mixolydian', loc: 'locrian', aeo: 'minor', ion: 'major' };

/** "109 BPM, D major" from a score's header, for a file a DAW opens; empty when it has none. */
export function tempoAndKey(abc: unknown): string {
  if (typeof abc !== 'string') return '';
  const tempo = /^Q:1\/4=(\d+)\s*$/m.exec(abc)?.[1];
  const key = /^K:\s*([A-G])([#b]?)\s*([a-zA-Z]*)/m.exec(abc);
  const mode = key ? MODES[key[3].toLowerCase().slice(0, 3)] : undefined;
  const parts = [tempo ? `${tempo} BPM` : '', key && mode ? `${key[1]}${key[2]} ${mode}` : ''].filter(Boolean);
  return parts.join(', ');
}

/** A stem's tempo and key are its song's, read from the song's score. */
export function stemTempoAndKey(song: Pick<Song, 'derived' | 'generationParams'>): string {
  const from = stemOf(song) !== null ? song.derived?.from : undefined;
  const source = from ? librarySongsNow()?.find(candidate => candidate.id === from) : song;
  return tempoAndKey((source?.generationParams as Record<string, unknown> | undefined)?.abc);
}

/** The label of the version that plays, when it is not the original. */
export function activeVersionLabel(song: Song): string | null {
  const active = song.activeVersion;
  if (!active || active === 'original') return null;
  return song.audioVersions?.find(version => version.id === active)?.label || null;
}

/** A file name for the track as it plays now: its title, the processing applied, the stored format. */
export function songFileName(song: Song, contentType: string): string {
  const version = activeVersionLabel(song);
  const named = version ? `${song.title || 'song'} (${version})` : song.title || 'song';
  // a stem goes into a DAW, which wants the song's tempo and key
  const tempoKey = stemOf(song) !== null ? stemTempoAndKey(song) : '';
  const base = tempoKey ? `${named} - ${tempoKey}` : named;
  const safe = base.replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_').replace(/[. ]+$/, '').slice(0, 180);
  const extension = EXTENSIONS.find(([marker]) => contentType.includes(marker))?.[1] ?? 'mp3';
  return `${safe || 'song'}.${extension}`;
}

/** Saves the track's audio as it plays now, where the user says. */
export async function downloadSongAudio(song: Song): Promise<SaveOutcome | undefined> {
  if (!song.audioUrl) return;
  // the name carries the stored format, which only the file's own type tells
  const head = await fetch(song.audioUrl, { method: 'HEAD' });
  if (!head.ok) throw new Error(`HTTP ${head.status}`);
  return saveFile(songFileName(song, head.headers.get('content-type') ?? ''), { url: song.audioUrl });
}
