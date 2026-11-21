import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  Disc3, Play, Pause, SkipBack, Repeat, RotateCcw, Headphones, Search, X,
  AudioLines, FolderOpen, Circle, Waves, ListMusic,
} from 'lucide-react';
import type { Playlist, Song } from '../../types';
import { trackCoverUrl } from '../../services/playerPanels';
import { stemOf } from '../../services/songStems';
import { saveFile } from '../../services/saveFile';
import { DjMixer, type DjDeck, type DjDeckSnapshot } from '../../services/djMixer';

type DeckIndex = 0 | 1;
type BrowserSource = 'all' | 'unassigned' | 'liked' | 'stems' | `collection:${string}`;

const TIME = (seconds: number): string => {
  if (!Number.isFinite(seconds)) return '0:00';
  const whole = Math.floor(Math.max(0, seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
};
const pct = (n: number): string => `${Math.round(n * 100)}%`;
const wrapAngle = (angle: number): number => angle > Math.PI ? angle - 2 * Math.PI : angle < -Math.PI ? angle + 2 * Math.PI : angle;
const DJ_MIME = 'application/x-yue2-dj-song';

const button = 'rounded-md border border-white/10 bg-white/5 px-3 py-1.5 text-xs font-semibold text-zinc-200 transition hover:border-white/25 hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-35';
const slider = 'h-1.5 w-full cursor-pointer accent-fuchsia-500';

const MixerMeter: React.FC<{ deck: DjDeck; color: string }> = ({ deck, color }) => {
  const canvas = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const ctx = element.getContext('2d');
    if (!ctx) return;
    const data = new Uint8Array(deck.meter.frequencyBinCount);
    let frame = 0;
    const draw = () => {
      const w = element.width, h = element.height;
      ctx.clearRect(0, 0, w, h);
      deck.meter.getByteFrequencyData(data);
      const bars = 64;
      const step = w / bars;
      for (let i = 0; i < bars; i++) {
        const amplitude = data[Math.min(data.length - 1, Math.floor(i * data.length / bars))] / 255;
        const height = Math.max(2, amplitude * h);
        ctx.fillStyle = color;
        ctx.globalAlpha = 0.4 + amplitude * 0.6;
        ctx.fillRect(i * step, (h - height) / 2, Math.max(1, step - 2), height);
      }
      ctx.globalAlpha = 1;
      frame = window.requestAnimationFrame(draw);
    };
    draw();
    return () => window.cancelAnimationFrame(frame);
  }, [deck, color]);
  return <canvas ref={canvas} width={500} height={58} className="h-14 w-full rounded-lg bg-black/70" aria-label="Live deck audio spectrum" />;
};

