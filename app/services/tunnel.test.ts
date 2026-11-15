import { describe, expect, it } from 'vitest';
import { flameVelocity, hopSource, ncsBars, noteHit, sourceBounds, sprayBirths, tunnelDrive, NCS_BARS, type Hop } from './tunnel';

const RATE = 44100;
const FFT = 2048;
const bins = FFT / 2;

function spectrum(fill: (i: number, hz: number) => number): Uint8Array {
  const data = new Uint8Array(bins);
  const binHz = RATE / FFT;
  for (let i = 0; i < bins; i++) data[i] = Math.max(0, Math.min(255, Math.round(fill(i, i * binHz))));
  return data;
}

describe('ncsBars', () => {
  it('reads the same bins as the classic NCS circle', () => {
    const data = spectrum(() => 0);
    data[10] = 255;
    data[20] = 128;
    const bars = ncsBars(data);
    expect(bars).toHaveLength(NCS_BARS);
    expect(bars[0]).toBeCloseTo(1);
    expect(bars[10]).toBeCloseTo(128 / 255);
    expect(bars[11]).toBe(0);
  });

  it('follows a new spectrum without dropping the previous bar in one frame', () => {
    const loud = spectrum(() => 0);
    loud[10] = 255;
    const first = tunnelDrive({ spectrum: loud, sampleRate: RATE, fftSize: FFT, time: 0 });
    const second = tunnelDrive({ spectrum: spectrum(() => 0), sampleRate: RATE, fftSize: FFT, time: 0, prev: first });
    expect(first.bars[0]).toBeCloseTo(1);
    expect(second.bars[0]).toBeGreaterThan(0.3);
    expect(second.bars[0]).toBeLessThan(0.6);
  });
});

