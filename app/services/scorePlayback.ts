import abcjs from 'abcjs';
import type { PlayNote } from '../components/midi/midiSynth';

/**
 * The notes of an engraved score in seconds. abcjs counts time in whole notes
 * at the score's tempo in quarter notes per minute; the first voice - the
 * melody that is sung - gets a soft sustained voice, the others a piano.
 */
export function scoreNotes(score: abcjs.TuneObject): PlayNote[] {
  const audio = score.setUpAudio({});
  const wholeSeconds = 240 / (audio.tempo || 120);
  return audio.tracks.flatMap((track, index) =>
    track
      .filter((item): item is abcjs.AudioTrackNoteItem => item.cmd === 'note')
      .map(note => ({
        pitch: note.pitch,
        start: note.start * wholeSeconds,
        duration: Math.max(0.05, (note.duration - (note.gap || 0)) * wholeSeconds),
        family: index === 0 ? 'flute' : 'piano',
      })),
  );
}

/** One moment of the score where notes start: when, in the synth's seconds, and where it is drawn. */
export type Cue = { at: number; left: number; top: number; height: number; elements: Element[] };

/**
 * Where each moment of an engraved score is drawn and when it sounds. abcjs
 * times its events at the score's beat; they are put on the same clock as
 * scoreNotes - whole notes at the tempo in quarter notes - so the cursor and
 * the sound agree.
 */
export function scoreCues(score: abcjs.TuneObject): Cue[] {
  const audio = score.setUpAudio({});
  const wholeSeconds = 240 / (audio.tempo || 120);
  const wholesPerSecond = (score.getBeatLength() * score.getBpm()) / 60;
  score.setTiming(0, 0);
  const timings = (score as unknown as { noteTimings?: abcjs.NoteTimingEvent[] }).noteTimings ?? [];
  return timings
    .filter(event => event.type === 'event' && event.left !== undefined && event.top !== undefined && event.height !== undefined)
    .map(event => ({
      at: (event.milliseconds / 1000) * wholesPerSecond * wholeSeconds,
      left: event.left as number,
      top: event.top as number,
      height: event.height as number,
      elements: (event.elements ?? []).flat() as Element[],
    }));
}

/** The last cue that has started by `seconds`, or -1 before the first. */
export function cueIndex(cues: { at: number }[], seconds: number): number {
  let low = 0;
  let high = cues.length - 1;
  let found = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (cues[middle].at <= seconds + 0.01) {
      found = middle;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found;
}
