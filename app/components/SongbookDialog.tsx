import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Printer, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { chordShape, type ChordInstrument } from '../services/chordShapes';
import { buildSongbook, chordName, layoutByPhrases, placeLyricSyllables } from '../services/songbook';
import { estimateLyricSyllables } from '../services/songbookSyllables';
import type { Language } from '../i18n/translations';
import { songbookStrings } from '../i18n/songbook';
import { Stepper } from './kit/Stepper';
import { CatalogSegments } from './kit/Catalog';
import { SecondaryButton } from './kit/Buttons';
import './songbook.css';


function NumberedNote({ number, duration }: { number: string; duration?: number }) {
  if (duration === undefined) return <span>{number}</span>;
  const quarters = duration * 4;
  const levels = [1, .5, .25, .125];
  const level = levels.findIndex(length => Math.abs(quarters - length) < 1e-7 || Math.abs(quarters - 1.5 * length) < 1e-7);
  const dotted = level >= 0 && Math.abs(quarters - 1.5 * levels[level]) < 1e-7;
  const held = quarters >= 2 && Number.isInteger(quarters) ? '—'.repeat(quarters - 1) : '';
  return <span>
    <span className={level > 0 ? `songbook-short-note songbook-short-note-${level}` : undefined}>{number}</span>
    {dotted ? '·' : held}
  </span>;
}

function Shape({ frets, chord }: { frets: number[]; chord: string }) {
  const held = frets.filter(fret => fret > 0);
  const base = held.length ? Math.min(...held) : 1;
  const name = frets.map(fret => fret < 0 ? '×' : fret).join(' ');
  const width = 28 + 10 * (frets.length - 1);
  return (
    <figure className="songbook-shape">
      <figcaption className="text-center font-semibold">{chord}</figcaption>
      <svg viewBox={`0 0 ${width} 62`} width={width} height={62} role="img" aria-label={`${chord}: ${name}`}>
        {frets.map((fret, string) => {
          const x = 14 + string * 10;
          return <g key={string}>
            <line x1={x} x2={x} y1={16} y2={56} stroke="currentColor" />
            {fret > 0 && <circle cx={x} cy={21 + 10 * (fret - base)} r="3" fill="currentColor" />}
            {fret <= 0 && <text x={x} y={12} textAnchor="middle" fontSize="10">{fret ? '×' : '○'}</text>}
          </g>;
        })}
        {[0, 1, 2, 3, 4].map(row => <line key={row} x1={14} x2={width - 14}
          y1={16 + row * 10} y2={16 + row * 10} stroke="currentColor" strokeWidth={base === 1 && row === 0 ? 2 : 1} />)}
        {base > 1 && <text x="3" y="24" fontSize="9">{base}</text>}
      </svg>
    </figure>
  );
}

