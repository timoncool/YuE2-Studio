import React, { useEffect, useRef, useState } from 'react';
import abcjs from 'abcjs';
import { ArrowDown, ArrowUp, Download, FileText, Guitar, Play, Square } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { MidiSynth, type PlayNote } from './midi/midiSynth';
import { saveFile } from '../services/saveFile';
import { failed as refusedScore, instrumentalScore, vocalOctave } from '../services/scoreApi';
import { engravedScore } from '../services/scoreEngraving';

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
/** A MIDI pitch as a note name with its octave: 60 is C4. */
const noteName = (pitch: number) => `${NOTE_NAMES[((pitch % 12) + 12) % 12]}${Math.floor(pitch / 12) - 1}`;

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
export const ScoreView: React.FC<{ abc: string; className?: string; title?: string; onChange?: (abc: string) => void }> = ({ abc, className, title, onChange }) => {
  const { t } = useI18n();
  const host = useRef<HTMLDivElement | null>(null);
  const tune = useRef<abcjs.TuneObject | null>(null);
  const synth = useRef<MidiSynth | null>(null);
  const [failed, setFailed] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [voice, setVoice] = useState<{ middle: number; inRange: boolean } | null>(null);

  useEffect(() => {
    let current = true;
    const timer = window.setTimeout(() => {
      void vocalOctave(abc, 0).then(answer => {
        if (!current) return;
        setVoice(refusedScore(answer) || answer.middle === null ? null : { middle: answer.middle, inRange: answer.in_range });
      });
    }, 400);
    return () => { current = false; window.clearTimeout(timer); };
  }, [abc]);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    // Typing into the score editor re-engraves on every keystroke; a short
    // pause keeps that from stuttering on a long song.
    const timer = window.setTimeout(() => {
      try {
        const rendered = abcjs.renderAbc(element, engravedScore(abc), {
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

  const savePdf = async () => {
    if (!tune.current) return;
    const bytes = await scorePdf(abc);
    await saveFile(`${fileStem(title)}.pdf`, { blob: new Blob([bytes], { type: 'application/pdf' }) });
  };

  const saveMidi = async () => {
    const score = tune.current;
    if (!score) return;
    // abcjs 6 answers one tune with its bytes, several with a list of them
    const made = abcjs.synth.getMidiFile(score, { midiOutputType: 'binary' }) as Uint8Array | Uint8Array[];
    const bytes = made instanceof Uint8Array ? made : made[0];
    if (!bytes?.length) return;
    await saveFile(`${fileStem(title)}.mid`, { blob: new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'audio/midi' }) });
  };

  const makeInstrumental = async () => {
    if (!onChange) return;
    const answer = await instrumentalScore(abc);
    if (refusedScore(answer)) {
      setNotice(answer.error);
      return;
    }
    onChange(answer.abc);
    setNotice(t('scoreInstrumentalDone').replace('{moved}', String(answer.moved)).replace('{trimmed}', String(answer.trimmed + answer.dropped)));
  };

  const moveVoice = async (octaves: number) => {
    if (!onChange) return;
    const answer = await vocalOctave(abc, octaves);
    if (refusedScore(answer)) {
      setNotice(answer.error);
      return;
    }
    onChange(answer.abc);
    setNotice(t(octaves < 0 ? 'scoreVoiceDownDone' : 'scoreVoiceUpDone').replace('{note}', answer.middle === null ? '' : noteName(answer.middle)));
  };

  const button = 'inline-flex items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 text-[11px] font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300';
  return (
    <div className={className}>
      {!failed && (
        <div className="sticky top-0 z-10 mb-1 flex justify-end gap-1.5 bg-white py-0.5">
          <button type="button" onClick={() => void listen()} className={button} title={t('scoreListenHint')}>
            {playing ? <Square size={12} /> : <Play size={12} />}{playing ? t('scoreStop') : t('scoreListen')}
          </button>
          <button type="button" onClick={() => void saveMidi()} className={button}>
            <Download size={12} />MIDI
          </button>
          <button type="button" onClick={() => void savePdf()} className={button} title={t('scorePdfHint')}>
            <FileText size={12} />PDF
          </button>
          {onChange && (
            <button type="button" onClick={() => void makeInstrumental()} className={button} title={t('scoreInstrumentalHint')}>
              <Guitar size={12} />{t('scoreInstrumental')}
            </button>
          )}
          {onChange && (
            <>
              <button type="button" onClick={() => void moveVoice(-1)} className={button} title={t('scoreVoiceDownHint')}>
                <ArrowDown size={12} />{t('scoreVoiceDown')}
              </button>
              <button type="button" onClick={() => void moveVoice(1)} className={button} title={t('scoreVoiceUpHint')}>
                <ArrowUp size={12} />{t('scoreVoiceUp')}
              </button>
            </>
          )}
        </div>
      )}
      {notice && <p className="mb-1 rounded-md bg-zinc-100 px-2 py-1 text-[11px] leading-4 text-zinc-600">{notice}</p>}
      {voice && !voice.inRange && (
        <p className="mb-1 rounded-md bg-amber-50 px-2 py-1 text-[11px] leading-4 text-amber-700">{t('scoreVoiceOutside').replace('{note}', noteName(voice.middle))}</p>
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

function fileStem(title?: string): string {
  return (title || 'score').replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'score';
}

/**
 * The score as an A4 PDF in vector form. Each system of staves is engraved as
 * its own SVG, so a page break never falls inside a staff.
 */
export async function scorePdf(abc: string): Promise<ArrayBuffer> {
  const [{ jsPDF }] = await Promise.all([import('jspdf'), import('svg2pdf.js')]);
  const host = document.createElement('div');
  host.style.cssText = 'position:fixed;left:-10000px;top:0;width:800px';
  document.body.appendChild(host);
  try {
    abcjs.renderAbc(host, engravedScore(abc), {
      oneSvgPerLine: true,
      staffwidth: 700,
      paddingtop: 0,
      paddingbottom: 8,
      paddingleft: 0,
      paddingright: 0,
      wrap: { minSpacing: 1.6, maxSpacing: 2.8, preferredMeasuresPerLine: 4 },
      foregroundColor: '#000000',
    });
    const doc = new jsPDF({ orientation: 'p', unit: 'pt', format: 'a4' });
    const margin = 36;
    const pageWidth = doc.internal.pageSize.getWidth() - margin * 2;
    const pageBottom = doc.internal.pageSize.getHeight() - margin;
    let y = margin;
    for (const svg of Array.from(host.querySelectorAll('svg'))) {
      const box = svg.viewBox.baseVal;
      const width = box?.width || svg.getBoundingClientRect().width;
      const height = box?.height || svg.getBoundingClientRect().height;
      if (!width || !height) continue;
      const scale = Math.min(1, pageWidth / width);
      if (y > margin && y + height * scale > pageBottom) {
        doc.addPage();
        y = margin;
      }
      await doc.svg(svg, { x: margin, y, width: width * scale, height: height * scale });
      y += height * scale;
    }
    return doc.output('arraybuffer');
  } finally {
    host.remove();
  }
}
