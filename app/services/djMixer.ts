import type { Song } from '../types';
import { stemOf } from './songStems';
import { apiUrl } from './apiBase';
import { MAX_PLAYBACK_RATE, MIN_PLAYBACK_RATE } from './playbackTempo';

/** Web Audio mixing uses equal-power crossfades so the middle does not fall silent. */
export function crossfadeGains(value: number): [number, number] {
  const x = Math.min(1, Math.max(0, value));
  return [Math.cos(x * Math.PI / 2), Math.sin(x * Math.PI / 2)];
}
export function rateForSync(sourceBpm: number, targetBpm: number, targetRate: number): number {
  return Math.max(MIN_PLAYBACK_RATE, Math.min(MAX_PLAYBACK_RATE, targetBpm * targetRate / sourceBpm));
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
  audio: HTMLAudioElement;
  source: MediaElementAudioSourceNode;
  gain: GainNode;
  state: DjStem;
}
const clamped = (value: number, min: number, max: number) => Math.max(min, Math.min(max, Number.isFinite(value) ? value : min));
const MIN_LOOP_SECONDS = 0.25;

/**
 * Each deck has its own audio elements, EQ, filter, cue points, loops, stem mixer and
 * short sample-grain scratch playback. Both feed the same AudioContext/master bus.
 * The studio's own player is paused on entering DJ mode: no competing sound sources.
 */
