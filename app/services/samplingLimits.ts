/** Frames the semantic stage writes per second of song. */
export const FRAMES_PER_SECOND = 25;

/**
 * Whether a minimum of `minTokens` frames leaves the model no room to end the song by itself before the length
 * limit: at 90 % of the frames the duration allows, it sings on to the limit and is cut off there.
 */
export function minTokensCutsTheSongOff(minTokens: string, durationSeconds: number): boolean {
  const minimum = Number(minTokens);
  if (minTokens.trim() === '' || !Number.isFinite(minimum) || minimum <= 0 || durationSeconds <= 0) return false;
  return minimum >= 0.9 * durationSeconds * FRAMES_PER_SECOND;
}
