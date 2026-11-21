import React, { useEffect, useRef, useState } from 'react';
import { Pause, Play } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { AudioWaveform } from './AudioWaveform';

/**
 * Before and after, playing together with one of them heard: switching keeps
 * the position, so the difference is the only thing that changes.
 */
export const AudioComparison: React.FC<{ original: string; processed: string; beforeLabel?: string; afterLabel?: string }> = ({ original, processed, beforeLabel, afterLabel }) => {
  const { t } = useI18n();
  const before = useRef<HTMLAudioElement | null>(null);
  const after = useRef<HTMLAudioElement | null>(null);
  const [side, setSide] = useState<'before' | 'after'>('after');
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);

  useEffect(() => {
    if (before.current) before.current.muted = side !== 'before';
    if (after.current) after.current.muted = side !== 'after';
  }, [side]);

  useEffect(() => {
    const tracks = [before.current, after.current];
    return () => tracks.forEach(track => track?.pause());
  }, []);

  // keep the silent one aligned with the heard one
  useEffect(() => {
    if (!playing) return;
    const timer = window.setInterval(() => {
      const a = before.current;
      const b = after.current;
      if (!a || !b) return;
      const [lead, follow] = side === 'after' ? [b, a] : [a, b];
      setTime(lead.currentTime);
      if (Math.abs(follow.currentTime - lead.currentTime) > 0.05) follow.currentTime = lead.currentTime;
    }, 200);
    return () => window.clearInterval(timer);
  }, [playing, side]);

  const both = () => [before.current, after.current].filter((element): element is HTMLAudioElement => Boolean(element));
  const toggle = async () => {
    if (playing) {
      both().forEach(element => element.pause());
      setPlaying(false);
    } else {
      await Promise.all(both().map(element => element.play().catch(() => undefined)));
      setPlaying(true);
    }
  };
  const seek = (value: number) => {
    both().forEach(element => { element.currentTime = value; });
    setTime(value);
  };
  const clock = (value: number) => `${Math.floor(value / 60)}:${String(Math.floor(value % 60)).padStart(2, '0')}`;

  return (
    <section className="rounded-xl border border-pink-400/40 bg-pink-500/5 p-3">
      <audio ref={before} src={original} preload="auto" muted onEnded={() => setPlaying(false)} />
      <audio ref={after} src={processed} preload="auto" onLoadedMetadata={event => setDuration(event.currentTarget.duration)} onEnded={() => setPlaying(false)} />
      <div className="flex items-center gap-3">
        <button type="button" onClick={() => void toggle()} className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-linear-to-r from-orange-500 to-pink-600 text-white" aria-label={playing ? 'pause' : 'play'}>
          {playing ? <Pause size={16} /> : <Play size={16} className="ml-0.5" />}
        </button>
        <div role="tablist" className="flex flex-1 rounded-lg bg-zinc-100 p-1 dark:bg-white/5">
          {(['before', 'after'] as const).map(value => (
            <button
              key={value}
              type="button"
              role="tab"
              aria-selected={side === value}
              onClick={() => {
                const lead = side === 'before' ? before.current : after.current;
                const next = value === 'before' ? before.current : after.current;
                if (lead && next && next !== lead) next.currentTime = lead.currentTime;
                setSide(value);
              }}
              className={`flex-1 rounded-md py-1.5 text-xs font-semibold transition-all ${side === value ? 'bg-white text-black shadow-xs dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}
            >
              {value === 'before' ? beforeLabel ?? t('processBefore') : afterLabel ?? t('processAfter')}
            </button>
          ))}
        </div>
      </div>
      <div className="mt-3 flex items-center gap-2 text-[11px] tabular-nums text-zinc-500">
        <span>{clock(time)}</span>
        <div className="min-w-0 flex-1">
          <AudioWaveform url={side === 'before' ? original : processed} currentTime={time} duration={duration} height={36} onSeek={share => seek(share * duration)} />
        </div>
        <span>{clock(duration)}</span>
      </div>
      <p className="mt-2 text-[11px] text-zinc-500">{t('processCompareHint')}</p>
    </section>
  );
};