/** Do not sum stems made by different separators of the same song. */
export function latestLibraryStems(songId: string, library: Song[]): Song[] {
  const stems = library.filter(song => song.audioUrl && song.derived?.from === songId && stemOf(song) !== null)
    .sort((left, right) => right.createdAt.getTime() - left.createdAt.getTime());
  const model = stems[0]?.derived?.settings?.model;
  const roles = new Set<string>();
  return stems.filter(stem => {
    const role = stemOf(stem)!;
    if (stem.derived?.settings?.model !== model || roles.has(role)) return false;
    roles.add(role);
    return true;
  });
}

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
  private readonly scratchStems = new Map<StemVoice, AudioBuffer>();
  private scratchRequest: AbortController | null = null;
  private scratchPromise: Promise<void> | null = null;
  private scratchPrevious = 0;
  private scratchLastAt = -1;
  private scratchSources: AudioBufferSourceNode[] = [];
  private disposed = false;
  private playingRequested = false;
  private loadingId = 0;
  private playAttempt = 0;
  private stemAttempt = 0;

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
      this.state.loopIn = Math.min(this.state.loopIn, this.state.duration);
      this.state.loopOut = Math.min(this.state.loopOut || this.state.duration, this.state.duration);
      if (!this.hasLoop()) this.state.looping = false;
      this.changed();
    });
    this.audio.addEventListener('timeupdate', () => this.updateTime());
    this.audio.addEventListener('ended', () => {
      if (this.playingRequested && this.state.looping && this.hasLoop()) {
        this.seek(this.state.loopIn);
        void this.play();
        return;
      }
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
    if (this.disposed || !song.audioUrl) return;
    this.loadingId++;
    this.playAttempt++;
    this.stemAttempt++;
    this.scratchRequest?.abort();
    this.scratchRequest = null;
    this.scratchPromise = null;
    this.scratchBuffer = null;
    this.stopScratchGrains();
    this.scratchLastAt = -1;
    this.playingRequested = false;
    this.audio.pause();
    this.stopStems();
    this.clearStemVoices();
    this.audio.src = song.audioUrl;
    this.audio.load();
    this.audio.defaultPlaybackRate = 1;
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
    for (const stem of latestLibraryStems(song.id, library)) {
      const audio = new Audio();
      audio.crossOrigin = 'anonymous';
      audio.src = stem.audioUrl!;
      audio.preload = 'metadata';
      audio.preservesPitch = false;
      const source = this.ctx.createMediaElementSource(audio);
      const gain = this.ctx.createGain();
      gain.gain.value = 0;
      source.connect(gain).connect(this.input);
      const state: DjStem = { id: stem.id, label: stemOf(stem) ?? 'other', level: 1, mute: false, solo: false };
      this.stemVoices.push({ audio, source, gain, state });
    }
    this.state.stems = this.stemVoices.map(voice => ({ ...voice.state }));
    this.changed();
    // Prepare scratch audio in the background as soon as the track is loaded.
    // Playback starts independently; the platter becomes scratch-ready when decoding finishes.
    void this.prepareScratch();
  }


  /** Reuse the native separator's files immediately, without needing a DJ-mode restart.
   * Their source track stays on the deck: the main clock remains the transport.
   */
  attachNativeStems(songId: string, names: string[], force = false): void {
    if (this.disposed || this.state.song?.id !== songId) return;
    const allowed = new Set(['vocals', 'instrumental', 'drums', 'bass', 'guitar', 'piano', 'other']);
    const stems = [...new Set(names.filter(name => allowed.has(name)))];
    if (!stems.length) return;
    const loaded = this.state.stems.map(stem => stem.label).sort();
    if (!force && loaded.length === stems.length && loaded.every((label, i) => label === [...stems].sort()[i])) return;
    const enabled = this.state.stemMode;
    if (enabled) this.setStemMode(false);
    this.clearStemVoices();
    for (const name of stems) {
      const audio = new Audio();
      audio.crossOrigin = 'anonymous';
      audio.src = apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/stems/${encodeURIComponent(name)}`);
      audio.preload = 'metadata';
      audio.preservesPitch = false;
      audio.playbackRate = this.state.rate;
      const source = this.ctx.createMediaElementSource(audio);
      const gain = this.ctx.createGain();
      gain.gain.value = 0;
      source.connect(gain).connect(this.input);
      this.stemVoices.push({ audio, source, gain,
        state: { id: `${songId}:${name}`, label: name, level: 1, mute: false, solo: false } });
    }
    this.state.stems = this.stemVoices.map(voice => ({ ...voice.state }));
    this.changed();
    if (enabled) this.setStemMode(true);
  }

  /** The named buttons are actual separated audio, not EQ-only approximations. */
  stemPreset(preset: 'original' | 'all' | 'vocals' | 'instrumental' | 'bass' | 'drums'): boolean {
    if (preset === 'original') { this.setStemMode(false); return true; }
    if (!this.stemVoices.length) return false;
    const available = this.stemVoices.some(voice => voice.state.label === preset);
    if (preset !== 'all' && preset !== 'instrumental' && !available) return false;
    if (preset === 'instrumental' && !this.stemVoices.some(voice => voice.state.label !== 'vocals')) return false;
    for (const voice of this.stemVoices) {
      const name = voice.state.label;
      const selected = preset === 'all' || (preset === 'instrumental' ? name !== 'vocals' : name === preset);
      voice.state = { ...voice.state, mute: !selected, solo: false };
    }
    this.state.stems = this.stemVoices.map(voice => ({ ...voice.state }));
    this.setStemMode(true);
    this.applyStemLevels();
    this.changed();
    return true;
  }

  private clearStemVoices(): void {
    this.scratchStems.clear();
    for (const voice of this.stemVoices) {
      voice.audio.pause();
      voice.audio.removeAttribute('src');
      voice.audio.load();
      voice.source.disconnect();
      voice.gain.disconnect();
    }
    this.stemVoices = [];
  }

  async play(): Promise<void> {
    if (!this.state.song) return;
    this.state.error = null;
    this.playingRequested = true;
    const attempt = ++this.playAttempt;
    try {
      await this.ctx.resume();
      if (attempt !== this.playAttempt) return;
      await this.audio.play();
      if (attempt !== this.playAttempt) return;
      if (this.state.stemMode) {
        await this.playStems();
      }
      if (attempt !== this.playAttempt) return;
      this.state.playing = true;
    } catch (error) {
      if (attempt !== this.playAttempt) return;
      this.playingRequested = false;
      this.audio.pause();
      this.stopStems();
      this.state.playing = false;
      this.state.error = error instanceof Error ? error.message : String(error);
    }
    this.changed();
  }

  pause(): void {
    this.playAttempt++;
    this.stemAttempt++;
    this.playingRequested = false;
    this.audio.pause();
    this.stopStems();
    this.state.playing = false;
    this.changed();
  }

  toggle(): void {
    if (this.playingRequested) this.pause();
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
    const next = clamped(rate, MIN_PLAYBACK_RATE, MAX_PLAYBACK_RATE);
    this.state.rate = next;
    this.audio.defaultPlaybackRate = next;
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

  private async playStems(): Promise<void> {
    const attempt = ++this.stemAttempt;
    this.syncStemClock();
    try {
      await Promise.all(this.stemVoices.map(voice => voice.audio.play()));
    } catch (error) {
      if (attempt !== this.stemAttempt || !this.state.stemMode) return;
      this.setStemMode(false);
      this.state.error = error instanceof Error ? error.message : String(error);
      this.changed();
    }
  }

  setStemMode(enabled: boolean): void {
    if (!this.stemVoices.length) return;
    this.stemAttempt++;
    if (this.state.stemMode !== enabled) {
      this.stopScratchGrains();
      this.scratchRequest?.abort();
      this.scratchRequest = null;
      this.scratchPromise = null;
      this.state.stemMode = enabled;
      this.state.scratchReady = enabled
        ? this.stemVoices.every(voice => this.scratchStems.has(voice))
        : this.scratchBuffer !== null;
      if (this.state.scratching) void this.prepareScratch();
    }
    this.mainGain.gain.setTargetAtTime(enabled ? 0 : 1, this.ctx.currentTime, 0.02);
    this.applyStemLevels();
    if (enabled && this.playingRequested && !this.state.scratching) void this.playStems();
    else this.stopStems();
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
    this.state.loopIn = clamped(this.state.time, 0, this.state.duration);
    if (!this.hasLoop()) {
      this.state.loopOut = Math.min(this.state.duration, this.state.loopIn + 4);
      if (!this.hasLoop()) this.state.looping = false;
    }
    this.changed();
  }
  setLoopOut(): void {
    this.state.loopOut = clamped(Math.max(this.state.loopIn + MIN_LOOP_SECONDS, this.state.time), this.state.loopIn, this.state.duration);
    this.state.looping = this.hasLoop();
    this.changed();
  }
  toggleLoop(): void {
    if (!this.hasLoop()) return;
    this.state.looping = !this.state.looping;
    this.changed();
  }

  private hasLoop(): boolean {
    return this.state.loopOut >= this.state.loopIn + MIN_LOOP_SECONDS && this.state.loopOut <= this.state.duration;
  }

  /** The pitch bend wheel has no artificial sound-sample length limit.
   * The track is decoded on first scratch, and small forward/reverse grains
   * are played at the actual pointer position while its transport is paused.
   */
  async prepareScratch(): Promise<void> {
    const url = this.state.song?.audioUrl;
    if (!url || this.state.scratchReady || this.scratchPromise) return this.scratchPromise ?? Promise.resolve();
    const stems = this.state.stemMode ? this.stemVoices : [];
    const request = new AbortController();
    this.scratchRequest = request;
    this.scratchPromise = (async () => {
      try {
        const decode = async (source: string) => {
          const response = await fetch(source, { signal: request.signal });
          if (!response.ok) throw new Error('Could not read audio for scratching.');
          return this.ctx.decodeAudioData(await response.arrayBuffer());
        };
        if (stems.length) {
          for (const voice of stems) {
            if (this.scratchStems.has(voice)) continue;
            const decoded = await decode(voice.audio.src);
            if (request.signal.aborted) return;
            this.scratchStems.set(voice, decoded);
          }
        } else {
          const decoded = await decode(url);
          if (request.signal.aborted) return;
          this.scratchBuffer = decoded;
        }
        this.state.scratchReady = true;
        this.changed();
      } catch (error) {
        if (!request.signal.aborted) {
          this.state.error = error instanceof Error ? error.message : String(error);
          this.changed();
        }
      } finally {
        if (this.scratchRequest === request) {
          this.scratchRequest = null;
          this.scratchPromise = null;
        }
      }
    })();
    await this.scratchPromise;
  }

  startScratch(): void {
    if (!this.state.song) return;
    this.state.scratching = true;
    this.scratchPrevious = this.state.time;
    this.scratchLastAt = -1;
    this.audio.pause();
    this.stopStems();
    void this.ctx.resume();
    void this.prepareScratch();
    this.changed();
  }

  scratch(deltaSeconds: number): void {
    if (!this.state.scratching || !this.state.song || !Number.isFinite(deltaSeconds)) return;
    const position = clamped(this.scratchPrevious + deltaSeconds, 0, this.state.duration || Number.MAX_SAFE_INTEGER);
    this.scratchPrevious = position;
    // Track the stylus position without repeatedly seeking every stem's media element.
    this.audio.currentTime = position;
    this.state.time = position;
    this.changed();
    if (!this.state.scratchReady || Math.abs(deltaSeconds) < 0.0005) return;

    // Bounded, overlapping windowed grains are smoother than one full sample
    // for every pointer event; a reversed window provides backwards scratching.
    const now = this.ctx.currentTime;
    if (this.scratchLastAt >= 0 && now - this.scratchLastAt < 0.028) return;
    this.scratchLastAt = now;
    const backwards = deltaSeconds < 0;
    if (this.state.stemMode) {
      for (const voice of this.stemVoices) {
        const decoded = this.scratchStems.get(voice);
        if (decoded) this.playScratchGrain(decoded, voice.gain, position, backwards, now);
      }
    } else if (this.scratchBuffer) {
      this.playScratchGrain(this.scratchBuffer, this.input, position, backwards, now);
    }
  }

  private playScratchGrain(decoded: AudioBuffer, destination: AudioNode, position: number, backwards: boolean, now: number): void {
    const lengthSeconds = 0.09;
    const frames = Math.min(decoded.length, Math.max(128, Math.round(decoded.sampleRate * lengthSeconds)));
    if (decoded.length < 128) return;
    const start = Math.max(0, Math.min(decoded.length - frames, Math.round(position * decoded.sampleRate) - (backwards ? frames : 0)));
    const grain = this.ctx.createBuffer(decoded.numberOfChannels, frames, decoded.sampleRate);
    for (let channel = 0; channel < decoded.numberOfChannels; channel++) {
      const original = decoded.getChannelData(channel);
      const target = grain.getChannelData(channel);
      for (let frame = 0; frame < frames; frame++) {
        target[frame] = original[start + (backwards ? frames - 1 - frame : frame)];
      }
    }
    const source = this.ctx.createBufferSource();
    const envelope = this.ctx.createGain();
    source.buffer = grain;
    const duration = frames / decoded.sampleRate;
    envelope.gain.setValueAtTime(0, now);
    envelope.gain.linearRampToValueAtTime(0.45, now + Math.min(0.014, duration / 4));
    envelope.gain.setValueAtTime(0.45, now + duration * 0.62);
    envelope.gain.linearRampToValueAtTime(0, now + duration);
    source.connect(envelope).connect(destination);
    source.onended = () => {
      source.disconnect();
      envelope.disconnect();
      this.scratchSources = this.scratchSources.filter(entry => entry !== source);
    };
    this.scratchSources.push(source);
    source.start(now);
    source.stop(now + duration);
  }

  private stopScratchGrains(): void {
    for (const source of this.scratchSources) {
      source.disconnect();
      try { source.stop(); } catch { /* already stopped */ }
    }
    this.scratchSources = [];
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
    this.clearStemVoices();
    this.stopScratchGrains();
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
  private startingRecording = false;
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
    if (this.startingRecording) return;
    if (this.recorder || typeof MediaRecorder === 'undefined') throw new Error('Audio recording is not available in this browser.');
    this.startingRecording = true;
    try {
      await this.context.resume();
      const mimeType = ['audio/webm;codecs=opus', 'audio/webm', 'audio/ogg;codecs=opus', 'audio/mp4'].find(type => MediaRecorder.isTypeSupported(type));
      const recorder = new MediaRecorder(this.recordingOutput.stream, mimeType ? { mimeType } : undefined);
      this.recordingParts = [];
      recorder.ondataavailable = (event) => { if (event.data.size) this.recordingParts.push(event.data); };
      recorder.start(1000);
      this.recorder = recorder;
      this.recording = true;
    } finally {
      this.startingRecording = false;
    }
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
