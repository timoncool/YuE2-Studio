import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NotificationSettings } from './NotificationSettings';
import { SONG_NOTIFICATIONS_KEY, setSongNotificationsEnabled } from '../services/songNotifications';

vi.mock('../context/I18nContext', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let host: HTMLDivElement | null = null;

beforeEach(() => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('Win32');
});

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  setSongNotificationsEnabled(true);
  localStorage.removeItem(SONG_NOTIFICATIONS_KEY);
  vi.unstubAllGlobals();
});

describe('song notification settings', () => {
  it('turns Windows notifications off and back on and provides a test alert', async () => {
    const invoke = vi.fn(async () => undefined);
    vi.stubGlobal('__TAURI__', { core: { invoke } });
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    act(() => root?.render(<NotificationSettings />));

    const checkbox = host.querySelector<HTMLInputElement>('input[type="checkbox"]');
    const test = Array.from(host.querySelectorAll('button')).find(button => button.textContent?.includes('shellNotificationsTest'));
    expect(checkbox?.checked).toBe(true);
    expect(test).not.toBeUndefined();

    act(() => checkbox?.click());
    expect(checkbox?.checked).toBe(false);
    expect(test?.disabled).toBe(true);
    expect(localStorage.getItem(SONG_NOTIFICATIONS_KEY)).toBe('0');

    act(() => checkbox?.click());
    expect(checkbox?.checked).toBe(true);
    expect(test?.disabled).toBe(false);

    await act(async () => { test?.click(); });
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith('notify_song_ready', { message: 'shellNotificationsTestBody' });
    expect(host.textContent).toContain('shellNotificationsTestRequested');
  });
});
