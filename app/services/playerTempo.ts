/** Apply one rate to the current library transport and to subsequent media loads. */
export function setLibraryPlaybackRate(
  audio: Pick<HTMLAudioElement, 'defaultPlaybackRate' | 'playbackRate' | 'preservesPitch'>,
  rate: number,
): void {
  audio.defaultPlaybackRate = rate;
  audio.playbackRate = rate;
  // Faster audio raises pitch, and slower audio lowers it.
  audio.preservesPitch = false;
}


/** A pause or a metadata refresh keeps the loaded track's chosen rate. */
export function loadLibraryTrack(
  audio: HTMLAudioElement,
  previousId: string | null,
  track: { id: string; audioUrl: string },
): boolean {
  if (previousId === track.id) return false;
  setLibraryPlaybackRate(audio, 1);
  audio.src = track.audioUrl;
  audio.load();
  return true;
}
