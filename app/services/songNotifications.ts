import { useEffect, useState } from 'react';

export const SONG_NOTIFICATIONS_KEY = 'studio.notifications.songReady';
const CHANGED = 'studio:song-notifications-changed';
let sessionPreference: boolean | undefined;

/** A separate, opt-out setting. Existing in-app toasts are unaffected. */
export function songNotificationsEnabled(): boolean {
  try {
    const value = window.localStorage.getItem(SONG_NOTIFICATIONS_KEY);
    return value === null ? (sessionPreference ?? true) : value !== '0';
  } catch {
    return sessionPreference ?? true;
  }
}

export function setSongNotificationsEnabled(enabled: boolean): void {
  sessionPreference = enabled;
  try { window.localStorage.setItem(SONG_NOTIFICATIONS_KEY, enabled ? '1' : '0'); }
  catch { /* Keep the choice for this session if local storage is unavailable. */ }
  window.dispatchEvent(new Event(CHANGED));
}

export function useSongNotifications(): [boolean, (enabled: boolean) => void] {
  const [enabled, setEnabled] = useState(songNotificationsEnabled);
  useEffect(() => {
    const changed = () => setEnabled(songNotificationsEnabled());
    window.addEventListener(CHANGED, changed);
    window.addEventListener('storage', changed);
    return () => {
      window.removeEventListener(CHANGED, changed);
      window.removeEventListener('storage', changed);
    };
  }, []);
  return [enabled, setSongNotificationsEnabled];
}

export function supportsWindowsSongNotifications(): boolean {
  return /^Win/.test(navigator.platform) && Boolean(desktopInvoke());
}

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
function desktopInvoke(): Invoke | undefined {
  return (window as unknown as { __TAURI__?: { core?: { invoke?: Invoke } } }).__TAURI__?.core?.invoke;
}

/** A real Windows notification, even from a portable build with the window
 * minimised/hidden. The app's regular success toast is still displayed.
 * Errors never interrupt song completion or trigger another notification. */
export async function sendWindowsSongNotification(message: string): Promise<boolean> {
  if (!supportsWindowsSongNotifications() || !songNotificationsEnabled() || !message.trim()) return false;
  const invoke = desktopInvoke();
  if (!invoke) return false;
  try {
    await invoke('notify_song_ready', { message });
    return true;
  } catch (error) {
    console.warn('[song notifications] native notification unavailable:', error);
    return false;
  }
}

export interface CompletionMessages {
  single: string;
  multiple: string;
  untitled: string;
}

/** Only called when a generation job has *completed* with at least one song.
 * Batch jobs produce one native notification, not one per track or poll.
 */
export async function sendCompletedSongsNotification(
  songs: ReadonlyArray<{ title?: string }>,
  text: CompletionMessages,
): Promise<boolean> {
  if (songs.length === 0) return false;
  const title = songs[0].title?.trim() || text.untitled;
  const template = songs.length === 1 ? text.single : text.multiple;
  return sendWindowsSongNotification(template.replace(/\{(title|count)\}/g, (_, field) => field === 'title' ? title : String(songs.length)));
}
