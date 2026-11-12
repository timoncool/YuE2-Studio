import React, { useCallback, useEffect, useRef, useState } from 'react';
import { FileMusic, Loader2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { TranslationKey } from '../i18n/translations';
import { isMidiFile, MIDI_EXTENSIONS, noteName } from '../services/midiFiles';
import { midiImportOptions } from '../i18n/midiImportOptions';

/** The largest MIDI file the service reads: a long song is tens of kilobytes. */
const LARGEST = 8 * 1024 * 1024;

interface PartRow {
  number: number;
  name: string;
  family: string;
  notes: number;
  low: number;
  high: number;
  drums: boolean;
  polyphony: number;
  role: '' | 'voice' | 'instrument' | 'chords';
  why: '' | 'karaoke' | 'name' | 'highest' | 'busiest' | 'chosen';
}

interface Notice {
  code: string;
  text: string;
  [field: string]: unknown;
}

interface Facts {
  bpm: number;
  meter: string;
  bars: number;
  seconds: number;
  key: string;
  key_source: 'file' | 'estimated';
  karaoke: boolean;
}

type Answer =
  | { ok: true; abc: string; lyrics: string; parts: PartRow[]; facts: Facts; notices: Notice[] }
  | { ok: false; error: string; parts?: PartRow[] };

export interface MidiImported {
  abc: string;
  lyrics: string | null;
  summary: string;
  mode: 'melody' | 'full';
}

const SELECT = 'h-8 rounded-lg border border-zinc-200 bg-white px-2 text-xs text-zinc-700 outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-zinc-800 dark:text-zinc-200';
const SEGMENT = 'rounded-md px-2.5 py-1 text-[11px] font-semibold transition-all';
const SEGMENT_ON = 'bg-white text-black shadow-xs dark:bg-zinc-700 dark:text-white';
const SEGMENT_OFF = 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200';

/**
 * A MIDI file read into a score: the file's tracks, which of them the voice
 * and the instrument take, chords or none, and what the service had to do to
 * write it. Every change of a choice reads the file again; nothing reaches the
 * editor until "Open in the editor".
 */
export const MidiImportDialog: React.FC<{
  chordsWanted: boolean;
  /** A file to read at once, as a library track's MIDI sent to a cover is: its name and base64 data. */
  initial?: { name: string; data: string };
  onCancel: () => void;
  onOpen: (imported: MidiImported) => void;
}> = ({ chordsWanted, initial, onCancel, onOpen }) => {
  const { t, language } = useI18n();
  const say = useCallback((key: string, values: Record<string, string | number> = {}) => Object.entries(values).reduce((text, [name, value]) => text.split(`{${name}}`).join(String(value)), t(key as TranslationKey)), [t]);
  const count = useCallback((base: string, value: number) => say(`${base}_${new Intl.PluralRules(language).select(value)}`, { count: value }), [say, language]);
  const picker = useRef<HTMLInputElement | null>(null);
  const [file, setFile] = useState<{ name: string; data: string } | null>(initial ?? null);
  const [mode, setMode] = useState<'melody' | 'full'>(chordsWanted ? 'full' : 'melody');
  const [vocal, setVocal] = useState('auto');
  const [instrument, setInstrument] = useState('auto');
  const [sections, setSections] = useState(false);
  const [grid, setGrid] = useState('auto');
  const [vocalOctaves, setVocalOctaves] = useState('0');
  const [instrumentOctaves, setInstrumentOctaves] = useState('0');
  const optionsText = midiImportOptions[language];
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const asked = useRef(0);

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.stopPropagation();
      onCancel();
    };
    document.addEventListener('keydown', escape, true);
    return () => document.removeEventListener('keydown', escape, true);
  }, [onCancel]);

  useEffect(() => {
    if (!file) return;
    const turn = ++asked.current;
    setBusy(true);
    setProblem(null);
    fetch('/v1/score/from-midi', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ data: file.data, mode, vocal, instrument, sections, grid: grid === 'auto' ? undefined : Number(grid), vocal_octaves: Number(vocalOctaves), instrument_octaves: Number(instrumentOctaves) }),
    })
      .then(async response => {
        const payload = (await response.json().catch(() => null)) as Answer | null;
        if (turn !== asked.current) return;
        setAnswer(payload && typeof payload === 'object' && 'ok' in payload ? payload : { ok: false, error: `HTTP ${response.status}` });
      })
      .catch(error => {
        if (turn === asked.current) setAnswer({ ok: false, error: error instanceof Error ? error.message : String(error) });
      })
      .finally(() => {
        if (turn === asked.current) setBusy(false);
      });
  }, [file, mode, vocal, instrument, sections, grid, vocalOctaves, instrumentOctaves]);

  const choose = async (picked: File) => {
    if (!isMidiFile(picked.name)) {
      setProblem(say('midiInHint'));
      return;
    }
    if (picked.size > LARGEST) {
      setProblem(`${picked.name}: > 8 MB`);
      return;
    }
    const bytes = new Uint8Array(await picked.arrayBuffer());
    let raw = '';
    for (let index = 0; index < bytes.length; index += 0x8000) raw += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
    setVocal('auto');
    setInstrument('auto');
    setGrid('auto'); setVocalOctaves('0'); setInstrumentOctaves('0');
    setAnswer(null);
    setFile({ name: picked.name, data: btoa(raw) });
  };

  const notice = (item: Notice): string => {
    const field = (name: string) => item[name] as number | string;
    switch (item.code) {
      case 'tempo_changes':
        return say('midiInNotice_tempo_changes', { low: Math.round(Number(field('low'))), high: Math.round(Number(field('high'))), bpm: Math.round(Number(field('bpm'))) });
      case 'moved': {
        const semitones = Number(field('semitones'));
        return say(semitones > 0 ? 'midiInNotice_moved_up' : 'midiInNotice_moved_down', { line: say(`midiInLine_${field('line')}`), track: field('track'), octaves: count('midiInOctaves', Math.abs(semitones) / 12) });
      }
      case 'struck':
        return say('midiInNotice_struck', { track: field('track'), count: field('count') });
      case 'chords_read':
        return say('midiInNotice_chords_read', { track: field('track'), name: field('name') });
      case 'unnamed':
        return say('midiInNotice_unnamed', { track: field('track'), bars: (item.bars as number[]).join(', ') });
      case 'played_in':
      case 'chords_guessed':
        return say(`midiInNotice_${item.code}`);
      default:
        return item.text;
    }
  };

  const parts = answer?.parts ?? [];
  const melodic = parts.filter(part => !part.drums);
  const facts = answer?.ok ? answer.facts : null;
  const factLine = facts
    ? say('midiInFacts', { bpm: facts.bpm, meter: facts.meter, bars: count('scoreBars', facts.bars), seconds: Math.round(facts.seconds), key: facts.key })
      + (facts.key_source === 'estimated' ? ` (${say('midiInKeyEstimated')})` : '')
      + (facts.karaoke ? ` · ${say('midiInKaraoke')}` : '')
    : '';

  const open = () => {
    if (!answer?.ok) return;
    onOpen({ abc: answer.abc, lyrics: answer.facts.karaoke ? answer.lyrics : null, summary: [factLine, ...answer.notices.map(notice)].join(' '), mode });
  };

  return (
    <div className="fixed inset-0 z-80 flex items-center justify-center bg-black/50 p-4">
      <div role="dialog" aria-modal="true" aria-labelledby="midi-import-title" className="flex max-h-[calc(var(--vh)*88)] w-full max-w-2xl flex-col overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-zinc-900">
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-3.5 dark:border-white/10">
          <h3 id="midi-import-title" className="flex items-center gap-2 text-base font-bold text-zinc-900 dark:text-white">
            <FileMusic size={17} className="text-pink-500" /> {say('midiInTitle')}
          </h3>
          <button type="button" onClick={onCancel} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200" aria-label={say('scoreClose')}>
            <X size={18} />
          </button>
        </div>

        <div className="min-h-0 space-y-4 overflow-y-auto p-5">
          <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{say('midiInHint')}</p>
          <div className="flex flex-wrap items-center gap-3">
            <button
              type="button"
              onClick={() => picker.current?.click()}
              className="inline-flex h-8 items-center gap-1.5 rounded-lg bg-pink-600 px-3 text-xs font-bold text-white transition hover:brightness-110"
            >
              <FileMusic size={13} /> {file ? say('midiInAnother') : say('midiInChoose')}
            </button>
            {file && <span className="min-w-0 truncate text-sm font-semibold text-zinc-800 dark:text-zinc-100">{file.name}</span>}
            {busy && <Loader2 size={14} className="animate-spin text-zinc-400" aria-label={say('midiInReading')} />}
            <input
              ref={picker}
              type="file"
              accept={MIDI_EXTENSIONS.join(',') + ',audio/midi,audio/x-midi'}
              className="hidden"
              onChange={event => {
                const picked = event.target.files?.[0];
                if (picked) void choose(picked);
                event.target.value = '';
              }}
            />
          </div>
          {problem && <p className="text-xs text-red-600 dark:text-red-400">{problem}</p>}

          {file && (
            <div className="grid gap-3 sm:grid-cols-2">
              <div>
                <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{say('midiInChords')}</span>
                <div className="inline-flex gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-black/30" role="radiogroup" aria-label={say('midiInChords')}>
                  {(['melody', 'full'] as const).map(value => (
                    <button
                      key={value}
                      type="button"
                      role="radio"
                      aria-checked={mode === value}
                      title={say(value === 'melody' ? 'midiInMelodyHint' : 'midiInFullHint')}
                      onClick={() => setMode(value)}
                      className={`${SEGMENT} ${mode === value ? SEGMENT_ON : SEGMENT_OFF}`}
                    >
                      {say(value === 'melody' ? 'midiInMelody' : 'midiInFull')}
                    </button>
                  ))}
                </div>
              </div>
              <label className="flex items-start gap-2 pt-5 text-xs text-zinc-600 dark:text-zinc-300" title={say('midiInSectionsHint')}>
                <input type="checkbox" checked={sections} onChange={event => setSections(event.target.checked)} className="mt-0.5 accent-pink-600" />
                <span>{say('midiInSections')}</span>
              </label>
              <label className="flex flex-col gap-1" title={say('midiInVoiceHint')}>
                <span className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{say('midiInVoice')}</span>
                <select value={vocal} onChange={event => setVocal(event.target.value)} className={SELECT}>
                  <option value="auto">{say('midiInAuto')}</option>
                  {melodic.map(part => <option key={part.number} value={String(part.number)}>{part.number}. {part.name || part.family}</option>)}
                </select>
              </label>
              <label className="flex flex-col gap-1" title={say('midiInInstrumentHint')}>
                <span className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{say('midiInInstrument')}</span>
                <select value={instrument} onChange={event => setInstrument(event.target.value)} className={SELECT}>
                  <option value="auto">{say('midiInAuto')}</option>
                  <option value="none">{say('midiInNone')}</option>
                  {melodic.map(part => <option key={part.number} value={String(part.number)}>{part.number}. {part.name || part.family}</option>)}
                </select>
              </label>
            </div>
          )}

          {answer && answer.ok === false && <p className="rounded-lg bg-red-500/10 p-2 text-xs leading-5 text-red-700 dark:text-red-300">{answer.error}</p>}
          {file && <div className="space-y-2">
            <div className="grid gap-3 sm:grid-cols-3">
              <label className="flex flex-col gap-1 text-xs text-zinc-500">{optionsText.grid}
                <select className={SELECT} value={grid} onChange={event => setGrid(event.target.value)}><option value="auto">{optionsText.auto}</option><option value="16">1/16</option><option value="32">1/32</option></select>
              </label>
              {([[optionsText.voice, vocalOctaves, setVocalOctaves], [optionsText.instrument, instrumentOctaves, setInstrumentOctaves]] as const).map(([label, value, change]) => <label key={label} className="flex flex-col gap-1 text-xs text-zinc-500">{label}
                <select className={SELECT} value={value} onChange={event => change(event.target.value)}>{[-3, -2, -1, 0, 1, 2, 3].map(octave => <option key={octave} value={octave}>{octave > 0 ? '+' : ''}{octave}</option>)}</select>
              </label>)}
            </div><p className="text-[11px] text-zinc-500">{optionsText.hint}</p>
          </div>}
          {facts && <p className="text-xs font-semibold text-zinc-700 dark:text-zinc-200">{factLine}</p>}

          {parts.length > 0 && (
            <div className="overflow-x-auto rounded-lg border border-zinc-200 dark:border-white/10">
              <table className="w-full text-left text-xs">
                <caption className="sr-only">{say('midiInTracks')}</caption>
                <thead className="bg-zinc-50 text-[11px] uppercase tracking-wide text-zinc-500 dark:bg-white/5 dark:text-zinc-400">
                  <tr>
                    <th className="px-2.5 py-1.5 font-semibold">{say('midiInColTrack')}</th>
                    <th className="px-2.5 py-1.5 font-semibold">{say('midiInColNotes')}</th>
                    <th className="px-2.5 py-1.5 font-semibold">{say('midiInColRange')}</th>
                    <th className="px-2.5 py-1.5 font-semibold">{say('midiInColTakes')}</th>
                  </tr>
                </thead>
                <tbody>
                  {parts.map(part => (
                    <tr key={part.number} className={`border-t border-zinc-100 dark:border-white/5 ${part.role ? 'bg-pink-500/5' : ''}`}>
                      <td className="px-2.5 py-1.5">
                        <span className="font-semibold text-zinc-800 dark:text-zinc-100">{part.number}. {part.name || part.family}</span>
                        <span className="ml-1.5 text-zinc-500">{part.drums ? say('midiInDrums') : part.family}</span>
                      </td>
                      <td className="px-2.5 py-1.5 tabular-nums text-zinc-600 dark:text-zinc-300">
                        {part.notes}
                        {part.polyphony > 1 && <span className="ml-1.5 text-zinc-400">{say('midiInPolyphony', { count: part.polyphony })}</span>}
                      </td>
                      <td className="px-2.5 py-1.5 tabular-nums text-zinc-600 dark:text-zinc-300">{part.drums ? '—' : `${noteName(part.low)}–${noteName(part.high)}`}</td>
                      <td className="px-2.5 py-1.5">
                        {part.role && <span className="font-semibold text-pink-600 dark:text-pink-300">{say(`midiInRole_${part.role}`)}</span>}
                        {part.why && <span className="ml-1.5 text-zinc-500">{say(`midiInWhy_${part.why}`)}</span>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {answer?.ok && answer.notices.length > 0 && (
            <ul className="space-y-1.5 text-xs leading-5 text-zinc-600 dark:text-zinc-300">
              {answer.notices.map((item, index) => <li key={index} className="rounded-lg bg-amber-500/10 px-2.5 py-1.5">{notice(item)}</li>)}
            </ul>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-zinc-200 px-5 py-3.5 dark:border-white/10">
          <button type="button" onClick={onCancel} className="rounded-lg px-4 py-2 text-sm font-medium text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200">
            {say('scoreCancel')}
          </button>
          <button
            type="button"
            onClick={open}
            disabled={!answer?.ok || busy}
            title={say('midiInUseHint')}
            className="rounded-lg bg-pink-600 px-4 py-2 text-sm font-bold text-white transition hover:brightness-110 disabled:opacity-50"
          >
            {say('midiInUse')}
          </button>
        </div>
      </div>
    </div>
  );
};
