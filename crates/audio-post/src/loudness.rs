//! Loudness and true peak as ITU-R BS.1770-4 and EBU R 128 define them.
//!
//! Integrated loudness: both channels K-weighted (a high shelf and a high pass,
//! their coefficients worked out for the track's own sample rate from the
//! analogue prototypes the reference implementations use), mean square over
//! 400 ms blocks overlapping by 75 %, an absolute gate at -70 LUFS and a
//! relative gate 10 LU under the loudness of what passed it.
//!
//! True peak: the signal oversampled four times at 44.1 and 48 kHz (the
//! standard's minimum), twice at 88.2 and 96 kHz, by a Kaiser-windowed sinc
//! interpolator, and the largest magnitude of the result. An estimate, as the
//! standard says every true-peak meter is; close to full scale it can read a
//! little under the analogue peak.

use crate::Stereo;

/// A biquad in direct form I, in double precision.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    fn run(&self, input: &[f32]) -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        input
            .iter()
            .map(|&sample| {
                let x = sample as f64;
                let y = self.b[0] * x + self.b[1] * x1 + self.b[2] * x2 - self.a[0] * y1 - self.a[1] * y2;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                y
            })
            .collect()
    }

    fn run_f64(&self, input: &[f64]) -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        input
            .iter()
            .map(|&x| {
                let y = self.b[0] * x + self.b[1] * x1 + self.b[2] * x2 - self.a[0] * y1 - self.a[1] * y2;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                y
            })
            .collect()
    }
}

/// The two K-weighting stages for a sample rate (the prototypes libebur128 uses;
/// at 48 kHz they give BS.1770's published coefficients).
fn k_weighting(rate: u32) -> (Biquad, Biquad) {
    let rate = rate as f64;
    // stage 1: the head's high shelf, about +4 dB above 1.5 kHz
    let (f0, gain_db, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
    };
    // stage 2: the RLB high pass at 38 Hz
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (std::f64::consts::PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let high_pass = Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0] };
    (shelf, high_pass)
}

/// Mean square of each 400 ms block, 100 ms apart, both channels summed (weight 1 each).
fn block_powers(audio: &Stereo) -> Vec<f64> {
    let (shelf, high_pass) = k_weighting(audio.rate);
    let left = high_pass.run_f64(&shelf.run(&audio.left));
    let right = high_pass.run_f64(&shelf.run(&audio.right));
    let block = (audio.rate as f64 * 0.4).round() as usize;
    let step = (audio.rate as f64 * 0.1).round() as usize;
    if block == 0 || left.len() < block {
        return Vec::new();
    }
    // running sums of squares, so every block costs two subtractions
    let mut sums = Vec::with_capacity(left.len() + 1);
    sums.push(0.0f64);
    for (l, r) in left.iter().zip(&right) {
        let last = *sums.last().expect("seeded");
        sums.push(last + l * l + r * r);
    }
    (0..=(left.len() - block) / step).map(|index| (sums[index * step + block] - sums[index * step]) / block as f64).collect()
}

fn lufs(power: f64) -> f64 {
    -0.691 + 10.0 * power.log10()
}

/// Integrated loudness in LUFS; none for a track under 400 ms or quieter than the absolute gate throughout.
pub fn integrated(audio: &Stereo) -> Option<f64> {
    let powers: Vec<f64> = block_powers(audio).into_iter().filter(|power| *power > 0.0 && lufs(*power) > -70.0).collect();
    if powers.is_empty() {
        return None;
    }
    let relative = lufs(powers.iter().sum::<f64>() / powers.len() as f64) - 10.0;
    let gated: Vec<f64> = powers.into_iter().filter(|power| lufs(*power) > relative).collect();
    if gated.is_empty() {
        return None;
    }
    Some(lufs(gated.iter().sum::<f64>() / gated.len() as f64))
}

/// The largest sample magnitude, in dBFS.
pub fn sample_peak_db(audio: &Stereo) -> f64 {
    to_db(audio.peak() as f64)
}

fn to_db(value: f64) -> f64 {
    if value > 0.0 { 20.0 * value.log10() } else { f64::NEG_INFINITY }
}

/// Taps per phase of the interpolator: 12 on each side of the sample.
const HALF: usize = 12;

/// The oversampling factor true peak is measured at: four times at 44.1 and 48 kHz, as BS.1770 asks, twice at 88.2/96 kHz, none above.
fn oversampling(rate: u32) -> usize {
    match rate {
        0..=50_000 => 4,
        50_001..=100_000 => 2,
        _ => 1,
    }
}