const JogWheel: React.FC<{ deck: DjDeck; side: 'A' | 'B'; color: string }> = ({ deck, side, color }) => {
  const previous = useRef<number | null>(null);
  const at = (event: React.PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return Math.atan2(event.clientY - bounds.top - bounds.height / 2, event.clientX - bounds.left - bounds.width / 2);
  };
  return (
    <div className="flex flex-col items-center gap-2">
      <div
        role="slider" tabIndex={0} aria-label={`Scratch deck ${side}`} aria-valuemin={0} aria-valuemax={Math.max(1, deck.state.duration)}
        aria-valuenow={Math.min(deck.state.duration || 1, deck.state.time)}
        onKeyDown={event => {
          if (event.key === 'ArrowLeft') { event.preventDefault(); deck.seek(deck.state.time - 0.1); }
          if (event.key === 'ArrowRight') { event.preventDefault(); deck.seek(deck.state.time + 0.1); }
        }}
        onPointerDown={event => {
          if (!deck.state.song) return;
          event.preventDefault();
          event.currentTarget.setPointerCapture(event.pointerId);
          previous.current = at(event);
          deck.startScratch();
        }}
        onPointerMove={event => {
          if (previous.current === null) return;
          const angle = at(event);
          const delta = wrapAngle(angle - previous.current);
          previous.current = angle;
          deck.scratch(delta / (Math.PI * 2) * 1.8);
        }}
        onPointerUp={event => {
          if (previous.current !== null) {
            previous.current = null;
            if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
            deck.endScratch();
          }
        }}
        onPointerCancel={() => { previous.current = null; deck.endScratch(); }}
        onLostPointerCapture={() => { previous.current = null; deck.endScratch(); }}
        className="relative flex h-40 w-40 cursor-grab touch-none select-none items-center justify-center rounded-full border-[9px] border-zinc-700 bg-[repeating-radial-gradient(circle,#26272b_0px,#26272b_3px,#141518_5px,#141518_7px)] shadow-[0_0_0_4px_#08080b,0_10px_35px_#000] active:cursor-grabbing sm:h-48 sm:w-48"
      >
        <div className="absolute inset-3 animate-spin rounded-full border-2 border-dotted border-white/20"
          style={{ animationPlayState: deck.state.playing && !deck.state.scratching ? 'running' : 'paused', animationDuration: `${Math.max(0.5, 2.2 / deck.state.rate)}s` }} />
        <div className="relative flex h-[60%] w-[60%] items-center justify-center rounded-full border-4 border-zinc-900 bg-zinc-950 shadow-inner">
          {deck.state.song
            ? <img className="h-full w-full rounded-full object-cover opacity-90 pointer-events-none" alt="" src={trackCoverUrl(deck.state.song)} />
            : <Disc3 className="text-zinc-500" size={36} />}
          <span className="absolute h-3 w-3 rounded-full border-2 border-zinc-700 bg-white" />
        </div>
        <div className="absolute -bottom-1 rounded-full px-3 py-0.5 text-[10px] font-bold text-white" style={{ background: color }}>DECK {side}</div>
      </div>
      <span className="text-[10px] text-zinc-400">{deck.state.scratching ? (deck.state.scratchReady ? 'Scratching' : 'Loading scratch audio…') : 'Drag platter to scratch · ← / → nudge'}</span>
    </div>
  );
};

const EqSlider: React.FC<{ label: string; value: number; onChange: (value: number) => void }> = ({ label, value, onChange }) => (
  <label className="flex min-w-0 flex-col gap-1 text-[10px] font-semibold uppercase text-zinc-400">
    <span className="flex justify-between gap-2"><span>{label}</span><span className="tabular-nums text-zinc-200">{value > 0 ? '+' : ''}{value} dB</span></span>
    <input className={slider} type="range" min="-18" max="12" step="1" value={value} onChange={event => onChange(Number(event.target.value))}
      onDoubleClick={() => onChange(0)} aria-label={`${label} EQ`} />
  </label>
);

