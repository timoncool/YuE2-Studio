import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  SONG_NOTIFICATIONS_KEY, sendCompletedSongsNotification, sendWindowsSongNotification,
  setSongNotificationsEnabled, songNotificationsEnabled,
} from './songNotifications';

beforeEach(() => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('Win32');
});

afterEach(() => {
  setSongNotificationsEnabled(true);
  localStorage.removeItem(SONG_NOTIFICATIONS_KEY);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const messages = {
  single: '"{title}" has finished generating.',
  multiple: '{count} tracks are ready, including "{title}".',
  untitled: 'Untitled',
};

describe('native Windows song completion alerts', () => {
  it('defaults on, and the menu toggle persists independently of promotional notices', () => {
    expect(songNotificationsEnabled()).toBe(true);
    setSongNotificationsEnabled(false);
    expect(songNotificationsEnabled()).toBe(false);
    expect(localStorage.getItem(SONG_NOTIFICATIONS_KEY)).toBe('0');
    setSongNotificationsEnabled(true);
    expect(songNotificationsEnabled()).toBe(true);
  });

  it('sends a single completion notification with the actual song title', async () => {
    const invoke = vi.fn(async () => null);
    vi.stubGlobal('__TAURI__', { core: { invoke } });
    expect(await sendCompletedSongsNotification([{ title: 'Lofi Rainy Days' }], messages)).toBe(true);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith('notify_song_ready', { message: '"Lofi Rainy Days" has finished generating.' });
  });

  it('sends just one native alert for an entire batch of songs', async () => {
    const invoke = vi.fn(async () => null);
    vi.stubGlobal('__TAURI__', { core: { invoke } });
    expect(await sendCompletedSongsNotification([{ title: 'Track 1' }, { title: 'Track 2' }], messages)).toBe(true);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith('notify_song_ready', { message: '2 tracks are ready, including "Track 1".' });
  });

  it('does nothing for an empty job or while alerts are disabled', async () => {
    const invoke = vi.fn(async () => null);
    vi.stubGlobal('__TAURI__', { core: { invoke } });
    expect(await sendCompletedSongsNotification([], messages)).toBe(false);
    setSongNotificationsEnabled(false);
    expect(await sendCompletedSongsNotification([{ title: 'Never alert' }], messages)).toBe(false);
    expect(await sendWindowsSongNotification('Test')).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('does not report success outside the desktop shell or if Windows refuses the alert', async () => {
    expect(await sendCompletedSongsNotification([{ title: 'Song' }], messages)).toBe(false);
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.stubGlobal('__TAURI__', { core: { invoke: vi.fn(async () => { throw new Error('Windows notifications blocked'); }) } });
    expect(await sendCompletedSongsNotification([{ title: 'Song' }], messages)).toBe(false);
    expect(warning).toHaveBeenCalled();
  });
});
