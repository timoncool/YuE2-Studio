import type { Song } from '../types';
import { stemOf } from './songStems';

/** Web Audio mixing uses equal-power crossfades so the middle does not fall silent. */
export function crossfadeGains(value: number): [number, number] {
  const x = Math.min(1, Math.max(0, value));
  return [Math.cos(x * Math.PI / 2), Math.sin(x * Math.PI / 2)];
}
export function rateForSync(sourceBpm: number, targetBpm: number, targetRate: number): number {
  return Math.max(0.25, Math.min(2, targetBpm * targetRate / sourceBpm));
}
export function songBpm(song: Song | null): number | null {
  if (!song) return null;
  if (song.bpm && Number.isFinite(song.bpm) && song.bpm > 0) return song.bpm;
  const match = /\b(\d{2,3}(?:\.\d+)?)\s*bpm\b/i.exec(song.style ?? '');
  return match ? Number(match[1]) : null;
}

export interface DjStem {
  id: string;
  label: string;
  level: number;
  mute: boolean;
  solo: boolean;
}
export interface DjDeckSnapshot {
  song: Song | null;
  playing: boolean;
  time: number;
  duration: number;
  rate: number;
  level: number;
  filter: number;
  echo: number;
  eq: [number, number, number];
  cue: number;
  hotCues: Array<number | null>;
  loopIn: number;
  loopOut: number;
  looping: boolean;
  stemMode: boolean;
  stems: DjStem[];
  scratchReady: boolean;
  scratching: boolean;
  bpm: number | null;
  error: string | null;
}
interface StemVoice {
  song: Song;
  audio: HTMLAudioElement;
  source: MediaElementAudioSourceNode;
  gain: GainNode;
  state: DjStem;
}
const MAX_RATE = 2;
const clamped = (value: number, min: number, max: number) => Math.max(min, Math.min(max, Number.isFinite(value) ? value : min));

/**
 * Each deck has its own audio elements, EQ, filter, cue points, loops, stem mixer and
 * short sample-grain scratch playback. Both feed the same AudioContext/master bus.
 * The studio's own player is paused on entering DJ mode: no competing sound sources.
 */
export class DjDeck {
  private readonly ctx: AudioContext;
  private readonly input: GainNode;
  private readonly mainGain: GainNode;
  private readonly mainSource: MediaElementAudioSourceNode;
  private readonly eqNodes: [BiquadFilterNode, BiquadFilterNode, BiquadFilterNode];
  private readonly filterNode: BiquadFilterNode;
  private readonly delay: DelayNode;
  private readonly feedback: GainNode;
  private readonly wet: GainNode;
  readonly output: GainNode;
  readonly meter: AnalyserNode;
  private readonly audio: HTMLAudioElement;
  private stemVoices: StemVoice[] = [];
  private readonly changed: () => void;
  private scratchBuffer: AudioBuffer | null = null;
  private scratchRequest: AbortController | null = null;
  private scratchPromise: Promise<void> | null = null;
  private scratchPrevious = 0;
  private scratchSources: AudioBufferSourceNode[] = [];
  private disposed = false;
  private playingRequested = false;
  private loadingId = 0;

  state: DjDeckSnapshot = {
    song: null, playing: false, time: 0, duration: 0, rate: 1, level: 1, filter: 0,
    echo: 0, eq: [0, 0, 0], cue: 0, hotCues: [null, null, null, null],
    loopIn: 0, loopOut: 0, looping: false, stemMode: false, stems: [],
    scratchReady: false, scratching: false, bpm: null, error: null,
  };

