export function supportsWindowsSongNotifications(): boolean {
  return /^Win/.test(navigator.platform) && Boolean(desktopInvoke());
}

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
function desktopInvoke(): Invoke | undefined {
  return (window as unknown as { __TAURI__?: { core?: { invoke?: Invoke } } }).__TAURI__?.core?.invoke;
}

export async function songNotificationsEnabled(): Promise<boolean> {
  return await desktopInvoke()?.('song_notification_preference') === true;
}

export async function setSongNotificationsEnabled(enabled: boolean): Promise<boolean> {
  const invoke = desktopInvoke();
  if (!invoke) throw new Error('The native notification preference is unavailable');
  return await invoke('set_song_notification_preference', { enabled }) === true;
}

/** A real Windows notification, even from a portable build with the window
 * minimised/hidden. The app's regular success toast is still displayed.
 * Errors never interrupt song completion or trigger another notification. */
export async function sendWindowsSongNotification(message: string): Promise<boolean> {
  if (!supportsWindowsSongNotifications() || !message.trim()) return false;
  const invoke = desktopInvoke();
  if (!invoke) return false;
  try {
    if (!await songNotificationsEnabled()) return false;
    return await invoke('notify_song_ready', { message }) === true;
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
