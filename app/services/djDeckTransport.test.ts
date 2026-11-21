import { afterEach, expect, it, vi } from 'vitest';
import type { Song } from '../types';
import { DjMixer } from './djMixer';

const param = () => ({ value: 1, setTargetAtTime(value: number) { this.value = value; } });
class AudioNodeFixture {
  gain = param(); frequency = param(); Q = param(); delayTime = param();
  threshold = param(); knee = param(); ratio = param(); attack = param(); release = param();
  connect<T>(next: T) { return next; }
  disconnect = vi.fn();
}
class DeckAudio extends EventTarget {
  static made: DeckAudio[] = [];
  src = '';
  currentTime = 0;
  duration = 3;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  preservesPitch = true;
  load = vi.fn();
  play = vi.fn(async () => undefined);
  pause = vi.fn();
  removeAttribute(name: string) { if (name === 'src') this.src = ''; }
  constructor() { super(); DeckAudio.made.push(this); }
}
class AudioContextFixture {
  currentTime = 0;
  destination = new AudioNodeFixture();
  resume = vi.fn(async () => undefined);
  close = vi.fn(async () => undefined);
  createMediaElementSource() { return new AudioNodeFixture(); }
  createGain() { return new AudioNodeFixture(); }
  createBiquadFilter() { return new AudioNodeFixture(); }
  createDelay() { return new AudioNodeFixture(); }
  createAnalyser() { return new AudioNodeFixture(); }
  createDynamicsCompressor() { return new AudioNodeFixture(); }
  createMediaStreamDestination() { return new AudioNodeFixture(); }
}

afterEach(() => {
  vi.unstubAllGlobals();
  DeckAudio.made = [];
});

it('plays both decks, keeps their faders independent and falls back if a stem fails', async () => {
  vi.stubGlobal('AudioContext', AudioContextFixture);
  vi.stubGlobal('Audio', DeckAudio);
  vi.stubGlobal('fetch', vi.fn(async () => new Response(null, { status: 404 })));
  const track = (id: string, bpm: number) =>
    ({ id, title: id, audioUrl: `/library/${id}.wav`, createdAt: new Date(0), bpm } as Song);
  const first = track('a', 100);
  const second = track('b', 125);
  const stem = (id: string, role: string, model: string, at: number) => ({
    ...track(id, 100), createdAt: new Date(at),
    derived: { from: 'a', fromTitle: 'a', tool: 'stems', settings: { stem: role, model } },
  });
  const mixer = new DjMixer(() => undefined);
  const [a, b] = mixer.decks;
  try {
    a.load(first, [
      stem('old-bass', 'bass', 'HT-Demucs', 1),
      stem('voice', 'vocals', 'RoFormer', 2), stem('backing', 'instrumental', 'RoFormer', 2),
    ]);
    b.load(second, []);
    expect(a.state.stems.map(channel => channel.id)).toEqual(['voice', 'backing']);
    expect(mixer.sync(0)).toBe(true);
    expect(a.state.rate).toBe(1.25);
    expect(b.state.rate).toBe(1);
    expect(DeckAudio.made[0].preservesPitch).toBe(false);
    await Promise.all([a.play(), b.play()]);
    expect(a.state.playing && b.state.playing).toBe(true);
    mixer.setCrossfade(0);
    expect(a.output.gain.value).toBeCloseTo(1);
    expect(b.output.gain.value).toBeCloseTo(0);
    mixer.gain(1, 0.4);
    mixer.setCrossfade(1);
    expect(a.output.gain.value).toBeCloseTo(0);
    expect(b.output.gain.value).toBeCloseTo(0.4);

    expect(a.stemPreset('vocals')).toBe(true);
    expect(a.state.stems.find(channel => channel.id === 'backing')?.mute).toBe(true);
    await Promise.resolve();
    DeckAudio.made.find(audio => audio.src.endsWith('/backing.wav'))!.play.mockRejectedValueOnce(new Error('bad stem'));
    a.setStemMode(true);
    await vi.waitFor(() => expect(a.state.stemMode).toBe(false));
    expect(a.state.error).toBe('bad stem');
    expect(a.state.playing && b.state.playing).toBe(true);

    a.load(second, []);
    expect(a.state.rate).toBe(1);
    expect(a.state.stems).toEqual([]);
    expect(b.state.playing).toBe(true);
  } finally {
    mixer.dispose();
  }
  expect(mixer.context.close).toHaveBeenCalledOnce();
});
