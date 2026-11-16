import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Download, Pause, Play, Repeat } from 'lucide-react';
import { AudioWaveform } from './AudioWaveform';
import { useI18n } from '../context/I18nContext';
import { keepContextRunning } from '../services/audioGraph';
import { clampLoop, clampPan, drifted, freshChannel, heardGain, loopJump, panLabel, volumeDb, type LoopRegion, type StemChannel } from '../services/stemMix';

/**
 * The stems of one song played together: one transport, and per stem mute,
 * solo, volume and pan, with a loop region to audition a section.
 *
 * Each stem is its own audio element feeding a gain and a stereo panner. The
 * transport is the first stem; the others follow it and are put back when
 * they drift more than 30 ms. Looping is done by seeking, so its seam is
 * about a frame of the screen, not a sample: this is for listening, and the
 * stems themselves are untouched by anything set here.
 */

const clock = (seconds: number): string => {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00.0';
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds - minutes * 60).toFixed(1).padStart(4, '0')}`;
};

interface Props {
  stems: string[];
  /** Where a stem's audio is served. */
  urlFor: (stem: string) => string;
  /** The mix the stems came from, drawn as the transport's waveform. */
  mixUrl: string;
  label: (stem: string) => string;
  onDownload: (stem: string) => void;
}

interface Graph {
  context: AudioContext;
  nodes: Map<string, { gain: GainNode; pan: StereoPannerNode }>;
  /** Ends the watch that keeps the context running. */
  release: () => void;
}

const SLIDER = 'h-1 w-20 accent-pink-500';
const SMALL_BUTTON = 'grid h-6 w-6 place-items-center rounded text-[10px] font-bold transition';

export const StemMixer: React.FC<Props> = ({ stems, urlFor, mixUrl, label, onDownload }) => {
  const { t } = useI18n();
  const elements = useRef(new Map<string, HTMLAudioElement>());
  const graph = useRef<Graph | null>(null);
  const [channels, setChannels] = useState<Record<string, StemChannel>>(() => Object.fromEntries(stems.map(stem => [stem, freshChannel()])));
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [length, setLength] = useState(0);
  const [loop, setLoop] = useState<LoopRegion>({ on: false, start: 0, end: 0 });
  const loopRef = useRef<LoopRegion | null>(null);
  loopRef.current = clampLoop(loop, length);

  const master = () => elements.current.get(stems[0]);

  // a stem appearing or going (separated again with other choices) gets a fresh channel
  useEffect(() => {
    setChannels(current => Object.fromEntries(stems.map(stem => [stem, current[stem] ?? freshChannel()])));
  }, [stems]);

  // the audio graph is built on the first play: browsers keep a context made earlier suspended
  const ensureGraph = useCallback(() => {
    if (graph.current) return graph.current;
    const context = new AudioContext();
    const release = keepContextRunning(context);
    const nodes = new Map<string, { gain: GainNode; pan: StereoPannerNode }>();
    for (const stem of stems) {
      const element = elements.current.get(stem);
      if (!element) continue;
      const source = context.createMediaElementSource(element);
      const gain = context.createGain();
      const pan = context.createStereoPanner();
      source.connect(gain).connect(pan).connect(context.destination);
      nodes.set(stem, { gain, pan });
    }
    graph.current = { context, nodes, release };
    return graph.current;
  }, [stems]);

  // the graph follows the channel settings
  useEffect(() => {
    const built = graph.current;
    if (!built) return;
    const all = stems.map(stem => channels[stem] ?? freshChannel());
    for (const stem of stems) {
      const node = built.nodes.get(stem);
      const channel = channels[stem] ?? freshChannel();
      if (!node) continue;
      node.gain.gain.setTargetAtTime(heardGain(channel, all), built.context.currentTime, 0.01);
      node.pan.pan.setTargetAtTime(clampPan(channel.pan), built.context.currentTime, 0.01);
    }
  }, [channels, stems, playing]);

  // the transport: follow the first stem, keep the rest with it, and loop
  useEffect(() => {
    if (!playing) return;
    let frame = 0;
    const tick = () => {
      const lead = master();
      if (lead) {
        const jump = loopJump(lead.currentTime, loopRef.current);
        if (jump !== null) {
          for (const element of elements.current.values()) element.currentTime = jump;
        } else {
          for (const element of elements.current.values()) {
            if (element !== lead && drifted(element.currentTime, lead.currentTime)) element.currentTime = lead.currentTime;
          }
        }
        setPosition(lead.currentTime);
        if (lead.ended) setPlaying(false);
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing, stems]);

  // stop everything on the way out
  useEffect(() => () => {
    for (const element of elements.current.values()) element.pause();
    graph.current?.release();
    void graph.current?.context.close();
  }, []);

  const toggle = async () => {
    const all = [...elements.current.values()];
    if (playing) {
      all.forEach(element => element.pause());
      setPlaying(false);
      return;
    }
    const built = ensureGraph();
    await built.context.resume();
    const lead = master();
    const at = loopRef.current?.on ? (loopJump(lead?.currentTime ?? 0, loopRef.current) ?? lead?.currentTime ?? 0) : lead?.currentTime ?? 0;
    all.forEach(element => { element.currentTime = at; });
    await Promise.all(all.map(element => element.play().catch(() => undefined)));
    setPlaying(true);
  };

  const seek = (share: number) => {
    if (!Number.isFinite(length) || length <= 0) return;
    const time = share * length;
    for (const element of elements.current.values()) element.currentTime = time;
    setPosition(time);
  };

  const update = (stem: string, change: Partial<StemChannel>) =>
    setChannels(current => ({ ...current, [stem]: { ...(current[stem] ?? freshChannel()), ...change } }));

  const shownLoop = loopRef.current;

  return (
    <div className="space-y-2" data-testid="stem-mixer">
      <div className="flex items-center gap-3 rounded-lg border border-zinc-200 px-3 py-2 dark:border-white/10">
        <button
          type="button"
          onClick={() => void toggle()}
          className="grid h-9 w-9 shrink-0 place-items-center rounded-full bg-linear-to-r from-orange-500 to-pink-600 text-white"
          aria-label={t(playing ? 'stemMixPause' : 'stemMixPlay')}
          title={t(playing ? 'stemMixPause' : 'stemMixPlay')}
        >
          {playing ? <Pause size={15} /> : <Play size={15} className="ml-0.5" />}
        </button>
        <span className="w-14 shrink-0 text-right text-[11px] tabular-nums text-zinc-500" data-testid="stem-position">{clock(position)}</span>
        <div className="relative min-w-0 flex-1">
          <AudioWaveform url={mixUrl} currentTime={position} duration={length} height={36} onSeek={seek} />
          {shownLoop?.on && length > 0 && (
            <div
              className="pointer-events-none absolute inset-y-0 rounded bg-pink-500/15 ring-1 ring-pink-500/50"
              style={{ left: `${(shownLoop.start / length) * 100}%`, width: `${((shownLoop.end - shownLoop.start) / length) * 100}%` }}
            />
          )}
        </div>
        <span className="w-14 shrink-0 text-[11px] tabular-nums text-zinc-500">{clock(length)}</span>
      </div>

      <div className="flex flex-wrap items-center gap-2 text-[11px] text-zinc-600 dark:text-zinc-300">
        <button
          type="button"
          aria-pressed={loop.on}
          onClick={() => setLoop(current => ({ ...current, on: !current.on, end: current.end > current.start ? current.end : Math.min(length, current.start + 8) }))}
          className={`inline-flex items-center gap-1 rounded-md border px-2 py-1 font-semibold ${loop.on ? 'border-pink-400 text-pink-600 dark:text-pink-300' : 'border-zinc-200 dark:border-white/10'}`}
        >
          <Repeat size={12} />{t('stemMixLoop')}
        </button>
        <label className="inline-flex items-center gap-1">{t('stemMixFrom')}
          <input type="number" step="0.1" min={0} aria-label={t('stemMixLoopStart')} value={Number((shownLoop?.start ?? loop.start).toFixed(1))}
            onChange={event => setLoop(current => ({ ...current, start: Number(event.target.value) }))}
            className="w-16 rounded border border-zinc-200 bg-transparent px-1 py-0.5 tabular-nums dark:border-white/10" />
        </label>
        <label className="inline-flex items-center gap-1">{t('stemMixTo')}
          <input type="number" step="0.1" min={0} aria-label={t('stemMixLoopEnd')} value={Number((shownLoop?.end ?? loop.end).toFixed(1))}
            onChange={event => setLoop(current => ({ ...current, end: Number(event.target.value) }))}
            className="w-16 rounded border border-zinc-200 bg-transparent px-1 py-0.5 tabular-nums dark:border-white/10" />
        </label>
        <button type="button" className="rounded-md border border-zinc-200 px-2 py-1 dark:border-white/10" onClick={() => setLoop(current => ({ ...current, start: position }))}>{t('stemMixStartHere')}</button>
        <button type="button" className="rounded-md border border-zinc-200 px-2 py-1 dark:border-white/10" onClick={() => setLoop(current => ({ ...current, end: position }))}>{t('stemMixEndHere')}</button>
      </div>

      {stems.map(stem => {
        const channel = channels[stem] ?? freshChannel();
        return (
          <div key={stem} className="flex items-center gap-2 rounded-lg border border-zinc-200 px-3 py-1.5 dark:border-white/10" data-testid={`stem-row-${stem}`}>
            <span className="w-20 shrink-0 truncate text-xs font-semibold uppercase tracking-wide text-zinc-500">{label(stem)}</span>
            <button type="button" aria-pressed={channel.mute} aria-label={`${t('stemMixMute')}: ${label(stem)}`} title={t('stemMixMute')}
              onClick={() => update(stem, { mute: !channel.mute })}
              className={`${SMALL_BUTTON} ${channel.mute ? 'bg-amber-500 text-white' : 'border border-zinc-200 text-zinc-500 dark:border-white/10'}`}>M</button>
            <button type="button" aria-pressed={channel.solo} aria-label={`${t('stemMixSolo')}: ${label(stem)}`} title={t('stemMixSolo')}
              onClick={() => update(stem, { solo: !channel.solo })}
              className={`${SMALL_BUTTON} ${channel.solo ? 'bg-emerald-500 text-white' : 'border border-zinc-200 text-zinc-500 dark:border-white/10'}`}>S</button>
            <input type="range" min={0} max={2} step={0.01} value={channel.volume} aria-label={`${t('stemMixVolume')}: ${label(stem)}`} title={t('stemMixVolume')}
              onChange={event => update(stem, { volume: Number(event.target.value) })} onDoubleClick={() => update(stem, { volume: 1 })} className={SLIDER} />
            <span className="w-14 shrink-0 text-[10px] tabular-nums text-zinc-500">{volumeDb(channel.volume)}</span>
            <input type="range" min={-1} max={1} step={0.01} value={channel.pan} aria-label={`${t('stemMixPan')}: ${label(stem)}`} title={t('stemMixPan')}
              onChange={event => update(stem, { pan: Number(event.target.value) })} onDoubleClick={() => update(stem, { pan: 0 })} className={SLIDER} />
            <span className="w-9 shrink-0 text-[10px] tabular-nums text-zinc-500">{panLabel(channel.pan)}</span>
            <span className="flex-1" />
            <button type="button" onClick={() => onDownload(stem)} className="shrink-0 text-zinc-400 hover:text-pink-500" title={t('stemMixSave')} aria-label={`${t('stemMixSave')}: ${label(stem)}`}>
              <Download size={15} />
            </button>
            <audio
              ref={element => { if (element) elements.current.set(stem, element); else elements.current.delete(stem); }}
              src={urlFor(stem)}
              // the desktop window is on another origin than the service: without CORS, Web Audio hears silence
              crossOrigin="anonymous"
              preload="auto"
              onLoadedMetadata={event => { if (stem === stems[0]) setLength(event.currentTarget.duration); }}
              className="hidden"
            />
          </div>
        );
      })}
      <p className="text-[11px] leading-5 text-zinc-500">{t('stemMixNote')}</p>
    </div>
  );
};
