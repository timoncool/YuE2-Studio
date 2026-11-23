import abcjs from 'abcjs';
import { engravedScore } from './scoreEngraving';

type Note = abcjs.VoiceItemNote & {
  chord?: { name: string; position: string }[];
  lyric?: { syllable: string; divider: string }[];
  midiPitches?: { pitch: number }[];
};

interface Event {
  note: Note;
  at: number;
  line: number;
  key: abcjs.KeySignature;
}

export interface SongbookCell {
  number: string;
  keyChange?: string;
  lyrics: string[];
  attack?: boolean;
  duration?: number;
  chords: string[];
}

const LETTERS = 'CDEFGAB';
const MAJOR = [0, 2, 4, 5, 7, 9, 11];
const SHARP = ['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B'];
const FLAT = ['C', 'D♭', 'D', 'E♭', 'E', 'F', 'G♭', 'G', 'A♭', 'A', 'B♭', 'B'];

export const pitchClass = (pitch: number) => (pitch % 12 + 12) % 12;

function namedPitch(letter: string, accidental: string): number {
  return MAJOR[LETTERS.indexOf(letter)] + (/[#♯]/.test(accidental) ? 1 : /[b♭]/.test(accidental) ? -1 : 0);
}

export function chordName(chord: string, semitones: number): string {
  if (!semitones) return chord;
  return chord.replace(/(^|\/)([A-G])([#b♯♭]?)(?=[^a-z]|$|maj|min|dim|aug|sus|m|add)/g,
    (_, prefix: string, letter: string, accidental: string) => {
      const at = pitchClass(namedPitch(letter, accidental) + semitones);
      return prefix + (/[b♭]/.test(accidental) ? FLAT : SHARP)[at];
    });
}

const keyName = (key: abcjs.KeySignature) => `${key.root}${key.acc}${key.mode}`;

function numberFor({ note, key }: Event): string {
  const pitch = note.pitches?.[0]?.pitch;
  const midi = note.midiPitches?.[0]?.pitch;
  const root = LETTERS.indexOf(key.root);
  if (pitch === undefined || midi === undefined || root < 0) return '0';
  const relative = pitch - root;
  const octave = Math.floor(relative / 7);
  const degree = relative - octave * 7;
  const tonic = 60 + namedPitch(key.root, key.acc);
  const alter = midi - (tonic + 12 * octave + MAJOR[degree]);
  const accidental = alter > 0 ? '♯'.repeat(Math.min(2, alter)) : '♭'.repeat(Math.min(2, -alter));
  return accidental + String(degree + 1) + (octave > 0 ? '\u0307'.repeat(octave) : '\u0323'.repeat(-octave));
}

function eventsFrom(tune: abcjs.TuneObject): Event[][] {
  tune.setUpAudio({});
  const byVoice = new Map<string, { at: number; events: Event[]; key: abcjs.KeySignature }>();
  tune.lines.forEach((line, index) => line.staff?.forEach((staff, stave) => staff.voices.forEach((voice, slot) => {
    const id = `${stave}:${slot}`;
    const cursor = byVoice.get(id) ?? { at: 0, events: [], key: tune.getKeySignature() };
    if (staff.key) cursor.key = staff.key;
    for (const item of voice) {
      if (item.el_type === 'key') cursor.key = item;
      if (item.el_type !== 'note') continue;
      cursor.events.push({ at: cursor.at, key: cursor.key, line: index, note: item });
      cursor.at += item.duration;
    }
    byVoice.set(id, cursor);
  })));
  return Array.from(byVoice.values(), voice => voice.events);
}

export function placeLyricSyllables(lines: SongbookCell[][], syllables: string[] | null): boolean {
  if (!syllables?.length || lines.some(line => line.some(cell => cell.lyrics.some(Boolean)))) return false;
  const attacks = lines.flat().filter(cell => cell.attack);
  if (attacks.length !== syllables.length) return false;
  attacks.forEach((cell, index) => { cell.lyrics = [syllables[index]]; });
  return true;
}

/** A rest this long (in whole notes) between sung notes ends a phrase. */
const PHRASE_GAP = 0.375;

/** The sung phrases of the melody: notes between long rests, with the chords of the gap before each one. */
function phrasesOf(cells: SongbookCell[]): SongbookCell[][] {
  const phrases: SongbookCell[][] = [];
  let current: SongbookCell[] = [];
  let gap: SongbookCell[] = [];
  let resting = 0;
  for (const cell of cells) {
    const rest = cell.number === '0' || cell.number === '';
    if (cell.attack) {
      if (current.length && resting >= PHRASE_GAP) {
        phrases.push(current);
        current = [];
      }
      current.push(...gap, cell);
      gap = [];
      resting = 0;
    } else if (!rest && current.length) {
      current.push(...gap, cell);
      gap = [];
    } else {
      resting += cell.duration ?? 0;
      gap.push(cell);
    }
  }
  if (current.length) phrases.push([...current, ...gap]);
  return phrases;
}

/** Splits `items` into `count` runs in order, each run's weight as close to its target as it can be. */
function group(weights: number[], targets: number[]): number[] {
  const n = weights.length;
  const k = targets.length;
  const sums = [0];
  for (const weight of weights) sums.push(sums[sums.length - 1] + weight);
  const cost = Array.from({ length: k + 1 }, () => Array(n + 1).fill(Infinity));
  const from = Array.from({ length: k + 1 }, () => Array(n + 1).fill(0));
  cost[0][0] = 0;
  for (let run = 1; run <= k; run++) {
    for (let end = run; end <= n - (k - run); end++) {
      for (let start = run - 1; start < end; start++) {
        const value = cost[run - 1][start] + Math.abs(sums[end] - sums[start] - targets[run - 1]);
        if (value < cost[run][end]) {
          cost[run][end] = value;
          from[run][end] = start;
        }
      }
    }
  }
  const ends: number[] = [];
  for (let run = k, end = n; run > 0; run--) {
    ends.unshift(end);
    end = from[run][end];
  }
  return ends;
}

/**
 * The songbook as the song is sung: a row per lyric line, its syllables under the notes of the phrase it is sung on
 * and the chords over them. Lines and phrases are paired in order; where their numbers differ, neighbouring phrases
 * (or lines) are joined so that each row's syllables come closest to its notes.
 */
export function layoutByPhrases(lines: SongbookCell[][], lyrics: string, syllablesOf: (line: string) => string[] | null): SongbookCell[][] | null {
  const words = lyrics.split(/\r?\n/).map(line => line.trim()).filter(line => line && !/^\[.*\]$/.test(line) && /\p{L}/u.test(line));
  const parts = words.map(syllablesOf);
  if (!words.length || parts.some(part => !part)) return null;
  const syllables = parts as string[][];
  const phrases = phrasesOf(lines.flat());
  if (!phrases.length) return null;
  const attacks = phrases.map(phrase => phrase.filter(cell => cell.attack).length);
  const rows: { cells: SongbookCell[]; syllables: string[] }[] = [];
  if (phrases.length >= syllables.length) {
    let start = 0;
    group(attacks, syllables.map(line => line.length)).forEach((end, line) => {
      rows.push({ cells: phrases.slice(start, end).flat(), syllables: syllables[line] });
      start = end;
    });
  } else {
    let start = 0;
    group(syllables.map(line => line.length), attacks).forEach((end, phrase) => {
      rows.push({ cells: phrases[phrase], syllables: syllables.slice(start, end).flat() });
      start = end;
    });
  }
  return rows.map(({ cells, syllables: sung }) => {
    let held = '';
    const row = cells.map(cell => ({ ...cell, lyrics: [] as string[], chords: cell.chords.filter(chord => {
      const changed = chord !== held;
      held = chord;
      return changed;
    }) }));
    const notes = row.filter(cell => cell.attack);
    sung.forEach((syllable, index) => {
      const cell = notes[Math.min(index, notes.length - 1)];
      if (!cell) return;
      cell.lyrics = cell.lyrics.length ? [`${cell.lyrics[0]}${cell.lyrics[0].endsWith('-') ? '' : ' '}${syllable}`] : [syllable];
    });
    return row;
  });
}

/** ABCJS retains note offsets but does not expose a voice's V: identifier. */
function namedVocalLine(source: string, voices: Event[][]): Event[] | undefined {
  // Ignore apparent inline voice switches inside quoted chords or comments.
  const switches = [...source.matchAll(/^[ \t]*V:[ \t]*([^\s\]]+)|"[^"\n]*"|%[^\n]*|\[V:[ \t]*([^\s\]]+)[^\]\n]*\]/gm)]
    .filter(match => match[1] || match[2]);
  if (!switches.some(match => (match[1] ?? match[2]).toLowerCase() === 'vocal')) return undefined;
  return voices.find(voice => {
    const start = voice.find(event => event.note.pitches?.length)?.note.startChar;
    if (start === undefined) return false;
    let current = '';
    for (const change of switches) {
      if (change.index > start) break;
      current = change[1] ?? change[2];
    }
    return current.toLowerCase() === 'vocal';
  });
}

export function buildSongbook(abc: string, transpose: number) {
  const source = engravedScore(abc.replace(/\r\n?/g, '\n'));
  const original = abcjs.parseOnly(source);
  const transposed = transpose ? abcjs.strTranspose(source, original, transpose) : source;
  const tune = transpose ? abcjs.parseOnly(transposed)[0] : original[0];
  if (!tune) return { title: '', key: '', lines: [] as SongbookCell[][] };
  const voices = eventsFrom(tune);
  const melody = namedVocalLine(transposed, voices)
    ?? voices.find(voice => voice.some(event => event.note.lyric?.some(item => item.syllable.trim())))
    ?? voices.find(voice => voice.some(event => event.note.pitches?.length)) ?? [];
  const harmony = voices.flatMap(voice => voice.flatMap(({ at, line, note }) =>
    (note.chord ?? []).filter(chord => chord.position === 'default')
      .map(chord => ({ at, line, chord: chord.name })))).sort((a, b) => a.at - b.at);
  const lines = new Map<number, SongbookCell[]>();
  let chord = 0;
  const initialKey = keyName(tune.getKeySignature());
  let displayedKey = initialKey;
  for (const event of melody) {
    const currentKey = keyName(event.key);
    const keyChange = currentKey === displayedKey ? undefined : currentKey;
    displayedKey = currentKey;
    const cells = lines.get(event.line) ?? [];
    const chords: string[] = [];
    while (chord < harmony.length && harmony[chord].at <= event.at + 1e-7) {
      const name = harmony[chord++].chord;
      if (!chords.includes(name)) chords.push(name);
    }
    cells.push({ chords, number: numberFor(event), keyChange, duration: event.note.duration,
      attack: Boolean(event.note.pitches?.some(pitch => !pitch.endTie)),
      lyrics: (event.note.lyric ?? []).map(item => item.syllable + (item.divider === '-' ? '-' : '')) });
    lines.set(event.line, cells);
  }
  while (chord < harmony.length) {
    const change = harmony[chord++];
    const cells = lines.get(change.line) ?? [];
    cells.push({ chords: [change.chord], number: '', lyrics: [] });
    lines.set(change.line, cells);
  }
  return { title: tune.metaText.title ?? '', key: initialKey,
    lines: [...lines.entries()].sort((a, b) => a[0] - b[0]).map(([, cells]) => cells) };
}
