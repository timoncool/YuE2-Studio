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
export function afterGenerationPause(duration: number, next: () => void): () => void {
  const deadline = performance.now() + duration;
  let timer: ReturnType<typeof setTimeout>;
  const advance = () => {
    const remaining = Math.ceil(deadline - performance.now());
    if (remaining <= 0) next();
    else timer = setTimeout(advance, Math.min(remaining, maxTimeout));
  };
  timer = setTimeout(advance, Math.min(duration, maxTimeout));
  return () => clearTimeout(timer);
}
