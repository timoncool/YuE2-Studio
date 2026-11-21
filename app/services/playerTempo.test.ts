import { describe, expect, it, vi } from 'vitest';
import { resetLibraryTempoForNewTrack } from './playerTempo';

describe('regular library playback tempo on track change', () => {
  it('resets the real audio speed and the UI to 100% together', () => {
    const audio = {
      defaultPlaybackRate: 1.4,
      playbackRate: 1.4,
      preservesPitch: true,
    };
    let shownRate = 1.4;
    const setShownRate = vi.fn((rate: number) => { shownRate = rate; });

    resetLibraryTempoForNewTrack(audio, setShownRate);

    expect(audio.playbackRate).toBe(1);
    expect(audio.defaultPlaybackRate).toBe(1);
    expect(audio.preservesPitch).toBe(false);
    expect(shownRate).toBe(1);
    expect(setShownRate).toHaveBeenCalledTimes(1);
    expect(setShownRate).toHaveBeenCalledWith(1);
  });

  it('also restores a slowed-down track to natural speed', () => {
    const audio = { defaultPlaybackRate: 0.7, playbackRate: 0.7, preservesPitch: false };
    const update = vi.fn();
    resetLibraryTempoForNewTrack(audio, update);
    expect(audio.playbackRate).toBe(1);
    expect(audio.defaultPlaybackRate).toBe(1);
    expect(update).toHaveBeenCalledWith(1);
  });
});
