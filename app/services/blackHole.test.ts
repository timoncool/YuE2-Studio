import { describe, expect, it } from 'vitest';
import { blackHoleDrive } from './blackHole';

const RATE = 44100;
const FFT = 2048;
const bins = FFT / 2;

function spectrum(fill: (i: number, hz: number) => number): Uint8Array {
  const data = new Uint8Array(bins);
  const binHz = RATE / FFT;
  for (let i = 0; i < bins; i++) data[i] = Math.max(0, Math.min(255, Math.round(fill(i, i * binHz))));
  return data;
}

describe('blackHoleDrive', () => {
  it('raises exposure when the bass climbs', () => {
    const quiet = blackHoleDrive({ spectrum: spectrum(() => 10), sampleRate: RATE, fftSize: FFT, time: 0 });
    const loud = blackHoleDrive({ spectrum: spectrum((_i, hz) => (hz < 150 ? 220 : 10)), sampleRate: RATE, fftSize: FFT, time: 0 });
    expect(loud.exposure).toBeGreaterThan(quiet.exposure);
    expect(loud.bloom).toBeGreaterThan(quiet.bloom);
  });

  it('raises elevation when the spectrum centroid moves up', () => {
    const low = blackHoleDrive({
      spectrum: spectrum((_i, hz) => (hz > 80 && hz < 250 ? 200 : 0)),
      sampleRate: RATE,
      fftSize: FFT,
      time: 0,
    });
    const high = blackHoleDrive({
      spectrum: spectrum((_i, hz) => (hz > 2500 && hz < 5000 ? 200 : 0)),
      sampleRate: RATE,
      fftSize: FFT,
      time: 0,
    });
    expect(high.elevation).toBeGreaterThan(low.elevation);
  });

  it('advances rotation with time', () => {
    const a = blackHoleDrive({ spectrum: spectrum(() => 0), sampleRate: RATE, fftSize: FFT, time: 0 });
    const b = blackHoleDrive({ spectrum: spectrum(() => 0), sampleRate: RATE, fftSize: FFT, time: 10 });
    expect(b.rotation).toBeGreaterThan(a.rotation);
  });
});
