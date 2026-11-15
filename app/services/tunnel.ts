/**
 * Feedback-tunnel visualiser. A classic NCS wheel (white ring, pink-to-blue
 * radial bars) is stamped occasionally at the outer radius; each frame shrinks
 * and turns the previous picture so the wheels spiral inward and fade. The
 * newest wheel is drawn live so its bars keep dancing. One source hops to a
 * new spot inside the frame when a note attacks. Grains leave it as a flame,
 * and a jump pulls that flame into a tail.
 */

/** Bar count of the video studio's classic NCS circle. Bins used are 10…89. */
export const NCS_BARS = 80;

export interface TunnelDrive {
  /** 0…1 low-band energy → smoke spawn and ring brightness. */
  bass: number;
  /** 0…1 mid-band energy → arc thickness. */
  mid: number;
  /** 0…1 high-band energy → spark spawn. */
  treb: number;
  /** Wheel rotation angle in radians. */
  rotation: number;
  /** How many red smoke particles to emit this frame. */
  smokeSpawn: number;
  /** How many white sparks to emit this frame. */
  sparkSpawn: number;
  /** Ring brightness multiplier. */
  ringBright: number;
  /** Feedback zoom (< 1 contracts toward centre). */
  zoom: number;
  /** 0…1 hop amplitude for particles leaping toward the viewer. */
  jump: number;
  /** 0…1 how hard a new note attacked this frame. Unsmoothed. */
  hit: number;
  /** 0…1 height of each classic-NCS bar, same bins as the video preset. */
  bars: Float32Array;
}

export interface TunnelDriveInput {
  spectrum: Uint8Array | Float32Array | number[];
  sampleRate: number;
  fftSize: number;
  time: number;
  prev?: TunnelDrive;
  /** Previous frame's raw spectrum, for the note attack. */
  prevSpectrum?: ArrayLike<number> | null;
}

function bandMean(spectrum: ArrayLike<number>, sampleRate: number, fftSize: number, lo: number, hi: number): number {
  const binHz = sampleRate / fftSize;
  let sum = 0;
  let n = 0;
  for (let i = 0; i < spectrum.length; i++) {
    const hz = i * binHz;
    if (hz < lo || hz >= hi) continue;
    sum += spectrum[i];
    n++;
  }
  if (!n) return 0;
  const mean = sum / n;
  return mean > 1.5 ? mean / 255 : mean;
}

