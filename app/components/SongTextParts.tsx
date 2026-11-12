import React, { useEffect, useId, useMemo, useRef, useState } from 'react';
import { ArrowDown, ArrowUp, Copy, Plus, RotateCw, Trash2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { songDetailStrings, songPartHints, songTokenStrings, songWritingStrings } from '../i18n/songWriting';
import { cycleSection, editLyricLine, lyricSections, SECTION_PRESETS, sectionText, setStyleRole, STYLE_LANGUAGES, styleRole, toggleLetterCase, type LyricSection } from '../services/songWriting';
import { styleSuggestions } from '../services/styleSuggestions';
import { MAIN_STYLES } from '../data/genres';
import { lyricLineOffsets, songTokens, tokensOnLine, type SongTokens } from '../services/songTokens';

const control = 'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-2 py-1.5 text-xs outline-hidden focus:border-pink-500 dark:border-white/10 dark:bg-black/25';
const button = 'rounded-md p-1.5 text-zinc-500 hover:bg-pink-500/10 hover:text-pink-500 disabled:opacity-30';
export function SongTextParts({ value, onChange, kind, style = '', cot = 'full' }: { value: string; onChange: (text: string) => void; kind: 'style' | 'lyrics'; style?: string; cot?: string }) {
  const [open, setOpen] = useState(false);
  const { language } = useI18n();
  const s = { ...songWritingStrings[language], ...songDetailStrings[language], ...songTokenStrings[language], ...songPartHints[language] };
  const hint = (text: string) => <span className="mt-0.5 block text-[10px] leading-4 text-zinc-400">{text}</span>;
  const suggestionId = useId();
  const suggestions = useMemo(() => styleSuggestions(language, MAIN_STYLES), [language]);
  const [newSound, setNewSound] = useState('');
  const [tokens, setTokens] = useState<SongTokens | null>(null);
  const [tokenProblem, setTokenProblem] = useState('');
  useEffect(() => {
    setTokens(null);
    setTokenProblem('');
    if (!open || kind !== 'lyrics') return;
    const run = new AbortController();
    const timer = window.setTimeout(() => { void songTokens(style, value, cot, run.signal).then(answer => { if (!run.signal.aborted) { setTokens(answer); setTokenProblem(answer.problem || ''); } }).catch(error => { if (!run.signal.aborted) setTokenProblem(error instanceof Error ? error.message : String(error)); }); }, 300);
    return () => { window.clearTimeout(timer); run.abort(); };
  }, [open, kind, style, value, cot]);
  const lineOffsets = lyricLineOffsets(value);
  const [sections, setSections] = useState(() => lyricSections(value));
  const lastValue = useRef(value);
  useEffect(() => {
    if (value !== lastValue.current) {
      setSections(lyricSections(value));
      lastValue.current = value;
    }
  }, [value]);
  const changeSections = (next: LyricSection[]) => {
    setSections(next);
    lastValue.current = sectionText(next);
    onChange(lastValue.current);
  };
  const parts = value.split(',');
  const parsedSections = lyricSections(value);
  const cutsAligned = parsedSections.length === sections.length && parsedSections.every((section, index) => section.tag === sections[index].tag && section.body === sections[index].body);
  const patchSection = (index: number, patch: Partial<LyricSection>) => changeSections(sections.map((section, at) => at === index ? { ...section, ...patch } : section));
  const move = (index: number, delta: number) => {
    const next = [...sections];
    [next[index], next[index + delta]] = [next[index + delta], next[index]];
    changeSections(next);
  };
  return <div className="mt-2">
    <button type="button" className="text-[11px] font-semibold text-pink-600 dark:text-pink-300" onClick={() => setOpen(!open)} aria-expanded={open}>{kind === 'style' ? s.parts : s.sections}</button>
    {open && <div className="mt-2 space-y-2">
      {kind === 'lyrics' && <p className="text-[11px] text-zinc-500" role="status">{tokens?.available ? `${tokens.tokens} ${s.tokens}` : tokenProblem || s.tokenLoading}</p>}
      {kind === 'style' && <div className="space-y-2 rounded-lg border border-zinc-200 p-2 dark:border-white/10">
        <div className="grid grid-cols-2 gap-2">
          <label className="text-[11px] text-zinc-500">{s.language}<select className={control} value={styleRole(value, 'language')} onChange={event => onChange(setStyleRole(value, 'language', event.target.value))}><option value="">{s.auto}</option>{STYLE_LANGUAGES.map(name => <option key={name}>{name}</option>)}</select>{hint(s.languageHint)}</label>
          <label className="text-[11px] text-zinc-500">{s.tempo}<input key={`tempo-${styleRole(value, 'tempo')}`} type="number" min={40} max={200} className={control} defaultValue={styleRole(value, 'tempo')} placeholder={s.auto} onBlur={event => onChange(setStyleRole(value, 'tempo', event.target.value))} /></label>
        </div>
        <div className="flex items-center gap-2"><label className="flex items-center gap-1 text-[11px] text-zinc-500"><input type="checkbox" className="accent-pink-500" checked={Boolean(styleRole(value, 'tempo'))} onChange={event => onChange(setStyleRole(value, 'tempo', event.target.checked ? '88' : ''))} />BPM</label><input type="range" min={40} max={200} step={1} aria-label={s.tempo} className="w-full accent-pink-500 disabled:opacity-30" disabled={!styleRole(value, 'tempo')} value={styleRole(value, 'tempo') || '88'} onChange={event => onChange(setStyleRole(value, 'tempo', event.target.value))} /></div>
        {hint(s.tempoHint)}
        <label className="block text-[11px] text-zinc-500">{s.voice}<input key={`voice-${styleRole(value, 'voice')}`} className={control} list={suggestionId} defaultValue={styleRole(value, 'voice')} placeholder={s.auto} onBlur={event => onChange(setStyleRole(value, 'voice', event.target.value))} />{hint(s.voiceHint)}</label>
        <div className="flex gap-1"><input className={control} list={suggestionId} aria-label={s.sound} value={newSound} onChange={event => setNewSound(event.target.value)} placeholder={s.sound} /><button type="button" className={button} title={s.add} disabled={!newSound.trim()} onClick={() => { const tempoAt = parts.findIndex(part => /^\s*(?:\d{2,3}\s*bpm|bpm\s*\d{2,3})\s*$/i.test(part)); const next = [...parts]; next.splice(tempoAt === parts.length - 1 ? tempoAt : parts.length, 0, ` ${newSound.trim()}`); onChange(next.filter(part => part.trim()).join(',')); setNewSound(''); }}><Plus size={13} /></button></div>
        {hint(s.soundHint)}
        <datalist id={suggestionId}>{suggestions.map(item => <option key={item.value} value={item.value} label={item.label} />)}</datalist>
      </div>}
      {kind === 'style' ? parts.map((part, index) => <div key={index} className="flex gap-1">
        <input className={control} aria-label={`${s.descriptor} ${index + 1}`} value={part} onChange={event => { const next = [...parts]; next[index] = event.target.value; onChange(next.join(',')); }} />
        <button type="button" className={button} title={s.up} disabled={index === 0} onClick={() => { const next = [...parts]; [next[index], next[index - 1]] = [next[index - 1], next[index]]; onChange(next.join(',')); }}><ArrowUp size={13} /></button>
        <button type="button" className={button} title={s.down} disabled={index === parts.length - 1} onClick={() => { const next = [...parts]; [next[index], next[index + 1]] = [next[index + 1], next[index]]; onChange(next.join(',')); }}><ArrowDown size={13} /></button>
        <button type="button" className={button} title={s.remove} onClick={() => onChange(parts.filter((_, i) => i !== index).join(','))}><Trash2 size={13} /></button>
      </div>) : sections.map((section, index) => <div key={index} className="rounded-lg border border-zinc-200 p-2 dark:border-white/10">
        <div data-section-controls className="mb-1 flex gap-1">
          <input className={control} aria-label={`${s.tag} ${index + 1}`} value={section.tag} onChange={event => { const next = [...sections]; next[index] = { ...section, tag: event.target.value.replace(/[\[\]\r\n]/g, '') }; changeSections(next); }} />
          <button type="button" className={button} title={s.up} disabled={index === 0} onClick={() => move(index, -1)}><ArrowUp size={13} /></button>
          <button type="button" className={button} title={s.down} disabled={index === sections.length - 1} onClick={() => move(index, 1)}><ArrowDown size={13} /></button>
          <button type="button" className={button} title={s.duplicate} onClick={() => { const next = [...sections]; next.splice(index + 1, 0, { ...section }); changeSections(next); }}><Copy size={13} /></button>
          <button type="button" className={button} title={s.add} onClick={() => { const next = [...sections]; next.splice(index + 1, 0, { tag: 'Verse', body: '' }); changeSections(next); }}><Plus size={13} /></button>
          <button type="button" className={button} title={s.remove} onClick={() => changeSections(sections.filter((_, i) => i !== index))}><Trash2 size={13} /></button>
        </div>
        <div className="mb-2 flex gap-1"><select className={control} aria-label={`${s.preset} ${index + 1}`} value={SECTION_PRESETS.includes(section.tag) ? section.tag : ''} onChange={event => { if (event.target.value) patchSection(index, { tag: event.target.value }); }}><option value="">{section.tag || s.preset}</option>{SECTION_PRESETS.map(tag => <option key={tag}>{tag}</option>)}</select><button type="button" className={button} title={s.cycle} onClick={() => patchSection(index, { tag: cycleSection(section.tag) })}><RotateCw size={13} /></button></div>
        <textarea className={`${control} min-h-20 font-mono`} aria-label={`${s.words} ${index + 1}`} value={section.body} onChange={event => { const next = [...sections]; next[index] = { ...section, body: event.target.value }; changeSections(next); }} />
        <details className="mt-2"><summary className="cursor-pointer text-[11px] text-pink-600 dark:text-pink-300">{s.lineEditor}</summary><p className="my-2 text-[11px] text-zinc-500">{s.caseHint}</p>
          {section.body.split('\n').map((line, lineIndex, lines) => {
            const facts = tokensOnLine(cutsAligned ? tokens : null, lineOffsets[index]?.[lineIndex], line);
            return <div key={lineIndex} className="mb-2 rounded-md border border-zinc-200 p-1.5 dark:border-white/10">
            <div className="flex gap-1"><input className={control} aria-label={`${s.words} ${index + 1}.${lineIndex + 1}`} value={line} onChange={event => { const next = [...lines]; next[lineIndex] = event.target.value; patchSection(index, { body: next.join('\n') }); }} />
              <button type="button" className={button} title={s.up} disabled={index === 0 && lineIndex === 0} onClick={() => changeSections(editLyricLine(sections, index, lineIndex, 'up'))}><ArrowUp size={12} /></button>
              <button type="button" className={button} title={s.down} disabled={index === sections.length - 1 && lineIndex === lines.length - 1} onClick={() => changeSections(editLyricLine(sections, index, lineIndex, 'down'))}><ArrowDown size={12} /></button>
              <button type="button" className={button} title={s.duplicate} onClick={() => changeSections(editLyricLine(sections, index, lineIndex, 'duplicate'))}><Copy size={12} /></button>
              <button type="button" className={button} title={s.addLine} onClick={() => { const next = [...lines]; next.splice(lineIndex + 1, 0, ''); patchSection(index, { body: next.join('\n') }); }}><Plus size={12} /></button>
              <button type="button" className={button} title={s.remove} onClick={() => changeSections(editLyricLine(sections, index, lineIndex, 'delete'))}><Trash2 size={12} /></button>
            </div>
            <div className="mt-1 whitespace-pre-wrap break-words font-mono text-xs">{Array.from(line).map((letter, at) => {
              const marks = `${facts?.cuts.has(at) ? 'border-l border-pink-500 pl-0.5' : ''} ${facts?.torn.has(at) ? 'underline decoration-amber-500 decoration-dotted' : ''}`;
              return letter.toUpperCase() !== letter.toLowerCase() ? <button key={at} type="button" className={`${marks} hover:bg-pink-500/20`} title={s.flipCase} onClick={() => patchSection(index, { body: lines.map((text, row) => row === lineIndex ? toggleLetterCase(text, at) : text).join('\n') })}>{letter}</button> : <span key={at} className={marks}>{letter}</span>;
            })}{facts && <span className="ml-2 font-sans text-[10px] text-zinc-500">{facts.count} {s.tokens}</span>}</div>
            {line && <div className="mt-1 flex gap-2 text-[10px] text-zinc-500"><button type="button" onClick={() => changeSections(editLyricLine(sections, index, lineIndex, 'duplicateStanza'))}>{s.duplicateStanza}</button><button type="button" onClick={() => changeSections(editLyricLine(sections, index, lineIndex, 'deleteStanza'))}>{s.deleteStanza}</button></div>}
          </div>; })}
          <button type="button" className="text-[11px] text-pink-600 dark:text-pink-300" onClick={() => patchSection(index, { body: `${section.body}\n` })}>{s.addLine}</button>
        </details>
      </div>)}
      <button type="button" className="inline-flex items-center gap-1 text-xs text-pink-600 dark:text-pink-300" onClick={() => kind === 'style' ? onChange([...parts, ''].join(',')) : changeSections([...sections, { tag: 'Verse', body: '' }])}><Plus size={13} />{s.add}</button>
    </div>}
  </div>;
}