const DjDeckPanel: React.FC<{
  mixer: DjMixer; index: DeckIndex; library: Song[]; onSelect: (song: Song) => void;
  onLoad: (index: DeckIndex, song: Song) => void; onTap: (index: DeckIndex) => void;
}> = ({ mixer, index, library, onSelect, onLoad, onTap }) => {
  const deck = mixer.decks[index];
  const state: DjDeckSnapshot = deck.state;
  const side = index === 0 ? 'A' : 'B';
  const color = index === 0 ? '#2a9cff' : '#ff416d';
  const [dropHover, setDropHover] = useState(false);
  const drag = (event: React.DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDropHover(false);
    const id = event.dataTransfer.getData(DJ_MIME) || event.dataTransfer.getData('text/plain');
    const song = library.find(candidate => candidate.id === id);
    if (song) onLoad(index, song);
  };
  const jumpToPercent = (event: React.MouseEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    if (state.duration) deck.seek((event.clientX - box.left) / box.width * state.duration);
  };
  return (
    <section
      onDragOver={event => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; setDropHover(true); }}
      onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setDropHover(false); }}
      onDrop={drag}
      className={`min-w-[345px] flex-1 rounded-xl border p-3 transition-all ${dropHover ? 'border-fuchsia-500 bg-fuchsia-500/10' : 'border-white/10 bg-zinc-900/80'}`}
      aria-label={`DJ deck ${side}`}
    >
      <div className="mb-3 flex items-center gap-3">
        <span className="rounded px-2 py-1 text-sm font-black text-white" style={{ background: color }}>{side}</span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-bold text-white">{state.song?.title ?? `Drag a song onto deck ${side}`}</div>
          <div className="truncate text-[11px] text-zinc-400">{state.song ? state.song.style : 'Drop from your library below, or use LOAD A / B'}</div>
        </div>
        <span className="font-mono text-xs tabular-nums text-zinc-300">{TIME(state.time)} / {TIME(state.duration)}</span>
      </div>
      <MixerMeter deck={deck} color={color} />
      <div onClick={jumpToPercent} role="slider" tabIndex={0} aria-label={`Seek deck ${side}`}
        aria-valuemin={0} aria-valuemax={Math.max(1, state.duration)} aria-valuenow={Math.min(state.time, state.duration || 1)}
        onKeyDown={event => {
          if (event.key === 'ArrowRight') deck.seek(state.time + 5);
          if (event.key === 'ArrowLeft') deck.seek(state.time - 5);
        }}
        className="mt-2 h-2 w-full cursor-pointer overflow-hidden rounded-full bg-zinc-700">
        <div className="h-full rounded-full" style={{ width: pct(state.duration ? state.time / state.duration : 0), background: color }} />
      </div>
      <div className="mt-3 flex flex-wrap justify-center gap-2">
        <button type="button" disabled={!state.song} onClick={() => deck.toggle()} className={`${button} flex items-center gap-1.5 bg-white/10 px-4`}>
          {state.playing ? <Pause size={16} /> : <Play size={16} />} {state.playing ? 'PAUSE' : 'PLAY'}
        </button>
        <button type="button" disabled={!state.song} onClick={() => deck.cue()} className={button}><SkipBack size={14} className="inline" /> CUE</button>
        <button type="button" disabled={!state.song} onClick={() => deck.setCue()} className={button}>SET CUE</button>
        <button type="button" disabled={!state.song} onClick={() => { if (!mixer.sync(index)) onTap(index); }} className={button} title="Match tempo to the opposite deck; if BPM is unknown, tap this track's beat first">SYNC</button>
        <button type="button" disabled={!state.song} onClick={() => deck.seek(0)} className={button} title="Back to start"><RotateCcw size={14} /></button>
      </div>
      <div className="mt-3 grid grid-cols-[minmax(140px,1fr)_minmax(130px,1fr)] gap-4">
        <JogWheel deck={deck} side={side} color={color} />
        <div className="flex min-w-0 flex-col justify-center gap-2">
          <div className="flex items-center justify-between text-xs">
            <span className="text-zinc-400">BPM</span>
            <span className="font-mono font-bold text-white">{state.bpm ? (state.bpm * state.rate).toFixed(1) : '—'}</span>
            <button className={button} type="button" onClick={() => onTap(index)}>TAP</button>
          </div>
          <label className="text-[10px] font-bold uppercase text-zinc-400">Tempo + pitch · {pct(state.rate)}</label>
          <input className={slider} type="range" min="0.25" max="2" step="0.01" value={state.rate} aria-label={`Tempo and pitch deck ${side}`}
            onChange={event => deck.rate(Number(event.target.value))} onDoubleClick={() => deck.rate(1)} />
          <div className="flex items-center justify-between">
            <button className={button} type="button" onClick={() => deck.rate(Math.max(0.25, state.rate - 0.01))}>−</button>
            <button className={button} type="button" onClick={() => deck.rate(1)}>1.00×</button>
            <button className={button} type="button" onClick={() => deck.rate(Math.min(2, state.rate + 0.01))}>+</button>
          </div>
          <span className="text-[10px] text-zinc-400">{(12 * Math.log2(state.rate)).toFixed(1)} semitones · pitch follows tempo</span>
          <label className="mt-1 text-[10px] font-bold uppercase text-zinc-400">Deck gain · {pct(state.level / 1.5)}</label>
          <input className={slider} type="range" min="0" max="1.5" step="0.01" value={state.level}
            onChange={event => mixer.gain(index, Number(event.target.value))} aria-label={`Deck ${side} gain`} />
        </div>
      </div>
      <div className="mt-4 grid grid-cols-3 gap-3 rounded-lg bg-black/30 p-3">
        <EqSlider label="Low" value={state.eq[0]} onChange={value => deck.equalizer(0, value)} />
        <EqSlider label="Mid" value={state.eq[1]} onChange={value => deck.equalizer(1, value)} />
        <EqSlider label="High" value={state.eq[2]} onChange={value => deck.equalizer(2, value)} />
        <label className="col-span-2 text-[10px] text-zinc-400">Filter · {Math.round(state.filter * 100)}
          <input className={slider} type="range" min="-1" max="1" step="0.01" value={state.filter}
            onChange={event => deck.filter(Number(event.target.value))} onDoubleClick={() => deck.filter(0)} aria-label={`Deck ${side} filter`} />
        </label>
        <label className="text-[10px] text-zinc-400">Echo · {pct(state.echo)}
          <input className={slider} type="range" min="0" max="1" step="0.01" value={state.echo}
            onChange={event => deck.echo(Number(event.target.value))} onDoubleClick={() => deck.echo(0)} aria-label={`Deck ${side} echo`} />
        </label>
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        <span className="mr-1 text-[10px] font-semibold text-zinc-500">HOT CUES</span>
        {state.hotCues.map((time, cue) => (
          <button key={cue} type="button" disabled={!state.song}
            onClick={event => event.shiftKey || time === null ? deck.setHotCue(cue) : deck.hotCue(cue)}
            onContextMenu={event => { event.preventDefault(); deck.setHotCue(cue); }}
            title="Click to jump · Shift-click or right-click to set"
            className={`${button} ${time !== null ? 'border-fuchsia-500/40 bg-fuchsia-500/20' : ''}`}>
            {cue + 1}
          </button>
        ))}
        <span className="ml-auto text-[10px] text-zinc-500">Shift+click to set</span>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <span className="text-[10px] font-semibold text-zinc-500">LOOP</span>
        <button className={button} type="button" disabled={!state.song} onClick={() => deck.setLoopIn()}>IN {TIME(state.loopIn)}</button>
        <button className={button} type="button" disabled={!state.song} onClick={() => deck.setLoopOut()}>OUT {TIME(state.loopOut)}</button>
        <button className={`${button} ${state.looping ? 'border-fuchsia-500 bg-fuchsia-500/20' : ''}`} type="button"
          disabled={!state.song} onClick={() => deck.toggleLoop()}><Repeat size={13} className="inline" /> {state.looping ? 'ON' : 'OFF'}</button>
      </div>
      {state.stems.length > 0 && (
        <div className="mt-3 rounded-lg border border-white/10 bg-black/25 p-2">
          <label className="flex cursor-pointer items-center gap-2 text-xs font-semibold text-zinc-200">
            <input type="checkbox" className="accent-fuchsia-500" checked={state.stemMode} onChange={event => deck.setStemMode(event.target.checked)} />
            <AudioLines size={13} /> STEM DECK ({state.stems.length}) — individual channels
          </label>
          {state.stemMode && <div className="mt-2 space-y-1.5">
            {state.stems.map(stem => (
              <div key={stem.id} className="flex items-center gap-2 text-[11px]">
                <span className="w-20 truncate uppercase text-zinc-400">{stem.label}</span>
                <button type="button" className={`${button} px-2 py-0.5 ${stem.mute ? 'bg-rose-500/30 text-rose-200' : ''}`} onClick={() => deck.updateStem(stem.id, { mute: !stem.mute })}>M</button>
                <button type="button" className={`${button} px-2 py-0.5 ${stem.solo ? 'bg-fuchsia-500/30 text-fuchsia-200' : ''}`} onClick={() => deck.updateStem(stem.id, { solo: !stem.solo })}>S</button>
                <input className={slider} type="range" min="0" max="1.5" step="0.01" value={stem.level} aria-label={`Deck ${side} ${stem.label} stem volume`}
                  onChange={event => deck.updateStem(stem.id, { level: Number(event.target.value) })} />
                <span className="w-9 text-right tabular-nums text-zinc-400">{Math.round(stem.level * 100)}%</span>
              </div>
            ))}
          </div>}
        </div>
      )}
      {state.error && <p role="alert" className="mt-2 rounded-md bg-rose-500/10 p-2 text-xs text-rose-300">{state.error}</p>}
      {state.song && <button className="mt-2 text-xs text-zinc-400 underline hover:text-white" type="button" onClick={() => onSelect(state.song!)}>Show track details</button>}
    </section>
  );
};