export function SongbookDialog({ abc, title, lyrics, onClose }: {
  abc: string;
  title?: string;
  lyrics?: string;
  onClose: () => void;
}) {
  const { language } = useI18n();
  const text = songbookStrings[language];
  const dialog = useRef<HTMLDialogElement>(null);
  const [transpose, setTranspose] = useState(0);
  const [capo, setCapo] = useState(0);
  const [instrument, setInstrument] = useState<ChordInstrument>('guitar');
  const [numbered, setNumbered] = useState(language === 'zh');
  const [lyricLanguage, setLyricLanguage] = useState(language);
  useEffect(() => {
    const frame = dialog.current;
    frame?.showModal();
    return () => frame?.close();
  }, []);
  const sheet = useMemo(() => {
    try {
      const result = buildSongbook(abc, transpose);
      let lines = result.lines;
      let approximate = false;
      if (lyrics?.trim()) {
        approximate = placeLyricSyllables(lines, estimateLyricSyllables(lyrics, lyricLanguage));
        const worded = lines.some(line => line.some(cell => cell.lyrics.some(Boolean)));
        const phrased = approximate || worded ? null : layoutByPhrases(lines, lyrics, line => estimateLyricSyllables(line, lyricLanguage));
        if (phrased) {
          lines = phrased;
          approximate = true;
        }
      }
      return { ...result, lines, approximate };
    } catch {
      return null;
    }
  }, [abc, transpose, lyrics, lyricLanguage]);
  const names = useMemo(() => [...new Set(sheet?.lines.flatMap(line => line.flatMap(cell => cell.chords))
    .map(chord => chordName(chord, -capo)) ?? [])], [sheet, capo]);
  const diagrams = useMemo(() => names.flatMap(name => {
    const frets = chordShape(name, instrument);
    return frets ? [{ name, frets }] : [];
  }), [names, instrument]);
  const missingDiagrams = names.filter(name => !diagrams.some(diagram => diagram.name === name));
  const containsWords = sheet?.lines.some(line => line.some(cell => cell.lyrics.some(Boolean)));
  const pageTitle = title || sheet?.title || text.title;

  return createPortal(
    <dialog ref={dialog} className="studio-songbook m-auto w-[min(92vw,56rem)] rounded-xl bg-white p-0 text-zinc-900 shadow-xl backdrop:bg-black/60 dark:bg-zinc-900 dark:text-white"
      aria-label={text.title} onCancel={event => { event.preventDefault(); onClose(); }}>
      <div className="songbook-controls sticky top-0 space-y-3 border-b border-zinc-200 bg-white p-4 dark:border-white/20 dark:bg-zinc-900">
        <div className="flex items-center gap-2">
          <h2 className="min-w-0 flex-1 truncate text-base font-bold">{text.title}</h2>
          <SecondaryButton disabled={!sheet?.lines.length} onClick={() => window.print()}><Printer size={15} aria-hidden />{text.print}</SecondaryButton>
          <button type="button" aria-label={text.close} title={text.close} onClick={onClose}
            className="inline-flex size-9 items-center justify-center rounded-lg text-zinc-500 hover:bg-zinc-100 hover:text-zinc-900 dark:text-zinc-400 dark:hover:bg-white/10 dark:hover:text-white"><X size={17} aria-hidden /></button>
        </div>
        <div className="flex flex-wrap items-end gap-x-5 gap-y-3 text-xs">
          <div className="grid gap-1"><span className="font-semibold text-zinc-600 dark:text-zinc-300">{text.transpose}</span>
            <Stepper value={transpose} min={-12} max={12} label={text.transpose} format={value => value > 0 ? `+${value}` : String(value)} onChange={setTranspose} /></div>
          <div className="grid gap-1"><span className="font-semibold text-zinc-600 dark:text-zinc-300">{text.capo}</span>
            <Stepper value={capo} min={0} max={12} label={text.capo} onChange={setCapo} /></div>
          <div className="grid gap-1"><span className="font-semibold text-zinc-600 dark:text-zinc-300">{text.instrument}</span>
            <CatalogSegments<ChordInstrument> label={text.instrument} value={instrument} onChange={setInstrument}
              options={[{ value: 'guitar', label: text.guitar }, { value: 'ukulele', label: text.ukulele }]} /></div>
          {lyrics?.trim() && <div className="grid gap-1"><span className="font-semibold text-zinc-600 dark:text-zinc-300">{text.lyricLanguage}</span>
            <CatalogSegments<Language> label={text.lyricLanguage} value={lyricLanguage} onChange={setLyricLanguage}
              options={[{ value: 'en', label: 'EN' }, { value: 'pt', label: 'PT' }, { value: 'ru', label: 'RU' }, { value: 'zh', label: 'ZH' }, { value: 'ja', label: 'JA' }, { value: 'ko', label: 'KO' }]} /></div>}
          <button type="button" role="switch" aria-checked={numbered} onClick={() => setNumbered(value => !value)} className="flex min-h-9 items-center gap-2 text-sm">
            <span className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${numbered ? 'bg-pink-500' : 'bg-zinc-300 dark:bg-zinc-600'}`}>
              <span className={`absolute top-[2px] h-4 w-4 rounded-full bg-white shadow-xs transition-all ${numbered ? 'left-[18px]' : 'left-[2px]'}`} />
            </span>
            {text.numbered}
          </button>
        </div>
      </div>
      <article className="songbook-paper max-h-[70vh] overflow-auto p-6">
        <h2 className="mb-1 text-xl font-bold">{pageTitle}</h2>
        {sheet?.key && <p className="mb-4 text-sm">{text.key} {sheet.key}{capo > 0 && ` · ${text.capo} ${capo}`}</p>}
        {!sheet?.lines.length && <p>{text.unavailable}</p>}
        {sheet?.approximate && <p className="mb-3 text-sm">{text.approximate}</p>}
        {sheet?.lines.map((line, index) => {
          const keyChanges = numbered && line.some(cell => cell.keyChange);
          return (
            <div key={index} className="songbook-line">
              {line.filter(cell => numbered || cell.chords.length || cell.lyrics.some(Boolean)).map((cell, column) => (
                <span key={column} className="songbook-cell">
                  {keyChanges && (
                    <span className="whitespace-nowrap text-xs font-semibold">
                      {cell.keyChange && `${text.key} ${cell.keyChange}`}
                    </span>
                  )}
                  <strong className="text-indigo-700 dark:text-indigo-300">{cell.chords.map(chord => chordName(chord, -capo)).join(' ')}</strong>
                  {numbered && <NumberedNote number={cell.number} duration={cell.duration} />}
                  {cell.lyrics.map((word, verse) => <span key={verse}>{word}</span>)}
                </span>
              ))}
            </div>
          );
        })}
        {!containsWords && lyrics?.trim() && <section className="mt-4">
          <h3 className="font-semibold">{text.unaligned}</h3>
          <p className="whitespace-pre-wrap">{lyrics}</p>
        </section>}
        {!!diagrams.length && <section className="mt-4">
          <h3 className="font-semibold">{text[instrument]}</h3>
          <div className="mt-2 flex flex-wrap gap-4">
            {diagrams.map(({ name, frets }) => <Shape key={name} chord={name} frets={frets} />)}
          </div>
        </section>}
        {!!missingDiagrams.length && <p className="mt-4 text-sm">{text.noShape} {missingDiagrams.join(', ')}</p>}
      </article>
    </dialog>,
    document.body,
  );
}
