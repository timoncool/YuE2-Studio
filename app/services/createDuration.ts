/** A max-duration loaded from a request (a reused track, an example, the
 *  writing assistant) never drops below the duration the user keeps for every
 *  new song (studio.createDuration); a longer one is honoured (#184). An absent
 *  or non-finite value keeps the saved maximum. */
export function clampDurationToKept(asked: number | undefined, kept: number): number {
  return asked !== undefined && Number.isFinite(asked) && asked > kept ? Math.round(asked) : kept;
}

/** The form's max duration when it sends none: the engine's whole budget, so the
 *  song is as long as the model's own plan and ends where the model ends it. */
export const AUTO_DURATION = 'auto';

export const isAutoDuration = (value: string): boolean => value.trim().toLowerCase() === AUTO_DURATION;

/** The max duration kept for every new song (studio.createDuration): Auto when
 *  nothing is kept yet or Auto was chosen, else the seconds chosen by hand. */
export function keptDurationText(stored: string | null, min: number, max: number): string {
  if (stored === null || isAutoDuration(stored)) return AUTO_DURATION;
  const kept = Number(stored);
  return Number.isFinite(kept) && kept >= min && kept <= max ? String(kept) : AUTO_DURATION;
}

/** What a loaded request (or the assistant's draft) leaves in the form: Auto stays
 *  Auto, since no ceiling is longer than none; seconds follow clampDurationToKept. */
export function durationAfterLoad(asked: number | undefined, kept: string, max: number): string {
  if (isAutoDuration(kept)) return AUTO_DURATION;
  return String(Math.min(max, clampDurationToKept(asked, Number(kept))));
}
