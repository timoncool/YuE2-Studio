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

export function buildSongbook(abc: string, transpose: number) {
  const source = engravedScore(abc);
  const original = abcjs.parseOnly(source);
  const transposed = transpose ? abcjs.strTranspose(source, original, transpose) : source;
  const tune = transpose ? abcjs.parseOnly(transposed)[0] : original[0];
  if (!tune) return { title: '', key: '', lines: [] as SongbookCell[][] };
  const voices = eventsFrom(tune);
  const melody = voices.find(voice => voice.some(event => event.note.lyric?.some(item => item.syllable.trim())))
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
