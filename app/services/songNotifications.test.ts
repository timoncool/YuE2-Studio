import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { sendCompletedSongsNotification, sendWindowsSongNotification, songNotificationsEnabled } from './songNotifications';

beforeEach(() => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('Win32');
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const messages = {
  single: '"{title}" has finished generating.',
  multiple: '{count} tracks are ready, including "{title}".',
  untitled: 'Untitled',
};

function nativeDesktop(enabled: boolean, notification: () => Promise<boolean> = async () => true) {
  const invoke = vi.fn(async (command: string, _args?: Record<string, unknown>) =>
    command === 'song_notification_preference' ? enabled : notification());
  vi.stubGlobal('__TAURI__', { core: { invoke } });
  return invoke;
}

describe('native Windows song completion alerts', () => {
  it('sends one alert containing the actual song title', async () => {
    const invoke = nativeDesktop(true);
    expect(await sendCompletedSongsNotification([{ title: 'Lofi Rainy Days' }], messages)).toBe(true);
    expect(invoke.mock.calls).toEqual([
      ['song_notification_preference'],
      ['notify_song_ready', { message: '"Lofi Rainy Days" has finished generating.' }],
    ]);
  });

  it('sends just one native alert for an entire batch of songs', async () => {
    const invoke = nativeDesktop(true);
    expect(await sendCompletedSongsNotification([{ title: 'Track 1' }, { title: 'Track 2' }], messages)).toBe(true);
    expect(invoke.mock.calls).toEqual([
      ['song_notification_preference'],
      ['notify_song_ready', { message: '2 tracks are ready, including "Track 1".' }],
    ]);
  });

  it('skips empty jobs and reads the native opt-out even in a new window', async () => {
    const invoke = nativeDesktop(false);
    expect(await sendCompletedSongsNotification([], messages)).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
    expect(await songNotificationsEnabled()).toBe(false);
    expect(await sendCompletedSongsNotification([{ title: 'Never alert' }], messages)).toBe(false);
    expect(invoke.mock.calls).toEqual([['song_notification_preference'], ['song_notification_preference']]);
  });

  it('honours a native opt-out between reading the preference and sending the notification', async () => {
    nativeDesktop(true, async () => false);
    expect(await sendWindowsSongNotification('Test')).toBe(false);
  });

  it('does not report success outside the desktop shell or if Windows refuses the alert', async () => {
    expect(await sendCompletedSongsNotification([{ title: 'Song' }], messages)).toBe(false);
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
    nativeDesktop(true, async () => { throw new Error('Windows notifications blocked'); });
    expect(await sendCompletedSongsNotification([{ title: 'Song' }], messages)).toBe(false);
    expect(warning).toHaveBeenCalled();
  });
});
