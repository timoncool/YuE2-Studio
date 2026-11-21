import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NotificationSettings } from './NotificationSettings';

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
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function mount() {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<NotificationSettings />));
  return {
    checkbox: host.querySelector<HTMLInputElement>('input[type="checkbox"]')!,
    test: host.querySelector<HTMLButtonElement>('button')!,
  };
}

describe('song notification settings', () => {
  it('reads and saves the native preference before changing the control', async () => {
    let enabled = true;
    const invoke = vi.fn(async (command: string, args?: { enabled: boolean }) => {
      if (command === 'set_song_notification_preference') enabled = args!.enabled;
      return enabled;
    });
    vi.stubGlobal('__TAURI__', { core: { invoke } });
    const { checkbox, test } = await mount();

    expect(checkbox.checked).toBe(true);
    await act(async () => checkbox.click());
    expect(checkbox.checked).toBe(false);
    expect(test.disabled).toBe(true);
    expect(invoke).toHaveBeenCalledWith('set_song_notification_preference', { enabled: false });

    await act(async () => checkbox.click());
    expect(checkbox.checked).toBe(true);
    expect(test.disabled).toBe(false);
    await act(async () => test.click());
    expect(invoke).toHaveBeenCalledWith('notify_song_ready', { message: 'shellNotificationsTestBody' });
    expect(host?.textContent).toContain('shellNotificationsTestRequested');
  });

  it('retains the saved preference and shows an error when a write fails', async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === 'set_song_notification_preference') throw new Error('write failed');
      return true;
    });
    vi.stubGlobal('__TAURI__', { core: { invoke } });
    const { checkbox, test } = await mount();
    await act(async () => checkbox.click());
    expect(checkbox.checked).toBe(true);
    expect(test.disabled).toBe(false);
    expect(host?.textContent).toContain('shellNotificationsPreferenceError');
  });
});
