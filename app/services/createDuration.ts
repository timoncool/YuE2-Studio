/** A max-duration loaded from a request (a reused track, an example, the
 *  writing assistant) never drops below the duration the user keeps for every
 *  new song (studio.createDuration); a longer one is honoured (#184). An absent
 *  or non-finite value keeps the saved maximum. */
export function clampDurationToKept(asked: number | undefined, kept: number): number {
  return asked !== undefined && Number.isFinite(asked) && asked > kept ? Math.round(asked) : kept;
}
