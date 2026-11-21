import React, { useEffect, useRef, useState } from 'react';
import { Pause, Play } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { AudioWaveform } from './AudioWaveform';

type Side = 'before' | 'after';

export const AudioCompare: React.FC<{
  original: string;
  processed: string;
  labels?: [string, string];
  action?: (side: Side) => React.ReactNode;
}> = ({ original, processed, labels, action }) => {
  const { t } = useI18n();
  const before = useRef<HTMLAudioElement | null>(null);
  const after = useRef<HTMLAudioElement | null>(null);
  const [side, setSide] = useState<Side>('after');
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [durations, setDurations] = useState<Record<Side, number>>({ before: 0, after: 0 });
  const duration = durations[side];

  useEffect(() => {
    if (before.current) before.current.muted = side !== 'before';
    if (after.current) after.current.muted = side !== 'after';
  }, [side]);

  // keep the silent one aligned with the heard one
  useEffect(() => {
    if (!playing) return;
    const timer = window.setInterval(() => {
      const a = before.current;
      const b = after.current;
      if (!a || !b) return;
      const [lead, follow] = side === 'after' ? [b, a] : [a, b];
      setTime(lead.currentTime);
      if (follow.readyState > 0 && lead.currentTime < follow.duration) {
        if (Math.abs(follow.currentTime - lead.currentTime) > 0.05) follow.currentTime = lead.currentTime;
      }
    }, 200);
    return () => window.clearInterval(timer);
  }, [playing, side]);

  const both = () => [before.current, after.current].filter((element): element is HTMLAudioElement => Boolean(element));
  const stop = () => {
    both().forEach(element => element.pause());
    setPlaying(false);
  };
  const toggle = async () => {
    if (playing) {
      stop();
    } else {
      const audible = side === 'before' ? before.current : after.current;
      if (!audible) return;
      if (audible.ended || (duration > 0 && audible.currentTime >= duration)) seek(0);
      const silent = side === 'before' ? after.current : before.current;
      if (silent && (!Number.isFinite(silent.duration) || silent.currentTime < silent.duration)) {
        void silent.play().catch(() => undefined);
      }
      try {
        await audible.play();
        setPlaying(true);
      } catch {
        stop();
      }
    }
  };
  const switchSide = (value: Side) => {
    if (value === side) return;
    const a = before.current;
    const b = after.current;
    if (a && b) {
      const [lead, follow] = value === 'after' ? [a, b] : [b, a];
      lead.muted = true;
      const end = durations[value];
      const position = end > 0 ? Math.min(lead.currentTime, end) : lead.currentTime;
      if (follow.readyState > 0) follow.currentTime = position;
      follow.muted = false;
      setTime(position);
      if (end > 0 && position >= end) {
        stop();
      } else if (playing && follow.paused) {
        void follow.play().catch(stop);
      }
    }
    setSide(value);
  };
  const seek = (value: number) => {
    both().forEach(element => {
      element.currentTime = Number.isFinite(element.duration) ? Math.min(value, element.duration) : value;
    });
    setTime(value);
  };
  const mediaEvents = (value: Side) => ({
    onLoadedMetadata: (event: React.SyntheticEvent<HTMLAudioElement>) => {
      const length = event.currentTarget.duration;
      setDurations(previous => ({ ...previous, [value]: Number.isFinite(length) ? length : 0 }));
    },
    onEnded: (event: React.SyntheticEvent<HTMLAudioElement>) => {
      if (side !== value) return;
      setTime(event.currentTarget.currentTime);
      stop();
    },
  });
  const clock = (value: number) => `${Math.floor(value / 60)}:${String(Math.floor(value % 60)).padStart(2, '0')}`;

  return (
    <section className="rounded-xl border border-pink-400/40 bg-pink-500/5 p-3">
      <audio ref={before} src={original} preload="auto" muted {...mediaEvents('before')} />
      <audio ref={after} src={processed} preload="auto" {...mediaEvents('after')} />
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
              onClick={() => switchSide(value)}
              className={`flex-1 rounded-md py-1.5 text-xs font-semibold transition-all ${side === value ? 'bg-white text-black shadow-xs dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}
            >
              {value === 'before' ? labels?.[0] ?? t('processBefore') : labels?.[1] ?? t('processAfter')}
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
      {action?.(side)}
    </section>
  );
};
