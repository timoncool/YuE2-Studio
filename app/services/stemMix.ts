/**
 * The stem mixer's arithmetic, kept apart from the audio graph so it can be
 * tested: what each stem is heard at, and where a looping transport goes next.
 */

export interface StemChannel {
  /** Linear gain, 0 to 2 (+6 dB). */
  volume: number;
  /** -1 hard left, 0 centre, 1 hard right. */
  pan: number;
  mute: boolean;
  solo: boolean;
}

export interface LoopRegion {
  on: boolean;
  start: number;
  end: number;
}

export const freshChannel = (): StemChannel => ({ volume: 1, pan: 0, mute: false, solo: false });

/** What a stem is heard at: nothing when muted, or when another stem is soloed and this one is not. */
export function heardGain(channel: StemChannel, all: StemChannel[]): number {
  if (channel.mute) return 0;
  if (all.some(other => other.solo) && !channel.solo) return 0;
  return Math.min(2, Math.max(0, channel.volume));
}

export function clampPan(pan: number): number {
  return Math.min(1, Math.max(-1, Number.isFinite(pan) ? pan : 0));
}

/** The shortest loop the transport can follow: a media element seeks in steps far coarser than a sample. */
export const MIN_LOOP_SECONDS = 0.25;

/** A loop kept inside the track and at least MIN_LOOP_SECONDS long; null when the track is too short for one. */
export function clampLoop(loop: LoopRegion, duration: number): LoopRegion | null {
  if (!Number.isFinite(duration) || duration < MIN_LOOP_SECONDS) return null;
  const start = Math.min(Math.max(0, Number.isFinite(loop.start) ? loop.start : 0), duration - MIN_LOOP_SECONDS);
  const end = Math.min(duration, Math.max(start + MIN_LOOP_SECONDS, Number.isFinite(loop.end) ? loop.end : duration));
  return { on: loop.on, start, end };
}

/** Where the transport must jump to, or null to carry on: past a loop's end it goes back to the start. */
export function loopJump(time: number, loop: LoopRegion | null): number | null {
  if (!loop || !loop.on) return null;
  if (time >= loop.end || time < loop.start - 0.05) return loop.start;
  return null;
}

/** How far one stem may drift from the transport before it is put back, in seconds. */
export const DRIFT_SECONDS = 0.03;

export function drifted(stemTime: number, transportTime: number): boolean {
  return Math.abs(stemTime - transportTime) > DRIFT_SECONDS;
}

export function volumeDb(volume: number): string {
  if (volume <= 0) return '-∞ dB';
  const db = 20 * Math.log10(volume);
  return `${db >= 0 ? '+' : ''}${db.toFixed(1)} dB`;
}

export function panLabel(pan: number): string {
  const amount = Math.round(Math.abs(clampPan(pan)) * 100);
  if (amount === 0) return 'C';
  return `${pan < 0 ? 'L' : 'R'}${amount}`;
}
