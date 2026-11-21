/** The regular library player has one shared speed control across many tracks.
 * Loading a different file restores the HTML audio element's natural speed;
 * keep the React control in sync rather than leaving the previous percentage.
 */
export function resetLibraryTempoForNewTrack(
  audio: Pick<HTMLAudioElement, 'defaultPlaybackRate' | 'playbackRate' | 'preservesPitch'>,
  updateRate: (rate: number) => void,
): void {
  // Set both rates: browsers can restore defaultPlaybackRate during load().
  audio.defaultPlaybackRate = 1;
  audio.playbackRate = 1;
  // Normal rate and any subsequent speed adjustment use the same tape-style
  // pitch behavior as the existing regular player.
  audio.preservesPitch = false;
  updateRate(1);
}
