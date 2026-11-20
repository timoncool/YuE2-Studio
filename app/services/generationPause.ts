export type GenerationPauseMode = 'none' | 'fixed' | 'random';

const minuteMs = 60_000;
const maxTimeout = 2_147_483_647;

export function generationPauseMs(mode: GenerationPauseMode, from: string, to: string, random = Math.random): number | null {
  if (mode === 'none') return 0;
  const minutesToMs = (value: string) => {
    const minutes = value.trim() ? Number(value) : NaN;
    return minutes >= 0 ? Math.round(minutes * minuteMs) : NaN;
  };
  const start = minutesToMs(from);
  const end = mode === 'fixed' ? start : minutesToMs(to);
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || end < start) return null;
  return mode === 'fixed' ? start : start + Math.round(random() * (end - start));
}

/** A long pause must not overflow the host timer and start the next job immediately. */
export function afterGenerationPause(duration: number, next: () => void, update?: (seconds: number) => void): () => void {
  const deadline = performance.now() + duration;
  const schedule = (remaining: number): ReturnType<typeof setTimeout> => {
    const seconds = Math.ceil(remaining / 1000);
    if (remaining > 0) update?.(seconds);
    // Schedule the next displayed second from the same deadline that sends the job.
    const tick = update ? remaining - Math.max(0, seconds - 1) * 1000 : maxTimeout;
    return setTimeout(advance, Math.min(remaining, tick, maxTimeout));
  };
  const advance = () => {
    const remaining = Math.ceil(deadline - performance.now());
    if (remaining <= 0) next();
    else timer = schedule(remaining);
  };
  let timer = schedule(duration);
  return () => clearTimeout(timer);
}

export function formatGenerationCountdown(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, '0')}`;
}
