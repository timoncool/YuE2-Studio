import React, { useEffect, useRef, useState } from 'react';
import abcjs from 'abcjs';
import { Download, Play, Square } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { MidiSynth, type PlayNote } from './midi/midiSynth';
import { saveFile } from '../services/saveFile';

/**
 * Engraves an ABC score as notation.
 *
 * The score is the one the model planned (or the one it was given), so the
 * voices it declares - "Vocal" and "Ins" - are drawn as separate staves the way
 * a musician would read them. Rendering is local; nothing is fetched.
 *
 * It can be heard and saved as MIDI too: the notes abcjs reads from the score
 * play on the studio's own MIDI voices, so no sound font is downloaded.
 */
export const ScoreView: React.FC<{ abc: string; className?: string; title?: string }> = ({ abc, className, title }) => {
  const { t } = useI18n();
  const host = useRef<HTMLDivElement | null>(null);
  const tune = useRef<abcjs.TuneObject | null>(null);
  const synth = useRef<MidiSynth | null>(null);
  const [failed, setFailed] = useState(false);
  const [playing, setPlaying] = useState(false);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    // Typing into the score editor re-engraves on every keystroke; a short
    // pause keeps that from stuttering on a long song.
    const timer = window.setTimeout(() => {
      try {
        const rendered = abcjs.renderAbc(element, abc, {
          responsive: 'resize',
          add_classes: true,
          paddingtop: 4,
          paddingbottom: 4,
          paddingleft: 4,
          paddingright: 4,
          staffwidth: 740,
          wrap: { minSpacing: 1.6, maxSpacing: 2.8, preferredMeasuresPerLine: 4 },
          foregroundColor: 'currentColor',
        });
        const empty = !rendered?.length || rendered[0].lines.length === 0;
        tune.current = empty ? null : rendered[0];
        setFailed(empty);
      } catch {
        tune.current = null;
        setFailed(true);
      }
    }, 250);
    return () => window.clearTimeout(timer);
  }, [abc]);

  useEffect(() => () => synth.current?.dispose(), []);
  // a new score stops the old one
  useEffect(() => {
    synth.current?.pause();
    setPlaying(false);
  }, [abc]);

  const listen = async () => {
    if (playing) {
      synth.current?.pause();
      setPlaying(false);
      return;
    }
    const score = tune.current;
    if (!score) return;
    const notes = scoreNotes(score);
    if (!notes.length) return;
    synth.current ??= new MidiSynth();
    synth.current.onEnded = () => setPlaying(false);
    await synth.current.playAlone(notes);
    setPlaying(true);
  };

  const saveMidi = async () => {
    const score = tune.current;
    if (!score) return;
    // abcjs 6 answers one tune with its bytes, several with a list of them
    const made = abcjs.synth.getMidiFile(score, { midiOutputType: 'binary' }) as Uint8Array | Uint8Array[];
    const bytes = made instanceof Uint8Array ? made : made[0];
    if (!bytes?.length) return;
    await saveFile(`${(title || 'score').replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'score'}.mid`, { blob: new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'audio/midi' }) });
  };

  const button = 'inline-flex items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 text-[11px] font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300';
  return (
    <div className={className}>
      {!failed && (
        <div className="mb-1 flex justify-end gap-1.5">
          <button type="button" onClick={() => void listen()} className={button} title={t('scoreListenHint')}>
            {playing ? <Square size={12} /> : <Play size={12} />}{playing ? t('scoreStop') : t('scoreListen')}
          </button>
          <button type="button" onClick={() => void saveMidi()} className={button}>
            <Download size={12} />MIDI
          </button>
        </div>
      )}
      <div ref={host} className="score-view text-zinc-900" />
      {failed && <p className="p-2 text-[11px] text-zinc-500">ABC</p>}
    </div>
  );
};

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
