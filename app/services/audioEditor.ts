/**
 * AudioMass, the studio's audio editor.
 *
 * The editor is a page of static files the local service already serves, and
 * it reaches the studio's media through the same address. Inside the desktop
 * shell it belongs in a window of the studio, like the visualiser; in a plain
 * browser, which has no such window to make, it keeps opening in a tab.
 */

import { invoke } from '@tauri-apps/api/core';
import { apiUrl } from './apiBase';
import { isDesktop, openExternal } from './externalLinks';

/** Opens the editor on one audio file, given as an absolute URL. */
export async function openAudioEditor(audioUrl: string): Promise<void> {
  const url = apiUrl(`/editor/index.html?audioUrl=${encodeURIComponent(audioUrl)}`);
  if (isDesktop()) {
    try {
      await invoke('open_audio_editor_window', { url });
      return;
    } catch (error) {
      // A shell too old for the command still edits the file, just in a tab.
      console.error('The audio editor window did not open:', error);
    }
  }
  await openExternal(url);
}