function mix(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/**
 * How hard a new note arrived. The biggest bin rise in the melodic range,
 * before any smoothing: a band average stays flat through a single note.
 */
export function noteHit(spectrum: ArrayLike<number>, prev: ArrayLike<number> | null, sampleRate: number, fftSize: number): number {
  if (!prev) return 0;
  const binHz = sampleRate / fftSize;
  let best = 0;
  const n = Math.min(spectrum.length, prev.length);
  for (let i = 0; i < n; i++) {
    const hz = i * binHz;
    if (hz < 60 || hz > 5000) continue;
    const rise = spectrum[i] - prev[i];
    if (rise > best) best = rise;
  }
  return clamp((best - 8) / 90, 0, 1);
}

/** 0…1 bar heights for the classic NCS circle: bar i reads spectrum bin i+10. */
export function ncsBars(spectrum: ArrayLike<number>, count = NCS_BARS): Float32Array {
  const out = new Float32Array(count);
  for (let i = 0; i < count; i++) {
    const sample = spectrum[i + 10];
    if (sample === undefined) continue;
    const v = sample > 1.5 ? sample / 255 : sample;
    out[i] = v < 0 ? 0 : v > 1 ? 1 : v;
  }
  return out;
}

/** Map analyser bins to spawn rates, hop height and shrink speed. Pure; smooths against `prev`. */
export function tunnelDrive(input: TunnelDriveInput): TunnelDrive {
  const bass = bandMean(input.spectrum, input.sampleRate, input.fftSize, 20, 150);
  const mid = bandMean(input.spectrum, input.sampleRate, input.fftSize, 150, 2000);
  const treb = bandMean(input.spectrum, input.sampleRate, input.fftSize, 2000, 12000);
  const jump = clamp(bass * 0.55 + mid * 0.25, 0, 1);
  // louder bass → slightly faster shrink (smaller zoom)
  const zoom = clamp(0.985 - bass * 0.025, 0.96, 0.985);
  const hit = noteHit(input.spectrum, input.prevSpectrum ?? null, input.sampleRate, input.fftSize);
  const bars = ncsBars(input.spectrum);
  const target: TunnelDrive = {
    bass,
    mid,
    treb,
    rotation: input.time * 0.55,
    smokeSpawn: 2 + bass * 14,
    sparkSpawn: treb * 18,
    ringBright: 0.45 + bass * 0.7 + mid * 0.25,
    zoom,
    jump,
    hit,
    bars,
  };
  const prev = input.prev;
  if (!prev) return target;
  const mixed = new Float32Array(NCS_BARS);
  for (let i = 0; i < NCS_BARS; i++) mixed[i] = mix(prev.bars?.[i] ?? 0, bars[i], 0.55);
  return {
    bass: mix(prev.bass, target.bass, 0.3),
    mid: mix(prev.mid, target.mid, 0.25),
    treb: mix(prev.treb, target.treb, 0.35),
    rotation: target.rotation,
    smokeSpawn: mix(prev.smokeSpawn, target.smokeSpawn, 0.35),
    sparkSpawn: mix(prev.sparkSpawn, target.sparkSpawn, 0.45),
    ringBright: mix(prev.ringBright, target.ringBright, 0.3),
    zoom: mix(prev.zoom, target.zoom, 0.2),
    jump: mix(prev.jump, target.jump, 0.35),
    hit,
    bars: mixed,
  };
}

// --- WebGL2 renderer -------------------------------------------------------

const FEED_VERT = `#version 300 es
precision highp float;
const vec2 P[3] = vec2[3](vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
out vec2 vUv;
void main() {
  vec2 p = P[gl_VertexID];
  vUv = p * 0.5 + 0.5;
  gl_Position = vec4(p, 0.0, 1.0);
}`;

const FEED_FRAG = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform sampler2D uPrev;
uniform sampler2D uBars;
uniform float uZoom;
uniform float uDecay;
uniform float uRotation;
uniform float uRingBright;
uniform float uThickness;
uniform float uAspect;
uniform float uSpin;
uniform float uDraw;
uniform float uPulse;
const float BARS = ${NCS_BARS}.0;
const float TAU = 6.28318530718;

void main() {
  vec2 c = vUv - 0.5;
  c.x *= uAspect;
  // zoom < 1: sample farther out → previous wheels shrink toward centre, and spin
  vec2 z = c / uZoom;
  float cs = cos(uSpin);
  float sn = sin(uSpin);
  vec2 spun = vec2(cs * z.x - sn * z.y, sn * z.x + cs * z.y);
  vec2 sampleUv = vec2(spun.x / uAspect, spun.y) + 0.5;
  vec3 col = vec3(0.0);
  if (sampleUv.x > 0.0 && sampleUv.x < 1.0 && sampleUv.y > 0.0 && sampleUv.y < 1.0) {
    col = texture(uPrev, sampleUv).rgb * uDecay;
  }

  // Classic NCS: white ring, bars just outside it, pink at the root and blue at the tip.
  // Proportions match the video preset (radius 150, bars from +10, height 8…128).
  float base = 0.20 * uPulse;
  float r = length(c);
  float ang = atan(c.y, c.x) - uRotation * 0.27;
  float slot = fract(ang / TAU + 0.5) * BARS;
  float amp = texture(uBars, vec2((floor(slot) + 0.5) / BARS, 0.5)).r;
  float spoke = 1.0 - smoothstep(0.20, 0.28, abs(fract(slot) - 0.5));
  float start = base * 1.0667;
  float reach = (0.0533 + pow(amp, 1.5) * 0.80) * base * mix(0.9, 1.1, uThickness);
  float along = r - start;
  float body = step(0.0, along) * (1.0 - smoothstep(reach - 0.004, reach + 0.002, along));
  vec3 pink = vec3(0.925, 0.282, 0.600);
  vec3 blue = vec3(0.231, 0.510, 0.965);
  vec3 rgb = mix(pink, blue, clamp(along / max(reach, 1e-4), 0.0, 1.0));
  float ring = 1.0 - smoothstep(0.0025, 0.007, abs(r - base));
  float glow = exp(-pow((r - base) * 90.0, 2.0)) * 0.45;
  float capAt = reach + base * 0.0133;
  float cap = spoke * (1.0 - smoothstep(0.0, base * 0.02, abs(along - capAt))) * step(0.0, along) * 0.28;
  float outer = (1.0 - smoothstep(0.001, 0.0035, abs(r - base * 2.0))) * 0.12;
  col += rgb * body * spoke * uDraw * uRingBright;
  col += (vec3(ring) + vec3(0.925, 0.282, 0.600) * glow + vec3(cap + outer)) * uDraw;

  fragColor = vec4(col, 1.0);
}`;

const POINT_VERT = `#version 300 es
precision highp float;
layout(location = 0) in vec2 aPos;
layout(location = 1) in vec4 aColor;
layout(location = 2) in float aSize;
out vec4 vColor;
void main() {
  vColor = aColor;
  gl_Position = vec4(aPos, 0.0, 1.0);
  gl_PointSize = aSize;
}`;

const POINT_FRAG = `#version 300 es
precision highp float;
in vec4 vColor;
out vec4 fragColor;
void main() {
  vec2 d = gl_PointCoord - 0.5;
  float a = 1.0 - smoothstep(0.2, 0.5, length(d));
  fragColor = vec4(vColor.rgb, vColor.a * a);
}`;

const COPY_FRAG = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform sampler2D uTex;
void main() {
  fragColor = texture(uTex, vUv);
}`;

/** Safe half-extents in ring space so a point of `pointPx` stays inside the framebuffer. */
export function sourceBounds(aspect: number, pointPx: number, viewW: number, viewH: number): { maxX: number; maxY: number } {
  const mx = (pointPx + 2) / Math.max(viewW, 1);
  const my = (pointPx + 2) / Math.max(viewH, 1);
  return {
    maxX: Math.max(0.02, (aspect / 2) * (1 - mx)),
    maxY: Math.max(0.02, 0.5 * (1 - my)),
  };
}

export interface Hop {
  x: number;
  y: number;
  tx: number;
  ty: number;
  cool: number;
  /** Unit heading toward the viewer. The flame trails the other way. */
  hx: number;
  hy: number;
}

/** A note softer than this leaves the source where it is. At or above it, the source jumps. */
export const JUMP_AT = 0.45;
/** Out of the vanishing point, toward the viewer. Screen-down is the near field. */
const TOWARD_USER_X = 0;
const TOWARD_USER_Y = -1;

/** Keep a hand-set threshold inside 0…1. Anything else falls back to `JUMP_AT`. */
export function jumpThreshold(value: number): number {
  return Number.isFinite(value) ? clamp(value, 0, 1) : JUMP_AT;
}

/** Unit direction from the vanishing point out toward the viewer. */
function faceUser(x: number, y: number): { x: number; y: number } {
  const len = Math.hypot(x, y);
  if (len < 1e-4) return { x: TOWARD_USER_X, y: TOWARD_USER_Y };
  return { x: x / len, y: y / len };
}

/**
 * One source. Below `jumpAt` it stays put, still facing the viewer, so the flame
 * can trail back into the tunnel. A harder attack lands at once inside `bounds`
 * and the heading follows the jump. `rand` is 0…1.
 */
export function hopSource(hop: Hop, hit: number, dt: number, bounds: { maxX: number; maxY: number }, rand: () => number = Math.random, jumpAt = JUMP_AT): { x: number; y: number; jumped: boolean } {
  hop.cool = Math.max(0, hop.cool - dt);
  if (hit >= jumpThreshold(jumpAt) && hop.cool <= 0) {
    hop.cool = 0.08;
    const reach = 0.18 + clamp(hit, 0, 1) * 0.12;
    hop.tx = (rand() * 2 - 1) * bounds.maxX * reach;
    hop.ty = (rand() * 2 - 1) * bounds.maxY * reach;
    const dx = hop.tx - hop.x;
    const dy = hop.ty - hop.y;
    const len = Math.hypot(dx, dy);
    if (len > 1e-6) {
      hop.hx = dx / len;
      hop.hy = dy / len;
    }
    hop.x = hop.tx;
    hop.y = hop.ty;
    return { x: hop.x, y: hop.y, jumped: true };
  }
  const face = faceUser(hop.x, hop.y);
  hop.hx = face.x;
  hop.hy = face.y;
  return { x: hop.x, y: hop.y, jumped: false };
}

/** How many grains the one source sheds this frame. A note throws more. */
export function sprayBirths(hit: number, treb: number): number {
  return 16 + Math.floor(clamp(hit, 0, 1) * 24 + clamp(treb, 0, 1) * 16);
}

/** A flame cone. `back` is the tail direction; `spread` is sideways and stays smaller than `along`. */
export function flameVelocity(backX: number, backY: number, spread: number, along: number): { vx: number; vy: number } {
  const len = Math.hypot(backX, backY) || 1;
  const bx = backX / len;
  const by = backY / len;
  return { vx: bx * along - by * spread, vy: by * along + bx * spread };
}

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const shader = gl.createShader(type);
  if (!shader) throw new Error('WebGL shader alloc failed');
  gl.shaderSource(shader, src);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader) ?? 'compile failed';
    gl.deleteShader(shader);
    throw new Error(log);
  }
  return shader;
}

