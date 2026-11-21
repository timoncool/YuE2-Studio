import { afterEach, describe, it, expect, vi } from 'vitest';
import { DjMixer, crossfadeGains, rateForSync, songBpm } from './djMixer';
import type { Song } from '../types';

describe('DJ mixing math', () => {
  it('keeps both decks audible at center and uses equal power through the crossfader', () => {
    expect(crossfadeGains(0)).toEqual([1, 0]);
    const [a, b] = crossfadeGains(0.5);
    expect(a).toBeCloseTo(Math.SQRT1_2);
    expect(b).toBeCloseTo(Math.SQRT1_2);
    expect(a * a + b * b).toBeCloseTo(1);
    expect(crossfadeGains(1)[0]).toBeCloseTo(0);
  });

  it('matches deck speed to the other deck BPM and tempo', () => {
    expect(rateForSync(100, 125, 1)).toBe(1.25);
    expect(rateForSync(140, 105, 1)).toBe(0.75);
    expect(rateForSync(60, 180, 1)).toBe(2);
  });

  it('reads BPM metadata and song style as a fallback', () => {
    expect(songBpm(null)).toBeNull();
    expect(songBpm({ bpm: 135, style: 'piano pop' } as Song)).toBe(135);
    expect(songBpm({ style: 'piano pop, 112 BPM' } as Song)).toBe(112);
    expect(songBpm({ style: 'indie pop' } as Song)).toBeNull();
  });
});

class Parameter {
  value = 1;
  setTargetAtTime(value: number) { this.value = value; }
  setValueAtTime() {}
  linearRampToValueAtTime() {}
}

class GraphNode {
  connections: GraphNode[] = [];
  gain = new Parameter();
  frequency = new Parameter();
  Q = new Parameter();
  delayTime = new Parameter();
  threshold = new Parameter();
  knee = new Parameter();
  ratio = new Parameter();
  attack = new Parameter();
  release = new Parameter();
  buffer?: ReturnType<GraphContext['createBuffer']>;
  connect(destination: GraphNode) { this.connections.push(destination); return destination; }
  disconnect() { this.connections = []; }
  start() {}
  stop() {}
}

class GraphContext {
  currentTime = 0;
  destination = new GraphNode();
  recording = new GraphNode();
  grains: GraphNode[] = [];
  media: FakeAudio[] = [];
  createGain() { return new GraphNode(); }
  createDynamicsCompressor() { return new GraphNode(); }
  createMediaStreamDestination() { return this.recording; }
  createBiquadFilter() { return new GraphNode(); }
  createDelay() { return new GraphNode(); }
  createAnalyser() { return new GraphNode(); }
  createMediaElementSource(audio: FakeAudio) { this.media.push(audio); return new GraphNode(); }
  createBufferSource() { const grain = new GraphNode(); this.grains.push(grain); return grain; }
  createBuffer(numberOfChannels: number, length: number, sampleRate: number) {
    const channels = Array.from({ length: numberOfChannels }, () => new Float32Array(length));
    return { numberOfChannels, length, sampleRate, getChannelData: (channel: number) => channels[channel] };
  }
  async decodeAudioData(raw: ArrayBuffer) {
    const decoded = this.createBuffer(1, 4000, 1000);
    decoded.getChannelData(0).fill(new Uint8Array(raw)[0]);
    return decoded;
  }
  async resume() {}
}

class FakeAudio extends EventTarget {
  src = '';
  duration = 10;
  currentTime = 0;
  paused = true;
  load() {}
  pause() { this.paused = true; }
  async play() { this.paused = false; }
  removeAttribute() { this.src = ''; }
}

function mixer() {
  vi.stubGlobal('Audio', FakeAudio);
  vi.stubGlobal('AudioContext', GraphContext);
  vi.stubGlobal('fetch', async (url: string) => ({
    ok: true, arrayBuffer: async () => Uint8Array.of(url.endsWith('/vocals') ? 2 : url.endsWith('/drums') ? 3 : 1).buffer,
  }));
  const mix = new DjMixer(() => {});
  const graph = mix.context as unknown as GraphContext;
  const deck = mix.decks[0];
  deck.load({ id: 'song', audioUrl: '/mix' } as Song, []);
  return { deck, graph, media: graph.media[0] };
}

afterEach(() => vi.unstubAllGlobals());

it('refuses a too-short tail and loops valid points at the end of the track', async () => {
  const { deck, media } = mixer();
  media.dispatchEvent(new Event('loadedmetadata'));
  deck.seek(9.9);
  deck.setLoopIn();
  deck.setLoopOut();
  deck.toggleLoop();
  expect([deck.state.loopIn, deck.state.loopOut, deck.state.looping]).toEqual([9.9, 10, false]);
  deck.seek(9.5);
  deck.setLoopIn();
  deck.seek(10);
  deck.setLoopOut();
  await deck.play();
  media.dispatchEvent(new Event('ended'));
  expect(deck.state.looping).toBe(true);
  expect(media.currentTime).toBe(9.5);
});

it('routes original and stem scratch grains through the audible and recording mix', async () => {
  const { deck, graph } = mixer();
  const reaches = (node: GraphNode, output: GraphNode, visited = new Set<GraphNode>()): boolean => {
    if (node === output) return true;
    if (visited.has(node) || !node.gain.value) return false;
    visited.add(node);
    return node.connections.some(next => reaches(next, output, visited));
  };
  await deck.prepareScratch();
  deck.startScratch();
  deck.scratch(0.2);
  const [original] = graph.grains;
  expect(original.buffer?.getChannelData(0)[0]).toBe(1);
  expect(reaches(original, graph.destination)).toBe(true);
  expect(reaches(original, graph.recording)).toBe(true);

  deck.attachNativeStems('song', ['vocals', 'drums']);
  deck.stemPreset('vocals');
  expect(reaches(original, graph.recording)).toBe(false);
  await deck.prepareScratch();
  graph.currentTime = 1;
  deck.scratch(-0.1);
  const [, vocals, drums] = graph.grains;
  expect(vocals.buffer?.getChannelData(0)[0]).toBe(2);
  expect(drums.buffer?.getChannelData(0)[0]).toBe(3);
  expect(reaches(vocals, graph.recording)).toBe(true);
  expect(reaches(drums, graph.recording)).toBe(false);
  deck.stemPreset('instrumental');
  expect(reaches(vocals, graph.recording)).toBe(false);
  expect(reaches(drums, graph.recording)).toBe(true);
  deck.stemPreset('all');
  deck.updateStem('song:drums', { solo: true, level: 0.5 });
  expect(reaches(vocals, graph.destination)).toBe(false);
  expect(reaches(vocals, graph.recording)).toBe(false);
  expect(reaches(drums, graph.destination)).toBe(true);
  expect(reaches(drums, graph.recording)).toBe(true);
  deck.updateStem('song:drums', { mute: true });
  expect(reaches(drums, graph.recording)).toBe(false);
});
