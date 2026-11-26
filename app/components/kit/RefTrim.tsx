import { useEffect, useMemo, useRef, useState } from 'react';
import { Loader2, Pause, Play, Scissors } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { soundSourceStrings } from '../../i18n/soundSource';
import { SecondaryButton } from './Buttons';

const BARS = 160;

/** Speech Studio's RefTrim: a recording's waveform with a start and an end to keep; listen to the piece, then keep it.
 * `limit` is the longest piece the place takes and `min` the shortest; `initial` is the piece kept now. */
export function RefTrim({ url, limit, min = 3, initial, busy, onApply }: {
  url: string; limit?: number; min?: number; initial?: [number, number] | null; busy: boolean; onApply: (start: number, end: number) => void;
}) {
  const { language } = useI18n();
  const text = soundSourceStrings[language];
  const [peaks, setPeaks] = useState<number[] | null>(null);
  const [duration, setDuration] = useState(0);
  const [range, setRange] = useState<[number, number]>([0, 0]);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const player = useRef<HTMLAudioElement | null>(null);
  const [from, to] = initial ?? [null, null];
  useEffect(() => {
    let alive = true;
    const ctx = new AudioContext();
    fetch(url)
      .then(response => { if (!response.ok) throw new Error(`${response.status} ${response.statusText}`); return response.arrayBuffer(); })
      .then(bytes => ctx.decodeAudioData(bytes))
      .then(audio => {
        if (!alive) return;
        const data = audio.getChannelData(0);
        const step = Math.max(1, Math.floor(data.length / BARS));
        const out: number[] = [];
        for (let i = 0; i < BARS; i++) {
          let top = 0;
          for (let k = i * step; k < Math.min(data.length, (i + 1) * step); k++) top = Math.max(top, Math.abs(data[k]));
          out.push(top);
        }
        const loudest = Math.max(...out, 1e-6);
        setPeaks(out.map(peak => peak / loudest));
        setDuration(audio.duration);
        setRange(from != null && to != null ? [from, to] : [0, Math.min(audio.duration, limit ?? audio.duration)]);
      })
      .catch((failure: unknown) => { if (alive) setError(failure instanceof Error ? failure.message : String(failure)); })
      .finally(() => { void ctx.close(); });
    return () => { alive = false; player.current?.pause(); };
  }, [url, limit, from, to]);
  const [start, end] = range;
  const length = end - start;
  const listen = () => {
    player.current?.pause();
    if (playing) { setPlaying(false); return; }
    const audio = new Audio(url);
    player.current = audio;
    audio.currentTime = start;
    audio.ontimeupdate = () => { if (audio.currentTime >= end) { audio.pause(); setPlaying(false); } };
    audio.onended = () => setPlaying(false);
    setPlaying(true);
    audio.play().catch((failure: unknown) => { setPlaying(false); setError(failure instanceof Error ? failure.message : String(failure)); });
  };
  const bars = useMemo(() => peaks?.map((peak, index) => {
    const at = (index / BARS) * duration;
    const inside = at >= start && at <= end;
    return <rect key={index} x={index * 3} y={20 - peak * 18} width={2} height={Math.max(1, peak * 36)} className={inside ? 'fill-pink-500' : 'fill-zinc-400 opacity-40'} />;
  }), [peaks, duration, start, end]);
  if (error) return <p role="alert" className="text-[13px] text-rose-700 dark:text-rose-300">{error}</p>;
  if (!peaks) return <Loader2 size={14} className="animate-spin text-zinc-500" />;
  const field = 'w-full accent-pink-500';
  const long = limit != null && length > limit + 0.05;
  const short = length < min;
  return (
    <div className="grid gap-1.5 text-[13px]">
      <svg viewBox={`0 0 ${BARS * 3} 40`} preserveAspectRatio="none" className="h-12 w-full rounded-md bg-zinc-100 dark:bg-white/5" aria-hidden>{bars}</svg>
      <label className="grid text-zinc-600 dark:text-zinc-400">{text.trimStart(start.toFixed(1))}
        <input type="range" min={0} max={duration} step={0.1} value={start} onChange={event => setRange([Math.min(Number(event.target.value), end - 0.5), end])} className={field} />
      </label>
      <label className="grid text-zinc-600 dark:text-zinc-400">{text.trimEnd(end.toFixed(1))}
        <input type="range" min={0} max={duration} step={0.1} value={end} onChange={event => setRange([start, Math.max(Number(event.target.value), start + 0.5)])} className={field} />
      </label>
      <div className="flex flex-wrap items-center gap-2">
        <SecondaryButton onClick={listen} aria-label={playing ? text.trimStop : text.trimListen}>{playing ? <Pause size={13} /> : <Play size={13} />}{playing ? text.trimStop : text.trimListen}</SecondaryButton>
        <span className={`tabular-nums ${long || short ? 'text-rose-700 dark:text-rose-300' : 'text-zinc-600 dark:text-zinc-400'}`}>
          {text.trimLength(length.toFixed(1))}{long ? ` · ${text.trimTooLong(String(limit))}` : short ? ` · ${text.trimTooShort}` : ''}
        </span>
        <span className="ml-auto">
          <SecondaryButton onClick={() => { player.current?.pause(); onApply(start, end); }}
            disabled={busy || long || short || (initial != null && start === initial[0] && end === initial[1])}>
            {busy ? <Loader2 size={12} className="animate-spin" /> : <Scissors size={12} />}{text.trimApply}
          </SecondaryButton>
        </span>
      </div>
    </div>
  );
}