  constructor(ctx: AudioContext, destination: AudioNode, changed: () => void) {
    this.ctx = ctx;
    this.changed = changed;
    this.audio = new Audio();
    this.audio.preload = 'metadata';
    this.audio.crossOrigin = 'anonymous';
    this.audio.preservesPitch = false;
    this.mainSource = ctx.createMediaElementSource(this.audio);
    this.input = ctx.createGain();
    this.mainGain = ctx.createGain();
    this.mainSource.connect(this.mainGain).connect(this.input);
    this.eqNodes = [ctx.createBiquadFilter(), ctx.createBiquadFilter(), ctx.createBiquadFilter()];
    this.eqNodes[0].type = 'lowshelf';
    this.eqNodes[0].frequency.value = 240;
    this.eqNodes[1].type = 'peaking';
    this.eqNodes[1].frequency.value = 1300;
    this.eqNodes[1].Q.value = 0.7;
    this.eqNodes[2].type = 'highshelf';
    this.eqNodes[2].frequency.value = 5200;
    this.filterNode = ctx.createBiquadFilter();
    this.filterNode.type = 'allpass';
    this.delay = ctx.createDelay(2);
    this.delay.delayTime.value = 0.28;
    this.feedback = ctx.createGain();
    this.feedback.gain.value = 0.32;
    this.wet = ctx.createGain();
    this.wet.gain.value = 0;
    this.output = ctx.createGain();
    this.meter = ctx.createAnalyser();
    this.meter.fftSize = 512;
    this.input.connect(this.eqNodes[0]);
    this.eqNodes[0].connect(this.eqNodes[1]);
    this.eqNodes[1].connect(this.eqNodes[2]);
    this.eqNodes[2].connect(this.filterNode);
    this.filterNode.connect(this.output);
    this.filterNode.connect(this.delay);
    this.delay.connect(this.feedback).connect(this.delay);
    this.delay.connect(this.wet).connect(this.output);
    this.output.connect(this.meter).connect(destination);
    this.audio.addEventListener('loadedmetadata', () => {
      if (this.disposed) return;
      this.state.duration = Number.isFinite(this.audio.duration) ? this.audio.duration : 0;
      if (!this.state.loopOut) this.state.loopOut = this.state.duration;
      this.changed();
    });
    this.audio.addEventListener('timeupdate', () => this.updateTime());
    this.audio.addEventListener('ended', () => {
      this.playingRequested = false;
      this.state.playing = false;
      this.stopStems();
      this.changed();
    });
    this.audio.addEventListener('error', () => {
      if (this.disposed) return;
      this.state.error = this.audio.error?.message || 'Unable to play this audio file.';
      this.changed();
    });
  }

  private updateTime(): void {
    this.state.time = this.audio.currentTime || 0;
    if (this.state.looping && this.state.loopOut > this.state.loopIn && this.state.time >= this.state.loopOut) {
      this.seek(this.state.loopIn);
    }
    // HTMLMediaElement clocks can drift across multiple stem files: align to the main clock.
    if (this.state.stemMode) for (const voice of this.stemVoices) {
      if (Math.abs(voice.audio.currentTime - this.audio.currentTime) > 0.045) voice.audio.currentTime = this.audio.currentTime;
    }
    this.changed();
  }

  load(song: Song, library: Song[]): void {
    if (!song.audioUrl) return;
    this.loadingId++;
    this.scratchRequest?.abort();
    this.scratchRequest = null;
    this.scratchPromise = null;
    this.scratchBuffer = null;
    this.playingRequested = false;
    this.audio.pause();
    this.stopStems();
    for (const voice of this.stemVoices) {
      voice.audio.pause();
      voice.audio.removeAttribute('src');
      voice.audio.load();
      voice.source.disconnect();
      voice.gain.disconnect();
    }
    this.stemVoices = [];
    this.audio.src = song.audioUrl;
    this.audio.load();
    this.audio.playbackRate = 1;
    this.audio.preservesPitch = false;
    this.mainGain.gain.value = 1;
    this.state = {
      song, playing: false, time: 0, duration: 0, rate: 1, level: this.state.level,
      filter: this.state.filter, echo: this.state.echo, eq: [this.state.eq[0], this.state.eq[1], this.state.eq[2]],
      cue: 0, hotCues: [null, null, null, null], loopIn: 0, loopOut: 0, looping: false,
      stemMode: false, stems: [], scratchReady: false, scratching: false,
      bpm: songBpm(song), error: null,
    };
    for (const stem of library.filter(candidate => candidate.audioUrl && candidate.derived?.from === song.id && stemOf(candidate) !== null)) {
      const audio = new Audio();
      audio.src = stem.audioUrl!;
      audio.crossOrigin = 'anonymous';
      audio.preload = 'metadata';
      audio.preservesPitch = false;
      const source = this.ctx.createMediaElementSource(audio);
      const gain = this.ctx.createGain();
      gain.gain.value = 0;
      source.connect(gain).connect(this.input);
      const state: DjStem = { id: stem.id, label: stemOf(stem) ?? 'other', level: 1, mute: false, solo: false };
      this.stemVoices.push({ song: stem, audio, source, gain, state });
    }
    this.state.stems = this.stemVoices.map(voice => ({ ...voice.state }));
    this.changed();
    // Prepare scratch audio in the background as soon as the track is loaded.
    // Playback starts independently; the platter becomes scratch-ready when decoding finishes.
    void this.prepareScratch();
  }

