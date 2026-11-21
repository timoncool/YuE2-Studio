import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { AudioCompare } from './AudioCompare';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('./AudioWaveform', () => ({
  AudioWaveform: ({ url, duration, currentTime }: { url: string; duration: number; currentTime: number }) => (
    <output>{url} {currentTime}/{duration}</output>
  ),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it.each([[30, 50], [50, 30]])('plays the full active take with %ss and %ss sources', async (aLength, bLength) => {
  const host = document.createElement('div');
  const root = createRoot(host);
  const button = (name: string) => Array.from(host.querySelectorAll('button'))
    .find(element => element.textContent === name || element.getAttribute('aria-label') === name)!;
  const click = (name: string) => act(async () => button(name).click());
  const shortSide = aLength < bLength ? 'A' : 'B';
  const longSide = aLength < bLength ? 'B' : 'A';
  try {
    await act(async () => root.render(<AudioCompare original="a.wav" processed="b.wav" labels={['A', 'B']} />));
    const [a, b] = Array.from(host.querySelectorAll('audio'));
    const [short, long] = aLength < bLength ? [a, b] : [b, a];
    const pauseLong = vi.spyOn(long, 'pause');
    for (const [audio, length] of [[a, aLength], [b, bLength]] as const) {
      Object.defineProperties(audio, { duration: { value: length }, readyState: { value: 1 } });
      vi.spyOn(audio, 'play').mockResolvedValue();
      await act(async () => audio.dispatchEvent(new Event('loadedmetadata')));
    }
    await click(longSide);
    expect(host.querySelector('output')?.textContent).toBe(`${long.getAttribute('src')} 0/50`);
    await click('play');
    short.currentTime = 30;
    long.currentTime = 40;
    await act(async () => short.dispatchEvent(new Event('ended')));
    expect(pauseLong).not.toHaveBeenCalled();
    expect(button('pause')).toBeDefined();
    await click(shortSide);
    expect(host.querySelector('output')?.textContent).toBe(`${short.getAttribute('src')} 30/30`);
    expect(short.currentTime).toBe(30);
    expect(pauseLong).toHaveBeenCalledOnce();
    expect(button('play')).toBeDefined();
    await click('play');
    expect(short.currentTime).toBe(0);
    expect(long.currentTime).toBe(0);
    await act(async () => short.dispatchEvent(new Event('ended')));
    expect(button('play')).toBeDefined();
  } finally {
    act(() => root.unmount());
  }
});
