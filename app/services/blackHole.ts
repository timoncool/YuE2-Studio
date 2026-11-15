/**
 * Schwarzschild geodesic black-hole visualiser. Rs = 1, M = 1/2.
 * Audio drives exposure, bloom, elevation and orbital Doppler asymmetry;
 * disk rotation follows wall time. Equations follow Belbruno & Pretorius
 * (geodesic) and the distant-observer circular-emitter frequency shift.
 */

export interface BlackHoleDrive {
  exposure: number;
  bloom: number;
  /** Radians from the equatorial plane. */
  elevation: number;
  /** −1…1 orbital brightness asymmetry. */
  asymmetry: number;
  /** Disk rotation angle in radians. */
  rotation: number;
}

export interface BlackHoleDriveInput {
  spectrum: Uint8Array | Float32Array | number[];
  sampleRate: number;
  fftSize: number;
  time: number;
  prev?: BlackHoleDrive;
}

const BASE_ELEV = (7.7 * Math.PI) / 180;
const ELEV_SPAN = (18 * Math.PI) / 180;

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
  // byte data is 0–255; float data may already be 0–1
  const mean = sum / n;
  return mean > 1.5 ? mean / 255 : mean;
}

function centroidHz(spectrum: ArrayLike<number>, sampleRate: number, fftSize: number): number {
  const binHz = sampleRate / fftSize;
  let weighted = 0;
  let total = 0;
  for (let i = 1; i < spectrum.length; i++) {
    const v = spectrum[i] > 1.5 ? spectrum[i] / 255 : spectrum[i];
    weighted += v * i * binHz;
    total += v;
  }
  return total > 1e-6 ? weighted / total : 400;
}

