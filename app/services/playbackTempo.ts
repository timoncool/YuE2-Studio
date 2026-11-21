/**
 * Tape-style playback speed shared by the library player and both DJ decks.
 * 1× is the original recording. Faster playback raises pitch (preservesPitch=false);
 * 2× is one octave up, 0.5× one octave down.
 */
export const MIN_PLAYBACK_RATE = 0.25;
export const MAX_PLAYBACK_RATE = 2;
export const PLAYBACK_RATE_PRESETS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2] as const;

export function semitonesFromPlaybackRate(rate: number): number {
  return 12 * Math.log2(rate);
}
