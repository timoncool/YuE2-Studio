import { BufferTarget, EncodedPacket, EncodedVideoPacketSource, Mp4OutputFormat, Output } from 'mediabunny';

/**
 * Hardware H.264 encoding for the video studio.
 *
 * "Hardware" is not necessarily the graphics card: it is whatever
 * fixed-function encoder the machine has - NVENC on an NVIDIA GPU, Quick Sync
 * inside an Intel processor, VCE on AMD. The browser picks one; this only asks.
 *
 * The studio used to capture every frame as a base64 JPEG and hand the lot to
 * ffmpeg compiled to WebAssembly, which encodes on one CPU core - minutes for
 * a two-minute track. WebCodecs hands the same frames to the GPU encoder the
 * machine already has and muxes the result directly, so the encode costs
 * roughly what the drawing costs.
 *
 * It is a capability, not an assumption: if the browser has no VideoEncoder,
 * or refuses the configuration, the caller falls back to the WebAssembly path
 * and the export still works.
 */

/** Which encoder the export ended up on, for the caller to report. */
export type EncoderKind = 'hardware' | 'software';

export interface HardwareEncoder {
  /** Where the work is actually happening. */
  readonly kind: EncoderKind;
  /** Encodes one drawn frame, waiting if the encoder is behind. */
  encode(canvas: HTMLCanvasElement, frameIndex: number): Promise<void>;
  /** Finishes the file and returns silent MP4 bytes. */
  finish(): Promise<Uint8Array>;
  close(): void;
}

interface EncoderOptions {
  width: number;
  height: number;
  fps: number;
  /** Roughly 0.1 bits per pixel per frame at 1080p30 looks clean enough. */
  bitrate?: number;
}

/** A key frame every two seconds keeps seeking usable without bloating size. */
const KEYFRAME_SECONDS = 2;

/**
 * How many frames may sit in the encoder's queue.
 *
 * Frames handed to `encode` hold graphics memory until the encoder gets to
 * them; feeding a render loop faster than the encoder drains grows that queue
 * until the process runs out of video memory. The loop waits instead.
 */
const QUEUE_LIMIT = 12;
const QUEUE_WAIT_TIMEOUT_MS = 15_000;

/**
 * Tried in order. Hardware encoders differ in the profiles they accept, and a
 * configuration this machine cannot run is better found here than three
 * minutes into an export.
 */
const CODECS = ['avc1.640028', 'avc1.4d0028', 'avc1.42e01e'];

export async function createHardwareEncoder(options: EncoderOptions): Promise<HardwareEncoder | null> {
  const VideoEncoderClass = (globalThis as { VideoEncoder?: typeof VideoEncoder }).VideoEncoder;
  if (!VideoEncoderClass || typeof VideoFrame === 'undefined') return null;

  const { width, height, fps } = options;
  // Dimensions must be even for 4:2:0 chroma; the canvas is not always.
  if (width % 2 !== 0 || height % 2 !== 0) return null;

  const bitrate = options.bitrate ?? Math.round(width * height * fps * 0.1);
  // Three steps down, in order: a fixed-function encoder wherever it lives,
  // graphics card or processor; then the browser's own software encoder; and
  // when neither configures, the caller's WebAssembly path.
  let config: VideoEncoderConfig | null = null;
  let kind: EncoderKind = 'hardware';
  for (const acceleration of ['prefer-hardware', 'no-preference'] as const) {
    for (const codec of CODECS) {
      const candidate: VideoEncoderConfig = {
        codec,
        width,
        height,
        framerate: fps,
        bitrate,
        hardwareAcceleration: acceleration,
        avc: { format: 'avc' },
      };
      const support = await VideoEncoderClass.isConfigSupported(candidate).catch(() => null);
      if (support?.supported) {
        config = support.config ?? candidate;
        kind = acceleration === 'prefer-hardware' ? 'hardware' : 'software';
        break;
      }
    }
    if (config) break;
  }
  if (!config) return null;

  const target = new BufferTarget();
  const output = new Output({ target, format: new Mp4OutputFormat({ fastStart: 'in-memory' }) });
  const video = new EncodedVideoPacketSource('avc');
  output.addVideoTrack(video, { frameRate: fps });

  let failure: unknown = null;
  let packets = Promise.resolve();
  let closed = false;
  const encoder = new VideoEncoderClass({
    output: (chunk, meta) => {
      // WebCodecs supplies chunks in decode order. Drain the asynchronous writes in that order.
      packets = packets.then(async () => {
        if (!closed && !failure) await video.add(EncodedPacket.fromEncodedChunk(chunk), meta);
      }).catch((error) => { failure ??= error; });
    },
    error: (error) => { failure ??= error; },
  });

  const close = () => {
    if (closed) return;
    closed = true;
    try { encoder.close(); } catch { /* already closed */ }
    if (output.state !== 'finalized') void output.cancel().catch(() => undefined);
  };

  const checkState = () => {
    if (failure) throw failure;
    if (closed) throw new DOMException('The video encoder was closed.', 'AbortError');
  };

  try {
    encoder.configure(config);
    await output.start();
  } catch (error) {
    close();
    throw error;
  }

  const keyFrameEvery = Math.max(1, Math.round(fps * KEYFRAME_SECONDS));

  return {
    kind,
    async encode(canvas, frameIndex) {
      await packets;
      checkState();
      // Wait rather than pile frames up in graphics memory.
      const queueWaitStarted = performance.now();
      while (encoder.encodeQueueSize > QUEUE_LIMIT) {
        await new Promise((resolve) => setTimeout(resolve, 8));
        checkState();
        if (performance.now() - queueWaitStarted >= QUEUE_WAIT_TIMEOUT_MS) {
          throw new Error('The video encoder stopped processing frames.');
        }
      }
      // A VideoFrame holds GPU memory until closed; leaking them stalls the
      // encoder within a few seconds of footage.
      const frame = new VideoFrame(canvas, {
        timestamp: Math.round((frameIndex * 1_000_000) / fps),
        duration: Math.round(1_000_000 / fps),
      });
      try {
        encoder.encode(frame, { keyFrame: frameIndex % keyFrameEvery === 0 });
      } finally {
        frame.close();
      }
    },
    async finish() {
      try {
        checkState();
        await encoder.flush();
        await packets;
        checkState();
        await output.finalize();
        const buffer = target.buffer;
        if (!buffer?.byteLength) throw new Error('The encoder produced an empty file.');
        checkState();
        return new Uint8Array(buffer);
      } finally {
        close();
      }
    },
    close,
  };
}

/** True when this machine can encode video without the WebAssembly fallback. */
export async function hardwareEncodingAvailable(width: number, height: number, fps: number): Promise<boolean> {
  const encoder = await createHardwareEncoder({ width, height, fps });
  if (!encoder) return false;
  encoder.close();
  return true;
}