function link(gl: WebGL2RenderingContext, vert: string, frag: string): WebGLProgram {
  const program = gl.createProgram();
  if (!program) throw new Error('WebGL program alloc failed');
  const vs = compile(gl, gl.VERTEX_SHADER, vert);
  const fs = compile(gl, gl.FRAGMENT_SHADER, frag);
  gl.attachShader(program, vs);
  gl.attachShader(program, fs);
  gl.linkProgram(program);
  gl.deleteShader(vs);
  gl.deleteShader(fs);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const log = gl.getProgramInfoLog(program) ?? 'link failed';
    gl.deleteProgram(program);
    throw new Error(log);
  }
  return program;
}

function makeTarget(gl: WebGL2RenderingContext, w: number, h: number): { fbo: WebGLFramebuffer; tex: WebGLTexture } {
  const tex = gl.createTexture()!;
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  const fbo = gl.createFramebuffer()!;
  gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  return { fbo, tex };
}

const TAIL = 64;
const SPRAY = 520;
const STRIDE = 7; // x y r g b a size
const RING_EVERY = 0.28;

export interface TunnelVisualizer {
  resize(width: number, height: number): void;
  render(drive: TunnelDrive, jumpAt?: number): void;
  destroy(): void;
}

/** NCS wheels that shrink and spin, plus one hopping emitter. */
export function createTunnelVisualizer(canvas: HTMLCanvasElement): TunnelVisualizer {
  const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, powerPreference: 'high-performance' });
  if (!gl) throw new Error('WebGL2 is not available');

  const feedProg = link(gl, FEED_VERT, FEED_FRAG);
  const pointProg = link(gl, POINT_VERT, POINT_FRAG);
  const copyProg = link(gl, FEED_VERT, COPY_FRAG);
  const quadVao = gl.createVertexArray()!;
  const pointVao = gl.createVertexArray()!;
  const pointBuf = gl.createBuffer()!;
  gl.bindVertexArray(pointVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, pointBuf);
  gl.bufferData(gl.ARRAY_BUFFER, (TAIL + SPRAY) * STRIDE * 4, gl.DYNAMIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, STRIDE * 4, 0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 4, gl.FLOAT, false, STRIDE * 4, 8);
  gl.enableVertexAttribArray(2);
  gl.vertexAttribPointer(2, 1, gl.FLOAT, false, STRIDE * 4, 24);
  gl.bindVertexArray(null);

  let dispW = 0;
  let dispH = 0;
  let fw = 0;
  let fh = 0;
  let ping = makeTarget(gl, 1, 1);
  let pong = makeTarget(gl, 1, 1);
  let writePing = true;

  const feedLoc = {
    prev: gl.getUniformLocation(feedProg, 'uPrev'),
    zoom: gl.getUniformLocation(feedProg, 'uZoom'),
    decay: gl.getUniformLocation(feedProg, 'uDecay'),
    rot: gl.getUniformLocation(feedProg, 'uRotation'),
    bright: gl.getUniformLocation(feedProg, 'uRingBright'),
    thick: gl.getUniformLocation(feedProg, 'uThickness'),
    aspect: gl.getUniformLocation(feedProg, 'uAspect'),
    spin: gl.getUniformLocation(feedProg, 'uSpin'),
    draw: gl.getUniformLocation(feedProg, 'uDraw'),
    bars: gl.getUniformLocation(feedProg, 'uBars'),
    pulse: gl.getUniformLocation(feedProg, 'uPulse'),
  };
  const copyLoc = gl.getUniformLocation(copyProg, 'uTex');
  const barTex = gl.createTexture()!;
  const barBytes = new Uint8Array(NCS_BARS);
  gl.bindTexture(gl.TEXTURE_2D, barTex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, NCS_BARS, 1, 0, gl.RED, gl.UNSIGNED_BYTE, barBytes);
  gl.useProgram(feedProg);
  gl.uniform1i(feedLoc.bars, 1);

  const tail: { x: number; y: number }[] = [];
  const sparks: { x: number; y: number; vx: number; vy: number; life: number }[] = [];
  const data = new Float32Array((TAIL + SPRAY) * STRIDE);
  const hop: Hop = { x: 0, y: 0, tx: 0, ty: 0, cool: 0, hx: TOWARD_USER_X, hy: TOWARD_USER_Y };
  let pull = { x: 0, y: 1, t: 0 };
  let ringClock = 0;

  const destroyTargets = () => {
    for (const t of [ping, pong]) {
      gl.deleteFramebuffer(t.fbo);
      gl.deleteTexture(t.tex);
    }
  };

  const clearTarget = (t: { fbo: WebGLFramebuffer }) => {
    gl.bindFramebuffer(gl.FRAMEBUFFER, t.fbo);
    gl.viewport(0, 0, fw, fh);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
  };

  const resizeInner = (w: number, h: number) => {
    const nw = Math.max(1, Math.round(Math.min(w, 960)));
    const nh = Math.max(1, Math.round((h * nw) / Math.max(w, 1)));
    if (nw === fw && nh === fh) return;
    destroyTargets();
    fw = nw;
    fh = nh;
    ping = makeTarget(gl, nw, nh);
    pong = makeTarget(gl, nw, nh);
    clearTarget(ping);
    clearTarget(pong);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  };

  const shed = (x: number, y: number, bx: number, by: number, along: number, life: number) => {
    if (sparks.length >= SPRAY) return;
    const v = flameVelocity(bx, by, (Math.random() - 0.5) * along * 0.45, along);
    sparks.push({ x, y, vx: v.vx, vy: v.vy, life });
  };

  /** One source. Idle grains trail back into the tunnel; a jump lays them along the path. */
  const stepSource = (drive: TunnelDrive, dt: number, aspect: number, jumpAt: number) => {
    const pointPx = (32 + drive.jump * 18) * (dispW / 480);
    const bounds = sourceBounds(aspect, pointPx, canvas.width, canvas.height);
    const fromX = hop.x;
    const fromY = hop.y;
    const next = hopSource(hop, drive.hit, dt, bounds, Math.random, jumpAt);
    const jumped = next.jumped;
    const dx = next.x - fromX;
    const dy = next.y - fromY;
    if (jumped) {
      const len = Math.hypot(dx, dy) || 1;
      pull = { x: -dx / len, y: -dy / len, t: 0.28 };
      const streak = 18;
      for (let i = 1; i <= streak; i++) {
        const u = i / streak;
        shed(next.x + (fromX - next.x) * u, next.y + (fromY - next.y) * u, pull.x, pull.y, 0.06 + u * 0.16, 1 - u * 0.55);
        tail.push({ x: next.x + (fromX - next.x) * u, y: next.y + (fromY - next.y) * u });
      }
    }
    pull.t = Math.max(0, pull.t - dt);
    const bx = pull.t > 0 ? pull.x : -hop.hx;
    const by = pull.t > 0 ? pull.y : -hop.hy;
    for (const p of tail) {
      p.x = clamp(p.x, -bounds.maxX, bounds.maxX);
      p.y = clamp(p.y, -bounds.maxY, bounds.maxY);
    }
    tail.push(next);
    while (tail.length > TAIL) tail.shift();
    const born = sprayBirths(jumped ? 1 : drive.hit, drive.treb);
    for (let i = 0; i < born; i++) shed(next.x, next.y, bx, by, 0.16 + Math.random() * 0.14, 1);
    const drag = Math.exp(-dt * 2.4);
    for (let i = sparks.length - 1; i >= 0; i--) {
      const p = sparks[i];
      p.life -= dt / 0.5;
      p.vx *= drag;
      p.vy *= drag;
      p.x += p.vx * dt;
      p.y += p.vy * dt;
      if (p.life <= 0 || Math.abs(p.x) > bounds.maxX || Math.abs(p.y) > bounds.maxY) sparks.splice(i, 1);
    }
  };

  const packPoint = (x: number, y: number, r: number, g: number, b: number, a: number, size: number, o: number, aspect: number) => {
    data[o] = (x * 2) / Math.max(aspect, 1e-6);
    data[o + 1] = y * 2;
    data[o + 2] = r;
    data[o + 3] = g;
    data[o + 4] = b;
    data[o + 5] = a;
    data[o + 6] = size * (dispW / 480);
  };

  let lastT = performance.now();

  return {
    resize(width: number, height: number) {
      dispW = Math.max(1, Math.round(width));
      dispH = Math.max(1, Math.round(height));
      const ratio = window.devicePixelRatio || 1;
      canvas.width = Math.round(dispW * ratio);
      canvas.height = Math.round(dispH * ratio);
      resizeInner(dispW, dispH);
    },
    render(drive: TunnelDrive, jumpAt = JUMP_AT) {
      if (!fw || !fh) return;
      const now = performance.now();
      const dt = Math.min(0.05, (now - lastT) / 1000);
      lastT = now;

      ringClock += dt;
      let drawRing = 0;
      if (ringClock >= RING_EVERY) {
        ringClock -= RING_EVERY;
        drawRing = 1;
      }
      const aspect = fw / Math.max(fh, 1);
      stepSource(drive, dt, aspect, jumpAt);

      const read = writePing ? ping : pong;
      const write = writePing ? pong : ping;
      // drive zoom 0.96..0.985 is too fast and smears rings into a comb; keep a slow shrink
      const z = clamp(drive.zoom, 0.96, 0.985);
      const zoom = 0.9975 - ((0.985 - z) / 0.025) * 0.002;
      const pulse = 1 + clamp(drive.bass, 0, 1) * 0.15;
      for (let i = 0; i < NCS_BARS; i++) barBytes[i] = Math.round(clamp(drive.bars?.[i] ?? 0, 0, 1) * 255);
      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, barTex);
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, NCS_BARS, 1, gl.RED, gl.UNSIGNED_BYTE, barBytes);

      gl.bindFramebuffer(gl.FRAMEBUFFER, write.fbo);
      gl.viewport(0, 0, fw, fh);
      gl.disable(gl.BLEND);
      gl.useProgram(feedProg);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, read.tex);
      gl.uniform1i(feedLoc.prev, 0);
      gl.uniform1f(feedLoc.zoom, zoom);
      gl.uniform1f(feedLoc.decay, 0.992);
      gl.uniform1f(feedLoc.rot, drive.rotation);
      gl.uniform1f(feedLoc.bright, clamp(drive.ringBright, 0.35, 1.1));
      gl.uniform1f(feedLoc.thick, clamp(drive.mid, 0, 1));
      gl.uniform1f(feedLoc.aspect, aspect);
      gl.uniform1f(feedLoc.spin, 0.45 * dt);
      gl.uniform1f(feedLoc.draw, drawRing);
      gl.uniform1f(feedLoc.pulse, pulse);
      gl.bindVertexArray(quadVao);
      gl.drawArrays(gl.TRIANGLES, 0, 3);

      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.useProgram(copyProg);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, write.tex);
      gl.uniform1i(copyLoc, 0);
      gl.bindVertexArray(quadVao);
      gl.drawArrays(gl.TRIANGLES, 0, 3);

      // the stamp frame already shows this wheel; other frames it dances in place while ghosts shrink
      if (!drawRing) {
        gl.enable(gl.BLEND);
        gl.blendFunc(gl.ONE, gl.ONE);
        gl.useProgram(feedProg);
        gl.uniform1f(feedLoc.zoom, 1);
        gl.uniform1f(feedLoc.decay, 0);
        gl.uniform1f(feedLoc.spin, 0);
        gl.uniform1f(feedLoc.draw, 1);
        gl.uniform1f(feedLoc.pulse, pulse);
        gl.drawArrays(gl.TRIANGLES, 0, 3);
      }

      let n = 0;
      for (const p of sparks) {
        if (n >= SPRAY) break;
        packPoint(p.x, p.y, 1, p.life * 0.72, p.life * 0.22, 0.35 + p.life * 0.6, 7 + p.life * 16, n * STRIDE, aspect);
        n++;
      }
      const last = tail.length - 1;
      for (let i = 0; i < tail.length && n < TAIL + SPRAY; i++) {
        const p = tail[i];
        const t = i / Math.max(last, 1);
        const source = i === last;
        const size = source ? 32 + drive.jump * 18 : 8 + t * 16;
        packPoint(p.x, p.y, 1, source ? 0.92 : t * 0.55, source ? 0.55 : t * 0.12, source ? 1 : 0.25 + t * 0.45, size, n * STRIDE, aspect);
        n++;
      }
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
      gl.useProgram(pointProg);
      gl.bindVertexArray(pointVao);
      gl.bindBuffer(gl.ARRAY_BUFFER, pointBuf);
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, data.subarray(0, n * STRIDE));
      gl.drawArrays(gl.POINTS, 0, n);
      gl.disable(gl.BLEND);

      writePing = !writePing;
    },
    destroy() {
      destroyTargets();
      gl.deleteProgram(feedProg);
      gl.deleteProgram(pointProg);
      gl.deleteProgram(copyProg);
      gl.deleteVertexArray(quadVao);
      gl.deleteVertexArray(pointVao);
      gl.deleteBuffer(pointBuf);
      gl.deleteTexture(barTex);
      // same canvas is compiled again after a dev remount; losing the context fails that compile
    },
  };
}
