import { Song } from '../types';

/**
 * Opens a track on the tools page, ready to be separated.
 *
 * The menu entry used to open a dialog of its own, which meant two places
 * showed the same thing and only one of them had the settings. Now it asks the
 * application to switch pages, and the tools page picks the track up.
 */
export function openStems(song: Song): void {
  window.dispatchEvent(new CustomEvent('yue:open-stems', { detail: { songId: song.id, tool: 'stems' } }));
}

/** Opens a track in the tools page's Audio to MIDI. */
export function openMidi(song: Song): void {
  window.dispatchEvent(new CustomEvent('yue:open-stems', { detail: { songId: song.id, tool: 'midi' } }));
}
