import { pitchClass } from './songbook';

export type ChordInstrument = 'guitar' | 'ukulele';

const TUNINGS: Record<ChordInstrument, number[]> = {
  guitar: [40, 45, 50, 55, 59, 64],
  ukulele: [67, 60, 64, 69],
};
const ROOT: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
const QUALITIES: Record<string, number[]> = {
  '': [0, 4, 7], maj: [0, 4, 7], m: [0, 3, 7], min: [0, 3, 7],
  dim: [0, 3, 6], '°': [0, 3, 6], aug: [0, 4, 8], '+': [0, 4, 8],
  sus2: [0, 2, 7], sus4: [0, 5, 7], sus: [0, 5, 7], '5': [0, 7],
  '6': [0, 4, 7, 9], m6: [0, 3, 7, 9], '7': [0, 4, 7, 10], m7: [0, 3, 7, 10],
  maj7: [0, 4, 7, 11], M7: [0, 4, 7, 11], '△7': [0, 4, 7, 11],
  '7sus4': [0, 5, 7, 10], dim7: [0, 3, 6, 9], m7b5: [0, 3, 6, 10],
  'ø7': [0, 3, 6, 10], add9: [0, 2, 4, 7], madd9: [0, 2, 3, 7],
};
const pitch = (root: string, accidental: string) =>
  pitchClass(ROOT[root] + (/[#♯]/.test(accidental) ? 1 : /[b♭]/.test(accidental) ? -1 : 0));

/** A chord shape has every requested pitch class, the declared slash bass,
 *  and at most four strings held down across four consecutive frets. */
export function chordShape(chord: string, instrument: ChordInstrument): number[] | null {
  const name = /^([A-G])([#b♯♭]?)([^/]*)(?:\/([A-G])([#b♯♭]?))?$/.exec(chord.trim());
  if (!name) return null;
  const quality = QUALITIES[name[3]];
  if (!quality) return null;
  const root = pitch(name[1], name[2]);
  const bass = name[4] ? pitch(name[4], name[5]) : null;
  const pitches = new Set(quality.map(interval => pitchClass(root + interval)));
  if (bass !== null) pitches.add(bass);

  const tuning = TUNINGS[instrument];
  let best: number[] | null = null;
  let bestCost = Infinity;
  for (let first = 1; first <= 9; first += 1) {
    const options = tuning.map(open => [-1, 0, first, first + 1, first + 2, first + 3]
      .filter(fret => fret < 0 || pitches.has(pitchClass(open + fret))));
    const frets: number[] = [];
    const visit = (string: number) => {
      if (string !== tuning.length) {
        for (const fret of options[string]) {
          frets[string] = fret;
          visit(string + 1);
        }
        return;
      }
      const sounding = tuning.flatMap((open, i) => frets[i] < 0 ? [] : [open + frets[i]]);
      if (new Set(sounding.map(pitchClass)).size !== pitches.size) return;
      const lowest = pitchClass(Math.min(...sounding));
      if (bass !== null && lowest !== bass) return;
      const held = frets.filter(fret => fret > 0);
      if (held.length > 4) return;
      const muted = frets.length - sounding.length;
      const firstPlayed = frets.findIndex(fret => fret >= 0);
      const lastPlayed = frets.length - 1 - [...frets].reverse().findIndex(fret => fret >= 0);
      const interiorMute = frets.slice(firstPlayed, lastPlayed).includes(-1);
      const highest = Math.max(0, ...held);
      const span = held.length ? highest - Math.min(...held) : 0;
      const cost = highest * 3 + span * 2 + held.length + muted * 3 + (interiorMute ? 8 : 0) + (lowest === root ? 0 : 2);
      if (cost < bestCost) {
        best = [...frets];
        bestCost = cost;
      }
    };
    visit(0);
  }
  return best;
}