  async play(): Promise<void> {
    if (!this.state.song) return;
    this.state.error = null;
    this.playingRequested = true;
    try {
      await this.ctx.resume();
      await this.audio.play();
      if (this.state.stemMode) {
        this.syncStemClock();
        await Promise.all(this.stemVoices.map(voice => voice.audio.play()));
      }
      this.state.playing = true;
    } catch (error) {
      this.playingRequested = false;
      this.audio.pause();
      this.stopStems();
      this.state.playing = false;
      this.state.error = error instanceof Error ? error.message : String(error);
    }
    this.changed();
  }

  pause(): void {
    this.playingRequested = false;
    this.audio.pause();
    this.stopStems();
    this.state.playing = false;
    this.changed();
  }

  toggle(): void {
    if (this.state.playing) this.pause();
    else void this.play();
  }

  seek(seconds: number): void {
    if (!this.state.song) return;
    const bound = this.state.duration || Number.MAX_SAFE_INTEGER;
    const value = clamped(seconds, 0, bound);
    this.audio.currentTime = value;
    this.state.time = value;
    this.syncStemClock();
    this.changed();
  }
  private syncStemClock(): void {
    for (const voice of this.stemVoices) {
      voice.audio.currentTime = this.audio.currentTime;
      voice.audio.playbackRate = this.state.rate;
    }
  }
  private stopStems(): void {
    for (const voice of this.stemVoices) voice.audio.pause();
  }

  rate(rate: number): void {
    const next = clamped(rate, 0.25, MAX_RATE);
    this.state.rate = next;
    this.audio.playbackRate = next;
    this.audio.preservesPitch = false;
    for (const voice of this.stemVoices) {
      voice.audio.playbackRate = next;
      voice.audio.preservesPitch = false;
    }
    this.changed();
  }

  gain(level: number): void { this.state.level = clamped(level, 0, 1.5); this.changed(); }
  equalizer(band: number, db: number): void {
    if (band < 0 || band > 2) return;
    this.state.eq[band] = clamped(db, -18, 12);
    this.eqNodes[band].gain.setTargetAtTime(this.state.eq[band], this.ctx.currentTime, 0.015);
    this.changed();
  }
  filter(amount: number): void {
    const x = clamped(amount, -1, 1);
    this.state.filter = x;
    if (Math.abs(x) < 0.01) this.filterNode.type = 'allpass';
    else if (x < 0) {
      this.filterNode.type = 'lowpass';
      this.filterNode.frequency.setTargetAtTime(20000 * Math.pow(120 / 20000, -x), this.ctx.currentTime, 0.015);
    } else {
      this.filterNode.type = 'highpass';
      this.filterNode.frequency.setTargetAtTime(20 * Math.pow(8000 / 20, x), this.ctx.currentTime, 0.015);
    }
    this.changed();
  }
  echo(amount: number): void {
    this.state.echo = clamped(amount, 0, 1);
    this.wet.gain.setTargetAtTime(this.state.echo * 0.7, this.ctx.currentTime, 0.015);
    this.changed();
  }

  setStemMode(enabled: boolean): void {
    if (!this.stemVoices.length) return;
    this.state.stemMode = enabled;
    this.mainGain.gain.setTargetAtTime(enabled ? 0 : 1, this.ctx.currentTime, 0.02);
    this.applyStemLevels();
    if (enabled && this.playingRequested) {
      this.syncStemClock();
      void Promise.all(this.stemVoices.map(voice => voice.audio.play())).catch(error => {
        this.state.error = error instanceof Error ? error.message : String(error);
        this.changed();
      });
    } else this.stopStems();
    this.changed();
  }

  updateStem(id: string, change: Partial<Pick<DjStem, 'level' | 'mute' | 'solo'>>): void {
    const voice = this.stemVoices.find(entry => entry.state.id === id);
    if (!voice) return;
    voice.state = { ...voice.state, ...change, level: clamped(change.level ?? voice.state.level, 0, 1.5) };
    this.state.stems = this.stemVoices.map(entry => ({ ...entry.state }));
    this.applyStemLevels();
    this.changed();
  }

  private applyStemLevels(): void {
    const solos = this.stemVoices.some(voice => voice.state.solo);
    for (const voice of this.stemVoices) {
      const state = voice.state;
      const level = this.state.stemMode && !state.mute && (!solos || state.solo) ? state.level : 0;
      voice.gain.gain.setTargetAtTime(level, this.ctx.currentTime, 0.02);
    }
  }

