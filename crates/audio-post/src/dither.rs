//! Fitting floating-point audio into integer samples.
//!
//! Rounding to fewer bits leaves an error that follows the music, heard as
//! distortion on quiet tails. TPDF dither (two uniform random values summed,
//! one step of the target format wide in all) turns that error into a steady,
//! signal-independent hiss at the format's own noise floor. It is added only
//! when bits are being taken away: samples that already sit on the target's
//! grid (a 16-bit file written again at 16 bits) are copied exactly.

use crate::Stereo;

/// Interleaved integer samples at `bits` (16 or 24), full scale being
/// `2^(bits-1)`, the scale decoders divide by, so an untouched integer
/// source comes back sample for sample. Overs are clipped at full scale;
/// callers fit the level first. The seed makes the dither repeatable.
pub fn quantize(audio: &Stereo, bits: u32, dither: bool, seed: u64) -> Vec<i32> {
    assert!((8..=24).contains(&bits), "integer formats of 8 to 24 bits");
    let full = (1i64 << (bits - 1)) as f64;
    let (low, high) = (-full, full - 1.0);
    let mut state = seed | 1;
    let mut uniform = move || {
        // xorshift64*: plenty for dither, and the same file every time
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let mut out = Vec::with_capacity(audio.frames() * 2);
    for (left, right) in audio.left.iter().zip(&audio.right) {
        for sample in [*left, *right] {
            let mut value = sample as f64 * full;
            if dither {
                value += uniform() + uniform();
            }
            out.push(value.round().clamp(low, high) as i32);
        }
    }
    out
}

/// Whether every sample already lies on the grid of a `bits`-bit integer format.
pub fn on_grid(audio: &Stereo, bits: u32) -> bool {
    let full = (1i64 << (bits - 1)) as f64;
    audio.left.iter().chain(&audio.right).all(|sample| {
        let value = *sample as f64 * full;
        value == value.round() && value >= -full && value < full
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sixteen_bit(values: &[i16]) -> Stereo {
        let samples: Vec<f32> = values.iter().map(|value| *value as f32 / 32768.0).collect();
        Stereo::new(samples.clone(), samples, 48_000)
    }

    #[test]
    fn samples_already_on_the_grid_come_back_exactly_without_dither() {
        let audio = sixteen_bit(&[0, 1, -1, 12345, -32768, 32767]);
        assert!(on_grid(&audio, 16));
        let out = quantize(&audio, 16, false, 1);
        let left: Vec<i32> = out.iter().step_by(2).copied().collect();
        assert_eq!(left, vec![0, 1, -1, 12345, -32768, 32767]);
        // and widening to 24 bits only shifts them
        assert!(on_grid(&audio, 24));
        assert_eq!(quantize(&audio, 24, false, 1)[6], 12345 << 8);
    }

    #[test]
    fn dither_is_triangular_two_steps_wide_and_repeatable() {
        let audio = Stereo::new(vec![0.0; 100_000], vec![0.0; 100_000], 48_000);
        assert!(on_grid(&audio, 16));
        let a = quantize(&audio, 16, true, 7);
        assert_eq!(a, quantize(&audio, 16, true, 7));
        assert!(a.iter().all(|value| (-1..=1).contains(value)));
        // TPDF of width two steps, rounded: P(0) = 3/4, P(+-1) = 1/8 each
        let zeros = a.iter().filter(|value| **value == 0).count() as f64 / a.len() as f64;
        assert!((zeros - 0.75).abs() < 0.01, "{zeros}");
        let mean = a.iter().map(|value| *value as f64).sum::<f64>() / a.len() as f64;
        assert!(mean.abs() < 0.01);
    }

    #[test]
    fn dither_removes_the_error_that_follows_a_quiet_signal() {
        // a sine 0.4 steps tall: plain rounding turns it into silence, dither keeps it on average
        let n = 200_000;
        let samples: Vec<f32> = (0..n).map(|i| (0.4 / 32768.0 * (i as f64 * 0.01).sin()) as f32).collect();
        let audio = Stereo::new(samples.clone(), samples.clone(), 48_000);
        assert!(!on_grid(&audio, 16));
        assert!(quantize(&audio, 16, false, 3).iter().all(|value| *value == 0));
        let dithered = quantize(&audio, 16, true, 3);
        let correlation: f64 = dithered.iter().step_by(2).zip(&samples).map(|(q, s)| *q as f64 * *s as f64).sum();
        assert!(correlation > 0.0);
    }

    #[test]
    fn overs_are_clipped_at_full_scale() {
        let audio = Stereo::new(vec![1.5, -1.5], vec![1.0, -1.0], 48_000);
        assert_eq!(quantize(&audio, 16, false, 1), vec![32767, 32767, -32768, -32768]);
    }
}
