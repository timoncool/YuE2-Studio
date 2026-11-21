import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { DjMode } from './DjMode';

const recorder = vi.hoisted(() => ({ stop: vi.fn() }));
vi.mock('../../services/externalLinks', () => ({ isDesktop: () => true }));
vi.mock('../../services/djMixer', () => ({
  DjMixer: class {
    recording = false;
    decks = [0, 1].map(() => ({
      meter: { frequencyBinCount: 0 },
      state: {
        song: null, time: 0, duration: 0, rate: 1, level: 1,
        eq: [0, 0, 0], filter: 0, echo: 0, hotCues: [], loopIn: 0, loopOut: 0,
      },
    }));
    constructor(private changed: () => void) {}
    getCrossfade() { return 0.5; }
    async startRecording() { this.recording = true; this.changed(); }
    async stopRecording() { this.recording = false; this.changed(); return recorder.stop(); }
    dispose() {}
  },
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('holds the original recording through cancellation and a failed write, then saves it on retry', async () => {
  const blob = new Blob(['recorded mix'], { type: 'audio/webm;codecs=opus' });
  recorder.stop.mockClear();
  recorder.stop.mockReturnValue(blob);
  const choose = vi.fn()
    .mockResolvedValueOnce(Response.json({ cancelled: true }))
    .mockResolvedValueOnce(Response.json({ id: 'first', path: 'D:/first.webm' }))
    .mockResolvedValueOnce(Response.json({ id: 'retry', path: 'D:/retry.webm' }));
  const write = vi.fn()
    .mockResolvedValueOnce(Response.json({ error: 'Disk is full' }, { status: 500 }))
    .mockResolvedValueOnce(Response.json({ path: 'D:/retry.webm', written: blob.size }));
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => {
    if (url.endsWith('/v1/files/save')) return choose(init);
    if (url.includes('/v1/files/save/')) return write(url, init.body);
    return Response.json({});
  }));
  const onClose = vi.fn();
  const host = document.createElement('div');
  const root = createRoot(host);
  const button = (label: string) => Array.from(host.querySelectorAll('button'))
    .find(element => element.textContent?.trim() === label);
  const click = async (label: string) => {
    const element = button(label);
    expect(element).toBeDefined();
    await act(async () => element?.click());
  };
  try {
    await act(async () => root.render(<DjMode library={[]} playlists={[]} initialSong={null} onClose={onClose} />));
    await click('Record mix');
    await click('Back to Studio');
    await click('Save and exit');
    expect(recorder.stop).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
    expect(write).not.toHaveBeenCalled();
    await click('Keep mixing');
    await click('Save recording');
    expect(host.querySelector('[role=alert]')?.textContent).toContain('could not be saved');
    expect(onClose).not.toHaveBeenCalled();
    await click('Back to Studio');
    await click('Save and exit');
    expect(recorder.stop).toHaveBeenCalledTimes(1);
    expect(write).toHaveBeenCalledTimes(2);
    expect(write.mock.calls[0][1]).toBe(blob);
    expect(write.mock.calls[1][1]).toBe(blob);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(button('Record mix')).toBeDefined();
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});

it.each([
  { action: 'Back to Studio', fail: false },
  { action: 'Escape', fail: true },
])('lets $action discard after a cancelled or failed save', async ({ action, fail }) => {
  const blob = new Blob(['pending mix'], { type: 'audio/webm;codecs=opus' });
  recorder.stop.mockClear();
  recorder.stop.mockReturnValue(blob);
  const choose = vi.fn(async () => fail
    ? Response.json({ id: 'busy', path: 'D:/pending.webm' })
    : Response.json({ cancelled: true }));
  const write = vi.fn(async () => Response.json({ error: 'Disk is full' }, { status: 500 }));
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    if (url.endsWith('/v1/files/save')) return choose();
    if (url.includes('/v1/files/save/')) return write();
    return Response.json({});
  }));
  const onClose = vi.fn();
  const host = document.createElement('div');
  const root = createRoot(host);
  const click = async (label: string) => {
    const target = [...host.querySelectorAll('button')].find(button => button.textContent?.trim() === label);
    expect(target).toBeDefined();
    await act(async () => target?.click());
  };
  const escape = async () => {
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })));
  };
  const requestExit = () => action === 'Escape' ? escape() : click(action);
  try {
    await act(async () => root.render(<DjMode library={[]} playlists={[]} initialSong={null} onClose={onClose} />));
    await click('Record mix');
    await click('Stop & save recording');
    await requestExit();
    expect(host.querySelector('[role="alertdialog"]')).not.toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    await escape();
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    await requestExit();
    await click('Discard and exit');
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(choose).toHaveBeenCalledTimes(1);
    expect(write).toHaveBeenCalledTimes(fail ? 1 : 0);
    expect(recorder.stop).toHaveBeenCalledTimes(1);
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