  setCue(): void { this.state.cue = this.state.time; this.changed(); }
  cue(): void { this.pause(); this.seek(this.state.cue); }
  setHotCue(index: number): void {
    if (index < 0 || index >= this.state.hotCues.length) return;
    this.state.hotCues[index] = this.state.time;
    this.changed();
  }
  hotCue(index: number): void {
    const time = this.state.hotCues[index];
    if (time !== null && time !== undefined) this.seek(time);
  }
  setLoopIn(): void {
    this.state.loopIn = this.state.time;
    if (this.state.loopOut <= this.state.loopIn) this.state.loopOut = Math.min(this.state.duration, this.state.loopIn + 4);
    this.changed();
  }
  setLoopOut(): void {
    this.state.loopOut = Math.max(this.state.loopIn + 0.25, this.state.time);
    this.state.looping = true;
    this.changed();
  }
  toggleLoop(): void {
    if (this.state.loopOut <= this.state.loopIn) return;
    this.state.looping = !this.state.looping;
    this.changed();
  }

  /** The pitch bend wheel has no artificial sound-sample length limit.
   * The track is decoded on first scratch, and small forward/reverse grains
   * are played at the actual pointer position while its transport is paused.
   */
  async prepareScratch(): Promise<void> {
    const url = this.state.song?.audioUrl;
    if (!url || this.scratchBuffer || this.scratchPromise) return this.scratchPromise ?? Promise.resolve();
    const id = this.loadingId;
    const request = new AbortController();
    this.scratchRequest = request;
    this.scratchPromise = (async () => {
      try {
        const response = await fetch(url, { signal: request.signal });
        if (!response.ok) throw new Error('Could not read audio for scratching.');
        const raw = await response.arrayBuffer();
        const decoded = await this.ctx.decodeAudioData(raw);
        if (id === this.loadingId && !this.disposed) {
          this.scratchBuffer = decoded;
          this.state.scratchReady = true;
          this.changed();
        }
      } catch (error) {
        if (!request.signal.aborted) {
          this.state.error = error instanceof Error ? error.message : String(error);
          this.changed();
        }
      } finally {
        if (this.scratchRequest === request) this.scratchRequest = null;
        this.scratchPromise = null;
      }
    })();
    await this.scratchPromise;
  }

  startScratch(): void {
    if (!this.state.song) return;
    this.state.scratching = true;
    this.scratchPrevious = this.state.time;
    this.audio.pause();
    this.stopStems();
    void this.ctx.resume();
    void this.prepareScratch();
    this.changed();
  }

  scratch(deltaSeconds: number): void {
    if (!this.state.scratching || !this.state.song) return;
    const position = clamped(this.scratchPrevious + deltaSeconds, 0, this.state.duration || Number.MAX_SAFE_INTEGER);
    this.scratchPrevious = position;
    this.seek(position);
    const decoded = this.scratchBuffer;
    if (!decoded) return;
    const backwards = deltaSeconds < 0;
    const frames = Math.min(Math.round(decoded.sampleRate * 0.065), decoded.length);
    const start = Math.max(0, Math.min(decoded.length - frames, Math.round(position * decoded.sampleRate) - (backwards ? frames : 0)));
    const grain = this.ctx.createBuffer(decoded.numberOfChannels, frames, decoded.sampleRate);
    for (let channel = 0; channel < decoded.numberOfChannels; channel++) {
      const source = decoded.getChannelData(channel);
      const dest = grain.getChannelData(channel);
      for (let i = 0; i < frames; i++) dest[i] = source[start + (backwards ? frames - 1 - i : i)] ?? 0;
    }
    const source = this.ctx.createBufferSource();
    const envelope = this.ctx.createGain();
    const now = this.ctx.currentTime;
    envelope.gain.setValueAtTime(0, now);
    envelope.gain.linearRampToValueAtTime(0.7, now + 0.007);
    envelope.gain.setValueAtTime(0.7, now + 0.035);
    envelope.gain.linearRampToValueAtTime(0, now + 0.065);
    source.buffer = grain;
    source.connect(envelope).connect(this.input);
    source.onended = () => { source.disconnect(); envelope.disconnect(); this.scratchSources = this.scratchSources.filter(entry => entry !== source); };
    this.scratchSources.push(source);
    source.start(now);
    source.stop(now + 0.066);
  }