/**
 * The library browser intentionally uses YuE2's own playlists, albums and stem
 * records. No separate file database, third-party DJ installer or song cap.
 */
export const DjMode: React.FC<{
  library: Song[];
  playlists: Playlist[];
  initialSong: Song | null;
  onClose: () => void;
}> = ({ library, playlists, initialSong, onClose }) => {
  const [revision, setRevision] = useState(0);
  const [mixer, setMixer] = useState<DjMixer | null>(null);
  const [source, setSource] = useState<BrowserSource>('all');
  const [search, setSearch] = useState('');
  const [selected, setSelected] = useState<Song | null>(initialSong);
  const [expandedStems, setExpandedStems] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [taps, setTaps] = useState<Record<number, number[]>>({});
  const tick = useRef<() => void>(() => {});
  const leaveFromKeyboard = useRef<() => void>(() => {});
  tick.current = () => setRevision(value => value + 1);

  useEffect(() => {
    const created = new DjMixer(() => tick.current());
    setMixer(created);
    if (initialSong?.audioUrl) created.decks[0].load(initialSong, library);
    return () => created.dispose();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void leaveFromKeyboard.current();
      }
      if (!(event.altKey && (event.key === '1' || event.key === '2'))) return;
      event.preventDefault();
      mixer?.decks[event.key === '1' ? 0 : 1].toggle();
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, [mixer, onClose]);

  const libraryPlayable = useMemo(() => library.filter(song => song.audioUrl && !song.isGenerating && song.stage !== 'failed' && song.stage !== 'cancelled'), [library]);
  const idsInCollections = useMemo(() => new Set(playlists.flatMap(list => list.songIds || [])), [playlists]);
  const visibleSongs = useMemo(() => {
    const collection = source.startsWith('collection:') ? playlists.find(list => list.id === source.slice('collection:'.length)) : null;
    const ordered = collection ? new Map((collection.songIds || []).map((id, index) => [id, index])) : null;
    const text = search.trim().toLowerCase();
    return libraryPlayable.filter(song => {
      if (source === 'all' && stemOf(song) !== null) return false;
      if (source === 'unassigned' && (idsInCollections.has(song.id) || stemOf(song) !== null)) return false;
      if (source === 'liked' && !song.liked) return false;
      if (source === 'stems' && stemOf(song) === null) return false;
      if (collection && !collection.songIds?.includes(song.id)) return false;
      return !text || `${song.title} ${song.style}`.toLowerCase().includes(text);
    }).sort((a, b) => ordered
      ? (ordered.get(a.id) ?? 0) - (ordered.get(b.id) ?? 0)
      : b.createdAt.getTime() - a.createdAt.getTime());
  }, [libraryPlayable, source, idsInCollections, playlists, search]);

  const stemsOf = useMemo(() => {
    const map = new Map<string, Song[]>();
    for (const song of libraryPlayable) {
      if (!song.derived?.from || stemOf(song) === null) continue;
      map.set(song.derived.from, [...(map.get(song.derived.from) ?? []), song]);
    }
    return map;
  }, [libraryPlayable]);

  const load = (index: DeckIndex, song: Song) => {
    mixer?.decks[index].load(song, libraryPlayable);
    setSelected(song);
  };
  const tap = (index: DeckIndex) => {
    if (!mixer) return;
    const now = performance.now();
    const times = (taps[index] ?? []).filter(time => now - time < 3000).concat(now).slice(-8);
    setTaps(old => ({ ...old, [index]: times }));
    if (times.length >= 2) {
      const intervals = times.slice(1).map((time, i) => time - times[i]);
      const average = intervals.reduce((total, ms) => total + ms, 0) / intervals.length;
      mixer.decks[index].state.bpm = Math.round(60000 / average * 10) / 10;
      tick.current();
    }
  };
  const toggleStems = (id: string) => setExpandedStems(old => {
    const next = new Set(old);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });

  const leave = async () => {
    if (mixer?.recording) {
      try {
        const blob = await mixer.stopRecording();
        await saveFile(`YuE2-DJ-mix-${new Date().toISOString().replace(/[:.]/g, '-')}.webm`, { blob });
      } catch (problem) {
        setError(problem instanceof Error ? problem.message : String(problem));
        return;
      }
    }
    onClose();
  };
  leaveFromKeyboard.current = () => { void leave(); };
  const record = async () => {
    if (!mixer) return;
    setError(null);
    try {
      if (!mixer.recording) await mixer.startRecording();
      else {
        const blob = await mixer.stopRecording();
        const extension = blob.type.includes('ogg') ? 'ogg' : 'webm';
        await saveFile(`YuE2-DJ-mix-${new Date().toISOString().replace(/[:.]/g, '-') }.${extension}`, { blob });
      }
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };
  const navigateButton = (key: BrowserSource, label: string, count?: number) => (
    <button type="button" key={key} onClick={() => setSource(key)}
      className={`flex w-full items-center justify-between gap-2 rounded-md px-3 py-2 text-left text-xs transition ${source === key ? 'bg-fuchsia-500/20 text-white' : 'text-zinc-400 hover:bg-white/10 hover:text-white'}`}>
      <span className="truncate">{label}</span>
      {count !== undefined && <span className="text-[10px] tabular-nums opacity-70">{count}</span>}
    </button>
  );
  // revision drives React re-renders from Web Audio and HTMLMediaElement events.
  void revision;
  return (
    <div className="fixed inset-0 z-80 flex flex-col overflow-hidden bg-[#0b0c10] text-zinc-200" data-testid="dj-mode">
      <header className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-white/10 bg-[#17181d] px-4 py-2">
        <div className="flex items-center gap-3"><div className="rounded bg-red-600 px-2 py-1 text-base font-black italic text-white">YuE2 DJ</div>
          <span className="text-xs text-zinc-400">Two-deck music mixer · Library / Albums / Playlists / Stems</span>
        </div>
        <div className="flex items-center gap-2">
          <button type="button" className={`${button} flex items-center gap-2 ${mixer?.recording ? 'border-red-400 bg-red-500/20 text-red-300' : ''}`}
            onClick={() => void record()} disabled={!mixer}><Circle size={13} fill={mixer?.recording ? 'currentColor' : 'none'} />{mixer?.recording ? 'Stop & save recording' : 'Record mix'}</button>
          <button className={`${button} flex items-center gap-1.5`} type="button" onClick={() => void leave()}><X size={16} /> Back to Studio</button>
        </div>
      </header>
      {error && <p role="alert" className="bg-rose-900/30 px-4 py-1.5 text-xs text-rose-200">{error}</p>}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {mixer ? (
          <div className="mx-auto flex max-w-[1850px] flex-col gap-3 px-3 py-3">
            <div className="flex items-stretch gap-3 overflow-x-auto">
              <DjDeckPanel mixer={mixer} index={0} library={libraryPlayable} onSelect={setSelected} onLoad={load} onTap={tap} />
              <div className="flex w-24 shrink-0 flex-col items-center justify-center gap-5 rounded-xl border border-white/10 bg-zinc-900 px-2">
                <Headphones className="text-zinc-400" size={24} />
                <span className="text-[11px] font-bold uppercase tracking-wider text-zinc-400">MIX</span>
                <div className="h-32 w-1 rounded-full bg-linear-to-b from-blue-500 via-fuchsia-500 to-rose-500" aria-hidden />
                <button type="button" className={button} onClick={() => mixer.setCrossfade(0.5)}>CENTER</button>
              </div>
              <DjDeckPanel mixer={mixer} index={1} library={libraryPlayable} onSelect={setSelected} onLoad={load} onTap={tap} />
            </div>
            <div className="flex flex-wrap items-center justify-center gap-3 rounded-lg border border-white/10 bg-[#202126] p-3">
              <span className="font-black text-blue-400">A</span>
              <button type="button" className={button} onClick={() => mixer.setCrossfade(0)}>CUT A</button>
              <input className="h-2 min-w-[170px] max-w-[650px] flex-1 cursor-pointer accent-fuchsia-500" type="range" min="0" max="1" step="0.001"
                value={mixer.getCrossfade()} onChange={event => mixer.setCrossfade(Number(event.target.value))}
                aria-label="Equal-power crossfader" onDoubleClick={() => mixer.setCrossfade(0.5)} />
              <button type="button" className={button} onClick={() => mixer.setCrossfade(1)}>CUT B</button>
              <span className="font-black text-rose-400">B</span>
              <span className="text-xs text-zinc-400">{Math.round((1 - mixer.getCrossfade()) * 100)}% / {Math.round(mixer.getCrossfade() * 100)}%</span>
            </div>
            <div className="grid min-h-[300px] grid-cols-1 gap-3 lg:grid-cols-[185px_minmax(360px,1fr)_245px]">
              <nav className="min-h-0 rounded-xl border border-white/10 bg-[#17181d] p-2" aria-label="DJ library folders">
                <div className="mb-2 flex items-center gap-2 px-2 text-xs font-bold text-white"><FolderOpen size={14} /> MY LIBRARY</div>
                {navigateButton('all', 'All songs', libraryPlayable.filter(song => stemOf(song) === null).length)}
                {navigateButton('unassigned', 'Not in any collection')}
                {navigateButton('liked', 'Liked songs')}
                {navigateButton('stems', 'All stems')}
                <div className="my-2 border-t border-white/10" />
                <div className="px-2 py-1 text-[10px] font-bold uppercase text-zinc-500">Playlists</div>
                {playlists.filter(list => list.kind !== 'album').map(list => navigateButton(`collection:${list.id}`, list.name, list.songIds?.length))}
                <div className="my-2 border-t border-white/10" />
                <div className="px-2 py-1 text-[10px] font-bold uppercase text-zinc-500">Albums</div>
                {playlists.filter(list => list.kind === 'album').map(list => navigateButton(`collection:${list.id}`, list.name, list.songIds?.length))}
              </nav>
              <section className="min-w-0 overflow-hidden rounded-xl border border-white/10 bg-[#17181d]" aria-label="DJ song browser">
                <div className="flex flex-wrap items-center justify-between gap-2 border-b border-white/10 p-2.5">
                  <label className="flex min-w-[180px] flex-1 items-center gap-2 rounded-md border border-white/10 bg-black/30 px-2 py-1.5 text-zinc-400">
                    <Search size={14} />
                    <input className="w-full bg-transparent text-xs text-white outline-none placeholder:text-zinc-500"
                      value={search} onChange={event => setSearch(event.target.value)} placeholder="Search title, style, genre…" aria-label="Search DJ library" />
                  </label>
                  <span className="text-xs text-zinc-400">{visibleSongs.length} tracks · drag to A or B</span>
                </div>
                <div className="max-h-[390px] overflow-y-auto">
                  {visibleSongs.length === 0 && <p className="p-6 text-center text-sm text-zinc-400">No tracks found in this collection.</p>}
                  {visibleSongs.map(song => {
                    const stems = stemsOf.get(song.id) ?? [];
                    const active = selected?.id === song.id;
                    return (
                      <React.Fragment key={song.id}>
                        <div draggable
                          onDragStart={event => {
                            event.dataTransfer.setData(DJ_MIME, song.id);
                            event.dataTransfer.setData('text/plain', song.id);
                            event.dataTransfer.effectAllowed = 'copy';
                          }}
                          onClick={() => setSelected(song)}
                          onDoubleClick={() => load(0, song)}
                          className={`group flex cursor-grab items-center gap-2 border-b border-white/5 px-2 py-1.5 transition hover:bg-white/10 ${active ? 'bg-fuchsia-500/10' : ''}`}>
                          <img className="h-10 w-10 shrink-0 rounded object-cover" src={trackCoverUrl(song)} alt="" loading="lazy" />
                          <div className="min-w-0 flex-1">
                            <div className="truncate text-xs font-bold text-white">{song.title}</div>
                            <div className="truncate text-[10px] text-zinc-400">{song.style || 'YuE2 Studio'}</div>
                          </div>
                          <span className="hidden w-14 shrink-0 text-right font-mono text-[11px] text-zinc-400 sm:block">{song.bpm ? `${song.bpm} BPM` : ''}</span>
                          <span className="hidden w-12 shrink-0 text-right font-mono text-[11px] text-zinc-400 sm:block">{song.duration}</span>
                          {stems.length > 0 && <button type="button" onClick={event => { event.stopPropagation(); toggleStems(song.id); }}
                            className={button} title="Show individual stems"><AudioLines size={14} /></button>}
                          <button type="button" className={`${button} shrink-0`} onClick={event => { event.stopPropagation(); load(0, song); }}>A</button>
                          <button type="button" className={`${button} shrink-0`} onClick={event => { event.stopPropagation(); load(1, song); }}>B</button>
                        </div>
                        {expandedStems.has(song.id) && stems.map(stem => (
                          <div key={stem.id} draggable onDragStart={event => {
                            event.dataTransfer.setData(DJ_MIME, stem.id);
                            event.dataTransfer.setData('text/plain', stem.id);
                            event.dataTransfer.effectAllowed = 'copy';
                          }} className="flex items-center gap-2 border-b border-white/5 bg-black/30 py-1 pl-12 pr-2 text-xs">
                            <Waves size={13} className="text-fuchsia-400" />
                            <span className="min-w-0 flex-1 truncate">{stemOf(stem)} · {stem.title}</span>
                            <button className={button} onClick={() => load(0, stem)}>A</button>
                            <button className={button} onClick={() => load(1, stem)}>B</button>
                          </div>
                        ))}
                      </React.Fragment>
                    );
                  })}
                </div>
              </section>
              <aside className="min-w-0 rounded-xl border border-white/10 bg-[#17181d] p-3" aria-label="DJ sideview">
                <div className="mb-3 flex items-center gap-2 text-xs font-bold text-white"><ListMusic size={15} /> SIDEVIEW</div>
                {selected ? <>
                  <img className="mx-auto aspect-square w-full max-w-[165px] rounded-lg object-cover" src={trackCoverUrl(selected)} alt="" />
                  <div className="mt-3 truncate text-sm font-bold text-white">{selected.title}</div>
                  <div className="mt-1 text-xs text-zinc-400">{selected.creator || 'YuE2 Studio'}</div>
                  <dl className="mt-3 grid grid-cols-2 gap-2 text-[11px]">
                    <dt className="text-zinc-500">Duration</dt><dd className="text-right">{selected.duration}</dd>
                    <dt className="text-zinc-500">BPM</dt><dd className="text-right">{selected.bpm || '—'}</dd>
                    <dt className="text-zinc-500">Key</dt><dd className="text-right">{selected.keyScale || '—'}</dd>
                    <dt className="text-zinc-500">Stems</dt><dd className="text-right">{stemsOf.get(selected.id)?.length || (stemOf(selected) ? 1 : 0)}</dd>
                  </dl>
                  <div className="mt-3 max-h-24 overflow-y-auto text-[11px] leading-5 text-zinc-500">{selected.style}</div>
                  <div className="mt-3 flex gap-2">
                    <button type="button" className={`${button} flex-1`} onClick={() => load(0, selected)}>LOAD A</button>
                    <button type="button" className={`${button} flex-1`} onClick={() => load(1, selected)}>LOAD B</button>
                  </div>
                </> : <p className="text-xs text-zinc-500">Select a song to inspect its artwork, BPM, key and stems.</p>}
              </aside>
            </div>
            <footer className="flex flex-wrap items-center justify-between gap-2 px-2 py-1 text-[11px] text-zinc-500">
              <span>Live Web Audio mixing · scratch in either direction · per-stem mute/solo · EQ + echo + filter</span>
              <span>Alt+1 / Alt+2 play · Shift+click hot cue to set · double-click a track to load A</span>
            </footer>
          </div>
        ) : <div className="p-12 text-center text-zinc-400">Preparing the audio mixer…</div>}
      </div>
    </div>
  );
};