function mix(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/** Map analyser bins to the shader uniforms. Pure; smooths against `prev`. */
export function blackHoleDrive(input: BlackHoleDriveInput): BlackHoleDrive {
  const bass = bandMean(input.spectrum, input.sampleRate, input.fftSize, 20, 150);
  const mid = bandMean(input.spectrum, input.sampleRate, input.fftSize, 150, 2000);
  const cent = centroidHz(input.spectrum, input.sampleRate, input.fftSize);
  // log centroid ~200 Hz → low elevation, ~4 kHz → high
  const centT = clamp((Math.log(cent) - Math.log(200)) / (Math.log(4000) - Math.log(200)), 0, 1);
  const target: BlackHoleDrive = {
    exposure: 0.85 + bass * 1.4,
    bloom: 0.35 + bass * 1.1,
    elevation: BASE_ELEV + (centT - 0.35) * ELEV_SPAN,
    asymmetry: clamp((mid - 0.15) * 2.2, -1, 1),
    rotation: input.time * 0.35,
  };
  const prev = input.prev;
  if (!prev) return target;
  return {
    exposure: mix(prev.exposure, target.exposure, 0.25),
    bloom: mix(prev.bloom, target.bloom, 0.2),
    elevation: mix(prev.elevation, target.elevation, 0.04),
    asymmetry: mix(prev.asymmetry, target.asymmetry, 0.12),
    rotation: target.rotation,
  };
}

// --- WebGL2 renderer -------------------------------------------------------

const VERT = `#version 300 es
precision highp float;
const vec2 P[3] = vec2[3](vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
out vec2 vUv;
void main() {
  vec2 p = P[gl_VertexID];
  vUv = p * 0.5 + 0.5;
  gl_Position = vec4(p, 0.0, 1.0);
}`;

const SCENE_FRAG = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform vec2 uResolution;
uniform float uElevation;
uniform float uRotation;
uniform float uExposure;
uniform float uAsymmetry;

vec3 accel(vec3 p, float h2) {
  float r2 = dot(p, p);
  float r = sqrt(r2);
  float r5 = r2 * r2 * r;
  return -1.5 * h2 * p / max(r5, 1e-8);
}

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

vec3 diskColor(float r, float ang) {
  float rings = 0.55 + 0.45 * sin(r * 3.7 + ang * 2.0);
  float grain = 0.85 + 0.15 * hash(vec2(r * 8.0, ang * 4.0));
  float t = clamp((r - 3.0) / 7.5, 0.0, 1.0);
  vec3 hot = vec3(1.0, 0.92, 0.75);
  vec3 mid = vec3(1.0, 0.55, 0.18);
  vec3 cool = vec3(0.55, 0.12, 0.02);
  vec3 c = mix(cool, mid, smoothstep(0.0, 0.45, t));
  c = mix(c, hot, smoothstep(0.45, 1.0, t));
  return c * rings * grain;
}

void main() {
  float aspect = uResolution.x / max(uResolution.y, 1.0);
  vec2 uv = (vUv - 0.5) * 2.0;
  uv.x *= aspect;
  float scale = 11.0;
  float elev = uElevation;
  vec3 camPos = vec3(0.0, -32.0 * cos(elev), 32.0 * sin(elev));
  vec3 forward = normalize(-camPos);
  vec3 right = normalize(cross(forward, vec3(0.0, 0.0, 1.0)));
  vec3 up = cross(right, forward);
  vec3 pos = camPos + right * (uv.x * scale) + up * (uv.y * scale);
  vec3 vel = forward;

  float h2 = length(cross(pos, vel));
  h2 *= h2;

  vec3 color = vec3(0.0);
  float hits = 0.0;
  const int MAX_STEPS = 240;

  for (int i = 0; i < MAX_STEPS; i++) {
    float r2 = dot(pos, pos);
    float r = sqrt(r2);
    if (r < 1.001) break;
    if (r > 46.0 && dot(pos, vel) > 0.0) break;

    float dt = min(r * 0.055, 1.6);
    vec3 a = accel(pos, h2);
    vec3 next = pos + vel * dt + a * dt * dt * 0.5;
    vec3 a2 = accel(next, h2);
    vec3 nextVel = vel + (a + a2) * dt * 0.5;

    // disk in xy (z = 0): interpolate plane crossing
    if (pos.z * next.z < 0.0) {
      float t = pos.z / (pos.z - next.z);
      vec3 hit = mix(pos, next, t);
      float hr = length(hit.xy);
      if (hr >= 3.0 && hr <= 10.5 && hits < 2.5) {
        float ang = atan(hit.y, hit.x) + uRotation;
        float Omega = sqrt(0.5 / max(hr * hr * hr, 1e-6));
        float Laxis = hit.x * nextVel.y - hit.y * nextVel.x;
        float E = max(length(nextVel), 1e-4);
        float ell = (Laxis / E) * (1.0 + uAsymmetry * 0.35);
        float g = sqrt(max(1.0 - 1.5 / hr, 0.0)) / max(1.0 + Omega * ell, 0.15);
        g = clamp(g, 0.15, 2.5);
        float bright = pow(g, 1.15);
        float weight = hits < 0.5 ? 1.0 : 0.35;
        color += diskColor(hr, ang) * bright * weight;
        hits += 1.0;
      }
    }

    pos = next;
    vel = nextVel;
  }

  // faint starfield when nothing hit
  if (hits < 0.5) {
    float s = step(0.997, hash(floor(uv * 90.0)));
    color += vec3(s) * 0.55;
  }

  color *= uExposure;
  fragColor = vec4(color, 1.0);
}`;

const BLIT_FRAG = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform sampler2D uTex;
uniform float uBloom;
uniform int uMode; // 0 copy, 1 extract, 2 blurH, 3 blurV, 4 composite
uniform vec2 uTexel;
uniform sampler2D uBloomTex;

void main() {
  vec4 c = texture(uTex, vUv);
  if (uMode == 0) { fragColor = c; return; }
  if (uMode == 1) {
    float lum = dot(c.rgb, vec3(0.2126, 0.7152, 0.0722));
    fragColor = vec4(c.rgb * smoothstep(0.55, 1.1, lum), 1.0);
    return;
  }
  if (uMode == 2 || uMode == 3) {
    vec2 dir = uMode == 2 ? vec2(uTexel.x, 0.0) : vec2(0.0, uTexel.y);
    vec3 sum = texture(uTex, vUv).rgb * 0.227027;
    sum += texture(uTex, vUv + dir * 1.384615).rgb * 0.316216;
    sum += texture(uTex, vUv - dir * 1.384615).rgb * 0.316216;
    sum += texture(uTex, vUv + dir * 3.230769).rgb * 0.070270;
    sum += texture(uTex, vUv - dir * 3.230769).rgb * 0.070270;
    fragColor = vec4(sum, 1.0);
    return;
  }
  // composite
  vec3 bloom = texture(uBloomTex, vUv).rgb;
  vec3 outc = c.rgb + bloom * uBloom;
  // soft tonemap
  outc = outc / (1.0 + outc * 0.35);
  fragColor = vec4(outc, 1.0);
}`;

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

export interface BlackHoleVisualizer {
  resize(width: number, height: number): void;
  render(drive: BlackHoleDrive): void;
  destroy(): void;
}

/** Half-res geodesic pass + extract + two separable blur passes + composite. */
export function createBlackHoleVisualizer(canvas: HTMLCanvasElement): BlackHoleVisualizer {
  const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, powerPreference: 'high-performance' });
  if (!gl) throw new Error('WebGL2 is not available');

  const sceneProg = link(gl, VERT, SCENE_FRAG);
  const blitProg = link(gl, VERT, BLIT_FRAG);
  const vao = gl.createVertexArray()!;
  gl.bindVertexArray(vao);

  let dispW = 0;
  let dispH = 0;
  let innerW = 0;
  let innerH = 0;
  let scene = makeTarget(gl, 1, 1);
  let ping = makeTarget(gl, 1, 1);
  let pong = makeTarget(gl, 1, 1);

  const sceneLoc = {
    res: gl.getUniformLocation(sceneProg, 'uResolution'),
    elev: gl.getUniformLocation(sceneProg, 'uElevation'),
    rot: gl.getUniformLocation(sceneProg, 'uRotation'),
    exp: gl.getUniformLocation(sceneProg, 'uExposure'),
    asym: gl.getUniformLocation(sceneProg, 'uAsymmetry'),
  };
  const blitLoc = {
    tex: gl.getUniformLocation(blitProg, 'uTex'),
    bloom: gl.getUniformLocation(blitProg, 'uBloom'),
    mode: gl.getUniformLocation(blitProg, 'uMode'),
    texel: gl.getUniformLocation(blitProg, 'uTexel'),
    bloomTex: gl.getUniformLocation(blitProg, 'uBloomTex'),
  };

  const destroyTargets = () => {
    for (const t of [scene, ping, pong]) {
      gl.deleteFramebuffer(t.fbo);
      gl.deleteTexture(t.tex);
    }
  };

  const resizeInner = (w: number, h: number) => {
    const capped = Math.min(w, 640);
    const nw = Math.max(1, Math.round(capped * 0.5));
    const nh = Math.max(1, Math.round((h * capped) / Math.max(w, 1) * 0.5));
    if (nw === innerW && nh === innerH) return;
    destroyTargets();
    innerW = nw;
    innerH = nh;
    scene = makeTarget(gl, nw, nh);
    ping = makeTarget(gl, nw, nh);
    pong = makeTarget(gl, nw, nh);
  };

  const drawQuad = () => {
    gl.bindVertexArray(vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };

  return {
    resize(width: number, height: number) {
      dispW = Math.max(1, Math.round(width));
      dispH = Math.max(1, Math.round(height));
      const ratio = window.devicePixelRatio || 1;
      canvas.width = Math.round(dispW * ratio);
      canvas.height = Math.round(dispH * ratio);
      resizeInner(dispW, dispH);
    },
    render(drive: BlackHoleDrive) {
      if (!innerW || !innerH) return;

      // 1. geodesic scene
      gl.bindFramebuffer(gl.FRAMEBUFFER, scene.fbo);
      gl.viewport(0, 0, innerW, innerH);
      gl.useProgram(sceneProg);
      gl.uniform2f(sceneLoc.res, innerW, innerH);
      gl.uniform1f(sceneLoc.elev, drive.elevation);
      gl.uniform1f(sceneLoc.rot, drive.rotation);
      gl.uniform1f(sceneLoc.exp, drive.exposure);
      gl.uniform1f(sceneLoc.asym, drive.asymmetry);
      drawQuad();

      // 2. extract highlights → ping
      gl.bindFramebuffer(gl.FRAMEBUFFER, ping.fbo);
      gl.useProgram(blitProg);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, scene.tex);
      gl.uniform1i(blitLoc.tex, 0);
      gl.uniform1i(blitLoc.mode, 1);
      gl.uniform1f(blitLoc.bloom, drive.bloom);
      gl.uniform2f(blitLoc.texel, 1 / innerW, 1 / innerH);
      drawQuad();

      // 3. blur H → pong
      gl.bindFramebuffer(gl.FRAMEBUFFER, pong.fbo);
      gl.bindTexture(gl.TEXTURE_2D, ping.tex);
      gl.uniform1i(blitLoc.mode, 2);
      drawQuad();

      // 4. blur V → ping
      gl.bindFramebuffer(gl.FRAMEBUFFER, ping.fbo);
      gl.bindTexture(gl.TEXTURE_2D, pong.tex);
      gl.uniform1i(blitLoc.mode, 3);
      drawQuad();

      // 5. composite to canvas
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, scene.tex);
      gl.uniform1i(blitLoc.tex, 0);
      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, ping.tex);
      gl.uniform1i(blitLoc.bloomTex, 1);
      gl.uniform1i(blitLoc.mode, 4);
      gl.uniform1f(blitLoc.bloom, drive.bloom);
      drawQuad();
    },
    destroy() {
      destroyTargets();
      gl.deleteProgram(sceneProg);
      gl.deleteProgram(blitProg);
      gl.deleteVertexArray(vao);
      // same canvas is compiled again after a dev remount; losing the context fails that compile
    },
  };
}