  endScratch(): void {
    if (!this.state.scratching) return;
    this.state.scratching = false;
    if (this.playingRequested) void this.play();
    this.changed();
  }

  close(): void {
    this.disposed = true;
    this.loadingId++;
    this.scratchRequest?.abort();
    this.pause();
    this.audio.removeAttribute('src');
    this.audio.load();
    for (const voice of this.stemVoices) {
      voice.audio.pause();
      voice.audio.removeAttribute('src');
      voice.audio.load();
      voice.source.disconnect();
      voice.gain.disconnect();
    }
    for (const source of this.scratchSources) { try { source.stop(); } catch { /* already stopped */ } }
    this.mainSource.disconnect();
    this.mainGain.disconnect();
    this.input.disconnect();
    this.output.disconnect();
    this.meter.disconnect();
    this.delay.disconnect();
    this.feedback.disconnect();
    this.wet.disconnect();
  }
}

export class DjMixer {
  readonly context: AudioContext;
  readonly decks: [DjDeck, DjDeck];
  private readonly bus: GainNode;
  private readonly recordingOutput: MediaStreamAudioDestinationNode;
  private crossfade = 0.5;
  private readonly onChange: () => void;
  private recorder: MediaRecorder | null = null;
  private recordingParts: Blob[] = [];
  recording = false;

  constructor(onChange: () => void) {
    this.onChange = onChange;
    this.context = new AudioContext({ latencyHint: 'interactive' });
    this.bus = this.context.createGain();
    const limiter = this.context.createDynamicsCompressor();
    limiter.threshold.value = -4;
    limiter.knee.value = 8;
    limiter.ratio.value = 12;
    limiter.attack.value = 0.003;
    limiter.release.value = 0.2;
    this.bus.connect(limiter).connect(this.context.destination);
    this.recordingOutput = this.context.createMediaStreamDestination();
    limiter.connect(this.recordingOutput);
    this.decks = [
      new DjDeck(this.context, this.bus, onChange),
      new DjDeck(this.context, this.bus, onChange),
    ];
    this.setCrossfade(0.5);
  }

  setCrossfade(value: number): void {
    this.crossfade = clamped(value, 0, 1);
    const [a, b] = crossfadeGains(this.crossfade);
    this.decks[0].output.gain.setTargetAtTime(a * this.decks[0].state.level, this.context.currentTime, 0.012);
    this.decks[1].output.gain.setTargetAtTime(b * this.decks[1].state.level, this.context.currentTime, 0.012);
    this.onChange();
  }
  getCrossfade(): number { return this.crossfade; }
  gain(index: number, level: number): void {
    this.decks[index].gain(level);
    this.setCrossfade(this.crossfade);
  }
  sync(index: 0 | 1): boolean {
    const mine = this.decks[index].state;
    const other = this.decks[index === 0 ? 1 : 0].state;
    if (!mine.bpm || !other.bpm) return false;
    this.decks[index].rate(rateForSync(mine.bpm, other.bpm, other.rate));
    return true;
  }
  async startRecording(): Promise<void> {
    if (this.recorder || typeof MediaRecorder === 'undefined') throw new Error('Audio recording is not available in this browser.');
    await this.context.resume();
    this.recordingParts = [];
    const mimeType = ['audio/webm;codecs=opus', 'audio/webm', 'audio/ogg;codecs=opus'].find(type => MediaRecorder.isTypeSupported(type));
    this.recorder = new MediaRecorder(this.recordingOutput.stream, mimeType ? { mimeType } : undefined);
    this.recorder.ondataavailable = (event) => { if (event.data.size) this.recordingParts.push(event.data); };
    this.recorder.start(1000);
    this.recording = true;
    this.onChange();
  }
  async stopRecording(): Promise<Blob> {
    const recorder = this.recorder;
    if (!recorder) throw new Error('There is no recording in progress.');
    return new Promise<Blob>((resolve, reject) => {
      recorder.addEventListener('stop', () => {
        const result = new Blob(this.recordingParts, { type: recorder.mimeType || 'audio/webm' });
        this.recorder = null;
        this.recordingParts = [];
        this.recording = false;
        this.onChange();
        resolve(result);
      }, { once: true });
      recorder.addEventListener('error', () => reject(new Error('DJ mix recording failed.')), { once: true });
      recorder.stop();
    });
  }
  dispose(): void {
    if (this.recorder && this.recorder.state !== 'inactive') this.recorder.stop();
    this.decks.forEach(deck => deck.close());
    void this.context.close();
  }
}
