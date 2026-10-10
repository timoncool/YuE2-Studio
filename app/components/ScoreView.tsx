import React, { useEffect, useRef, useState } from 'react';
import abcjs from 'abcjs';
import { ArrowDown, ArrowUp, Download, FileText, Guitar, Pause, Pencil, Play } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { MidiSynth, type PlayNote } from './midi/midiSynth';
import { cueIndex, scoreCues, scoreNotes, type Cue } from '../services/scorePlayback';
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
export const ScoreView: React.FC<{ abc: string; className?: string; title?: string; onChange?: (abc: string) => void; onEdit?: () => void }> = ({ abc, className, title, onChange, onEdit }) => {
  const { t } = useI18n();
  const host = useRef<HTMLDivElement | null>(null);
  const tune = useRef<abcjs.TuneObject | null>(null);
  const synth = useRef<MidiSynth | null>(null);
  const [failed, setFailed] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [voice, setVoice] = useState<{ middle: number; inRange: boolean } | null>(null);
  // the playback cursor follows the synth's own clock, so what is marked is what sounds
  const notes = useRef<PlayNote[]>([]);
  const cues = useRef<Cue[]>([]);
  const cursor = useRef<SVGLineElement | null>(null);
  const shown = useRef(-1);
  const frame = useRef<number | null>(null);
  const shownSecond = useRef(-1);
  const [position, setPosition] = useState(0);
  const [length, setLength] = useState(0);

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
          selectionColor: '#db2777',
          clickListener: element => seekToElement.current(element),
        });
        const empty = !rendered?.length || rendered[0].lines.length === 0;
        tune.current = empty ? null : rendered[0];
        notes.current = empty ? [] : scoreNotes(rendered[0]);
        cues.current = empty ? [] : scoreCues(rendered[0]);
        cursor.current = null;
        shown.current = -1;
        setLength(notes.current.reduce((end, note) => Math.max(end, note.start + note.duration), 0));
        setFailed(empty);
      } catch {
        tune.current = null;
        notes.current = [];
        cues.current = [];
        setFailed(true);
      }
    }, 250);
    return () => window.clearTimeout(timer);
  }, [abc]);

  const stopFollowing = () => {
    if (frame.current !== null) cancelAnimationFrame(frame.current);
    frame.current = null;
  };

  /** Marks the notes sounding at `seconds` and puts the cursor before them; nothing before the first note. */
  const showAt = (seconds: number) => {
    const index = cueIndex(cues.current, seconds);
    if (index === shown.current) return;
    const before = cues.current[shown.current];
    for (const element of before?.elements ?? []) element.classList.remove('score-sounding');
    shown.current = index;
    const cue = cues.current[index];
    if (!cue) {
      cursor.current?.setAttribute('visibility', 'hidden');
      return;
    }
    for (const element of cue.elements) element.classList.add('score-sounding');
    const line = cursorLine(host.current, cursor);
    if (!line) return;
    line.setAttribute('x1', String(cue.left - 2));
    line.setAttribute('x2', String(cue.left - 2));
    line.setAttribute('y1', String(cue.top));
    line.setAttribute('y2', String(cue.top + cue.height));
    line.setAttribute('visibility', 'visible');
    // a new line of music scrolls into view; notes on the same line do not move the page
    if (!before || before.top !== cue.top) cue.elements[0]?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  };

  const showSecond = (seconds: number) => {
    const rounded = Math.round(seconds * 10) / 10;
    if (rounded === shownSecond.current) return;
    shownSecond.current = rounded;
    setPosition(rounded);
  };

  const follow = () => {
    const now = synth.current?.currentTime ?? 0;
    showAt(now);
    showSecond(now);
    frame.current = requestAnimationFrame(follow);
  };

  const finished = () => {
    stopFollowing();
    setPlaying(false);
    showAt(-1);
    showSecond(0);
  };

  const play = async (from: number) => {
    if (!notes.current.length) return;
    synth.current ??= new MidiSynth();
    synth.current.onEnded = finished;
    await synth.current.playAlone(notes.current, from);
    setPlaying(true);
    stopFollowing();
    frame.current = requestAnimationFrame(follow);
  };

  const pause = () => {
    const at = synth.current?.currentTime ?? position;
    synth.current?.pause();
    stopFollowing();
    setPlaying(false);
    showAt(at);
    showSecond(at);
  };

  /** Moves playback to `seconds`: playing goes on from there, paused waits there. */
  const seek = (seconds: number) => {
    const at = Math.max(0, Math.min(length, seconds));
    showAt(at);
    showSecond(at);
    if (playing) void play(at);
  };

  const seekToElement = useRef<(element: abcjs.AbcElem) => void>(() => {});
  seekToElement.current = element => {
    const drawn = new Set<Element>((element.abselem as { elemset?: Element[] } | undefined)?.elemset ?? []);
    const cue = cues.current.find(candidate => candidate.elements.some(part => drawn.has(part)));
    if (cue) seek(cue.at);
  };

  useEffect(() => () => {
    stopFollowing();
    synth.current?.dispose();
  }, []);
  // a new score stops the old one and starts from its beginning
  useEffect(() => {
    synth.current?.pause();
    stopFollowing();
    setPlaying(false);
    shownSecond.current = -1;
    setPosition(0);
  }, [abc]);

  const listen = () => {
    if (playing) {
      pause();
      return;
    }
    void play(position >= length - 0.05 ? 0 : position);
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
        <div className="sticky top-0 z-10 mb-1 flex flex-wrap items-center gap-1.5 bg-white py-0.5">
          <button type="button" onClick={listen} className={button} title={t('scoreListenHint')}>
            {playing ? <Pause size={12} /> : <Play size={12} />}{playing ? t('scorePause') : t('scoreListen')}
          </button>
          <input
            type="range"
            min={0}
            max={Math.max(length, 0.1)}
            step={0.1}
            value={Math.min(position, length)}
            onChange={event => seek(Number(event.target.value))}
            title={t('scoreSeekHint')}
            aria-label={t('scoreSeekHint')}
            className="h-1 min-w-24 flex-1 cursor-pointer accent-pink-500"
          />
          <span className="font-mono text-[10px] tabular-nums text-zinc-500">{clock(position)} / {clock(length)}</span>
          {onEdit && (
            <button type="button" onClick={onEdit} className={button} title={t('scoreEditHint')}>
              <Pencil size={12} />{t('scoreEdit')}
            </button>
          )}
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

/** The cursor line in the engraved score, made on first use; the engraving replaces it on a new score. */
function cursorLine(host: HTMLDivElement | null, kept: React.MutableRefObject<SVGLineElement | null>): SVGLineElement | null {
  if (kept.current?.isConnected) return kept.current;
  const svg = host?.querySelector('svg');
  if (!svg) return null;
  const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
  line.setAttribute('class', 'score-cursor');
  line.setAttribute('stroke', '#ec4899');
  line.setAttribute('stroke-width', '2');
  line.setAttribute('pointer-events', 'none');
  svg.appendChild(line);
  kept.current = line;
  return line;
}

function clock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
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