describe('tunnelDrive', () => {
  it('raises smoke spawn when the bass climbs', () => {
    const quiet = tunnelDrive({ spectrum: spectrum(() => 10), sampleRate: RATE, fftSize: FFT, time: 0 });
    const loud = tunnelDrive({ spectrum: spectrum((_i, hz) => (hz < 150 ? 220 : 10)), sampleRate: RATE, fftSize: FFT, time: 0 });
    expect(loud.smokeSpawn).toBeGreaterThan(quiet.smokeSpawn);
    expect(loud.ringBright).toBeGreaterThan(quiet.ringBright);
    expect(loud.bass).toBeGreaterThan(quiet.bass);
  });

  it('raises spark spawn when the treble climbs', () => {
    const quiet = tunnelDrive({ spectrum: spectrum(() => 10), sampleRate: RATE, fftSize: FFT, time: 0 });
    const bright = tunnelDrive({
      spectrum: spectrum((_i, hz) => (hz > 2500 && hz < 8000 ? 220 : 10)),
      sampleRate: RATE,
      fftSize: FFT,
      time: 0,
    });
    expect(bright.sparkSpawn).toBeGreaterThan(quiet.sparkSpawn);
    expect(bright.treb).toBeGreaterThan(quiet.treb);
  });

  it('raises jump and shrinks zoom when the bass climbs', () => {
    const quiet = tunnelDrive({ spectrum: spectrum(() => 10), sampleRate: RATE, fftSize: FFT, time: 0 });
    const loud = tunnelDrive({ spectrum: spectrum((_i, hz) => (hz < 150 ? 220 : 10)), sampleRate: RATE, fftSize: FFT, time: 0 });
    expect(loud.jump).toBeGreaterThan(quiet.jump);
    expect(loud.zoom).toBeLessThan(quiet.zoom);
    expect(loud.zoom).toBeGreaterThanOrEqual(0.96);
    expect(loud.zoom).toBeLessThanOrEqual(0.985);
  });

  it('hears a new note in one bin and ignores a held spectrum', () => {
    const quiet = spectrum(() => 12);
    const note = spectrum((_i, hz) => (hz > 300 && hz < 380 ? 160 : 12));
    expect(noteHit(note, quiet, RATE, FFT)).toBeGreaterThan(0.5);
    expect(noteHit(note, note, RATE, FFT)).toBe(0);
    const flicker = spectrum((_i, hz) => (hz > 300 && hz < 380 ? 18 : 12));
    expect(noteHit(flicker, quiet, RATE, FFT)).toBe(0);
    const driven = tunnelDrive({ spectrum: note, prevSpectrum: quiet, sampleRate: RATE, fftSize: FFT, time: 0 });
    const held = tunnelDrive({ spectrum: note, prevSpectrum: note, sampleRate: RATE, fftSize: FFT, time: 0, prev: driven });
    expect(driven.hit).toBeGreaterThan(0.5);
    expect(held.hit).toBe(0);
  });

  it('keeps each hop inside wide, square and tall frames', () => {
    const views = [
      { aspect: 1.6, w: 768, h: 480 },
      { aspect: 1, w: 400, h: 400 },
      { aspect: 0.6, w: 300, h: 500 },
    ];
    for (const view of views) {
      const pointPx = 24;
      const bounds = sourceBounds(view.aspect, pointPx, view.w, view.h);
      const hop: Hop = { x: 0, y: 0, tx: 0, ty: 0, cool: 0, hx: 1, hy: 0 };
      for (let step = 0; step < 48; step++) {
        const hit = step % 12 === 0 ? 0.8 : 0;
        const p = hopSource(hop, hit, 1 / 60, bounds, () => (step * 17 % 100) / 100);
        expect(Math.abs((p.x * 2) / view.aspect)).toBeLessThanOrEqual(1 - pointPx / view.w + 1e-6);
        expect(Math.abs(p.y * 2)).toBeLessThanOrEqual(1 - pointPx / view.h + 1e-6);
      }
    }
  });

  it('jumps once per note, in both axes, and a harder note lands farther', () => {
    const bounds = sourceBounds(1.6, 16, 640, 400);
    const land = (hit: number) => {
      const hop: Hop = { x: 0, y: 0, tx: 0, ty: 0, cool: 0, hx: 1, hy: 0 };
      let draws = 0;
      const counted = () => {
        draws += 1;
        return 0.9;
      };
      hopSource(hop, hit, 1 / 60, bounds, counted);
      for (let i = 0; i < 20; i++) hopSource(hop, 0, 1 / 60, bounds, counted);
      return { hop, draws };
    };
    const loud = land(1);
    const soft = land(0.55);
    expect(loud.draws).toBe(2);
    expect(soft.draws).toBe(2);
    expect(Math.abs(loud.hop.x)).toBeGreaterThan(bounds.maxX * 0.15);
    expect(Math.abs(loud.hop.x)).toBeLessThan(bounds.maxX * 0.35);
    expect(Math.abs(loud.hop.y)).toBeGreaterThan(bounds.maxY * 0.15);
    expect(Math.abs(loud.hop.y)).toBeLessThan(bounds.maxY * 0.35);
    expect(Math.hypot(soft.hop.tx, soft.hop.ty)).toBeLessThan(Math.hypot(loud.hop.tx, loud.hop.ty));
  });

  it('stays put on a held tone and hops again on the next attack', () => {
    const bounds = sourceBounds(1.6, 16, 640, 400);
    const hop: Hop = { x: 0, y: 0, tx: 0, ty: 0, cool: 0, hx: 1, hy: 0 };
    const spots = [0.15, 0.8, 0.95, 0.05];
    let n = 0;
    const rand = () => spots[n++];
    hopSource(hop, 0.9, 1 / 60, bounds, rand);
    const held = { x: hop.x, y: hop.y };
    const draws = n;
    for (let i = 0; i < 20; i++) hopSource(hop, 0.2, 1 / 60, bounds, rand);
    expect(n).toBe(draws);
    expect(hop.x).toBeCloseTo(held.x, 5);
    expect(hop.y).toBeCloseTo(held.y, 5);
    hopSource(hop, 0.9, 1 / 60, bounds, rand);
    expect(Math.hypot(hop.x - held.x, hop.y - held.y)).toBeGreaterThan(bounds.maxX * 0.3);
  });

  it('jumps only above the threshold it is given', () => {
    const bounds = sourceBounds(1.6, 16, 640, 400);
    const quiet: Hop = { x: 0, y: 0, tx: 0, ty: 0, cool: 0, hx: 1, hy: 0 };
    const loud: Hop = { x: 0, y: 0, tx: 0, ty: 0, cool: 0, hx: 1, hy: 0 };
    expect(hopSource(quiet, 0.5, 1 / 60, bounds, () => 0.2, 0.9).jumped).toBe(false);
    expect(hopSource(loud, 0.5, 1 / 60, bounds, () => 0.2, 0.4).jumped).toBe(true);
  });

  it('stays put below the jump threshold and keeps the flame aimed back', () => {
    const bounds = sourceBounds(1.6, 16, 640, 400);
    const hop: Hop = { x: 0, y: 0, tx: 0, ty: 0, cool: 0, hx: 1, hy: 0 };
    let draws = 0;
    for (let i = 0; i < 30; i++) hopSource(hop, 0.3, 1 / 60, bounds, () => {
      draws += 1;
      return 0.5;
    });
    expect(draws).toBe(0);
    expect(hop.x).toBe(0);
    expect(hop.y).toBe(0);
    expect(hop.hx).toBe(0);
    expect(hop.hy).toBe(-1);
    const side: Hop = { x: 0.2, y: 0, tx: 0, ty: 0, cool: 0, hx: 0, hy: -1 };
    hopSource(side, 0.2, 1 / 60, bounds);
    expect(side.x).toBeCloseTo(0.2, 5);
    expect(side.y).toBeCloseTo(0, 5);
    expect(side.hx).toBeCloseTo(1, 5);
    expect(side.hy).toBeCloseTo(0, 5);
  });

  it('sheds a cloud, and a note sheds more', () => {
    expect(sprayBirths(0, 0)).toBeGreaterThanOrEqual(16);
    expect(sprayBirths(1, 1)).toBeGreaterThan(sprayBirths(0, 0));
  });

  it('sends a flame up, and a jump pulls the tail backward', () => {
    const up = flameVelocity(0, 1, 0.04, 0.2);
    expect(up.vy).toBeGreaterThan(Math.abs(up.vx));
    expect(up.vy).toBeGreaterThan(0);
    const back = flameVelocity(-0.6, 0.2, 0, 0.3);
    expect(back.vx).toBeLessThan(0);
    expect(Math.abs(back.vx)).toBeGreaterThan(Math.abs(back.vy));
  });

  it('advances rotation with time', () => {
    const a = tunnelDrive({ spectrum: spectrum(() => 0), sampleRate: RATE, fftSize: FFT, time: 0 });
    const b = tunnelDrive({ spectrum: spectrum(() => 0), sampleRate: RATE, fftSize: FFT, time: 10 });
    expect(b.rotation).toBeGreaterThan(a.rotation);
  });
});
