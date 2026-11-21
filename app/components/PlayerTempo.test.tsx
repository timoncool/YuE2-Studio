import React, { act, useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { Player } from './Player';
import { loadLibraryTrack, setLibraryPlaybackRate } from '../services/playerTempo';
import type { Song } from '../types';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../context/ResponsiveContext', () => ({ useResponsive: () => ({ isMobile: false }) }));
vi.mock('../services/studioQueries', () => ({ useKaraokeStatus: () => ({ data: undefined }) }));
vi.mock('./AlbumCover', () => ({ SongCover: () => null }));
vi.mock('./SongDropdownMenu', () => ({ SongDropdownMenu: () => null }));
vi.mock('./player/PlayerExtraButtons', () => ({ PlayerExtraButtons: () => null }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('applies presets and slider to the transport, keeps same-track rate, and resets on a different track', async () => {
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const audio = new Audio();
  const load = vi.spyOn(audio, 'load').mockImplementation(() => undefined);
  const song = (id: string) => ({ id, title: id, style: '', audioUrl: `/${id}.wav` }) as Song & { audioUrl: string };
  const first = song('first');
  const second = song('second');
  const noAction = () => undefined;
  function Fixture({ track, playing }: { track: ReturnType<typeof song>; playing: boolean }) {
    const [rate, setRate] = useState(1);
    const ref = useRef(audio);
    const current = useRef<string | null>(null);
    useEffect(() => {
      if (loadLibraryTrack(audio, current.current, track)) {
        current.current = track.id;
        setRate(1);
      }
    }, [track, playing]);
    return <Player currentSong={track} isPlaying={playing} onTogglePlay={noAction}
      currentTime={0} duration={8} onSeek={noAction} onNext={noAction} onPrevious={noAction}
      volume={1} onVolumeChange={noAction} playbackRate={rate} audioRef={ref}
      onPlaybackRateChange={next => { setLibraryPlaybackRate(audio, next); setRate(next); }}
      isShuffle={false} onToggleShuffle={noAction} repeatMode="none" onToggleRepeat={noAction}
      isLiked={false} onToggleLike={noAction} />;
  }
  const trigger = () => host.querySelector<HTMLButtonElement>('[aria-haspopup="dialog"][aria-label="speedPitchTitle"]')!;
  const speedPanel = () => host.querySelector('[role="dialog"][aria-label="speedPitchTitle"]')!;
  const preset = (speed: string) => [...speedPanel().querySelectorAll('button')].find(button => button.textContent === speed)!;
  const change = (value: string) => act(() => {
    const range = speedPanel().querySelector<HTMLInputElement>('input[type="range"]')!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(range, value);
    range.dispatchEvent(new Event('input', { bubbles: true }));
  });
  const render = (track: ReturnType<typeof song>, playing = false) => act(async () => {
    root.render(<Fixture track={track} playing={playing} />);
  });
  try {
    await render(first, true);
    expect(load).toHaveBeenCalledOnce();
    expect(audio.preservesPitch).toBe(false);
    act(() => trigger().click());
    act(() => preset('1.5x').click());
    expect(audio.playbackRate).toBe(1.5);
    expect(speedPanel().textContent).toContain('+7.0 st');

    await render({ ...first, title: 'Renamed first track' });
    await render(first, true);
    expect(load).toHaveBeenCalledOnce();
    expect(trigger().textContent).toBe('1.5x');

    change('0.68');
    expect(audio.playbackRate).toBe(0.68);
    expect(audio.defaultPlaybackRate).toBe(0.68);
    expect(speedPanel().textContent).toContain('-6.7 st');
    await render(second, true);
    expect(load).toHaveBeenCalledTimes(2);
    expect(audio.src).toContain('/second.wav');
    expect(audio.playbackRate).toBe(1);
    expect(audio.defaultPlaybackRate).toBe(1);
    expect(trigger().textContent).toBe('1x');

    act(() => preset('2x').click());
    act(() => preset('speedPitchReset').click());
    expect(audio.playbackRate).toBe(1);
    const range = speedPanel().querySelector<HTMLInputElement>('input[type="range"]')!;
    range.focus();
    act(() => range.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    expect(speedPanel()).toBeNull();
    expect(document.activeElement).toBe(trigger());
  } finally {
    act(() => root.unmount());
    host.remove();
  }
});