/// The interpolating filter for one fractional position `phase / factor` between samples: a Kaiser-windowed sinc.
fn phase_taps(phase: usize, factor: usize) -> Vec<f64> {
    let offset = phase as f64 / factor as f64;
    let span = (2 * HALF) as f64;
    let beta = 8.0;
    let bessel = |x: f64| {
        // I0 by its series
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..40 {
            term *= (x / 2.0) * (x / 2.0) / (k as f64 * k as f64);
            sum += term;
        }
        sum
    };
    (0..2 * HALF)
        .map(|tap| {
            let t = tap as f64 - (HALF as f64 - 1.0) - offset;
            let sinc = if t.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
            let position = (t + HALF as f64) / span; // 0..1 across the window
            let window = bessel(beta * (1.0 - (2.0 * position - 1.0).powi(2)).max(0.0).sqrt()) / bessel(beta);
            sinc * window
        })
        .collect()
}

fn channel_true_peak(samples: &[f32], factor: usize) -> f64 {
    let mut peak = samples.iter().fold(0.0f64, |peak, sample| peak.max(sample.abs() as f64));
    if factor == 1 {
        return peak;
    }
    let phases: Vec<Vec<f64>> = (1..factor).map(|phase| phase_taps(phase, factor)).collect();
    let at = |index: isize| if index < 0 || index as usize >= samples.len() { 0.0 } else { samples[index as usize] as f64 };
    for n in 0..samples.len() as isize {
        for taps in &phases {
            let mut value = 0.0;
            for (tap, weight) in taps.iter().enumerate() {
                value += weight * at(n + tap as isize - (HALF as isize - 1));
            }
            peak = peak.max(value.abs());
        }
    }
    peak
}

/// True peak in dBTP.
pub fn true_peak_db(audio: &Stereo) -> f64 {
    let factor = oversampling(audio.rate);
    to_db(channel_true_peak(&audio.left, factor).max(channel_true_peak(&audio.right, factor)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EBU Tech 3341's tones: a 1 kHz sine in both channels at the given level in dBFS for the given seconds.
    fn tones(rate: u32, parts: &[(f64, f64)]) -> Stereo {
        let mut samples = Vec::new();
        let mut phase = 0.0f64;
        for &(db, seconds) in parts {
            let amplitude = 10f64.powf(db / 20.0);
            for _ in 0..(seconds * rate as f64).round() as usize {
                samples.push((amplitude * (2.0 * std::f64::consts::PI * phase).sin()) as f32);
                phase = (phase + 1000.0 / rate as f64).fract();
            }
        }
        Stereo::new(samples.clone(), samples, rate)
    }

    fn assert_near(measured: Option<f64>, expected: f64, tolerance: f64) {
        let measured = measured.expect("a loudness");
        assert!((measured - expected).abs() <= tolerance, "measured {measured:.3}, expected {expected} ± {tolerance}");
    }

    #[test]
    fn ebu_3341_case_1_and_2_steady_tones() {
        for rate in [44_100, 48_000] {
            assert_near(integrated(&tones(rate, &[(-23.0, 20.0)])), -23.0, 0.1);
            assert_near(integrated(&tones(rate, &[(-33.0, 20.0)])), -33.0, 0.1);
        }
    }

    #[test]
    fn ebu_3341_case_3_to_5_gating() {
        let rate = 48_000;
        assert_near(integrated(&tones(rate, &[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)])), -23.0, 0.1);
        assert_near(integrated(&tones(rate, &[(-72.0, 10.0), (-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0), (-72.0, 10.0)])), -23.0, 0.1);
        assert_near(integrated(&tones(rate, &[(-26.0, 20.0), (-20.0, 20.1), (-26.0, 20.0)])), -23.0, 0.1);
    }

    #[test]
    fn silence_and_short_clips_have_no_loudness() {
        assert!(integrated(&Stereo::new(vec![0.0; 48_000], vec![0.0; 48_000], 48_000)).is_none());
        assert!(integrated(&tones(48_000, &[(-23.0, 0.3)])).is_none());
    }

    #[test]
    fn true_peak_finds_the_peak_between_samples() {
        // a sine at a quarter of the rate, sampled 45 degrees off its crests: every sample sits at 0.707 of the real peak
        let rate = 48_000;
        let amplitude = 0.5f64;
        let samples: Vec<f32> = (0..rate as usize).map(|n| (amplitude * (std::f64::consts::FRAC_PI_2 * n as f64 + std::f64::consts::FRAC_PI_4).sin()) as f32).collect();
        let audio = Stereo::new(samples.clone(), samples, rate);
        let expected = 20.0 * amplitude.log10();
        assert!((sample_peak_db(&audio) - (expected - 3.01)).abs() < 0.05);
        let measured = true_peak_db(&audio);
        assert!((measured - expected).abs() < 0.2, "true peak {measured:.3} dBTP, expected {expected:.3}");
        // and a slow sine reads the same as its sample peak
        let slow = tones(rate, &[(-6.0, 1.0)]);
        assert!((true_peak_db(&slow) - (-6.0)).abs() < 0.05);
    }
}
