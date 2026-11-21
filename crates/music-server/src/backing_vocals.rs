use anyhow::{Result, ensure};
use std::path::Path;

use crate::audio_pcm;

/// Keep the backing's rate, channels, length and level. Only the added vocal
/// is attenuated when the two signals would otherwise clip.
pub(super) fn mix(backing: &Path, voice: Vec<u8>) -> Result<Vec<u8>> {
    let mut backing = audio_pcm::decode_stereo(backing)?;
    let voice = audio_pcm::decode_stereo_bytes(voice, "wav")?.resampled(backing.rate)?;
    add_voice(&mut backing, &voice)?;
    let work = tempfile::tempdir()?;
    let output = work.path().join("backing-with-vocals.wav");
    audio_pcm::write_wav_f32(&output, &backing)?;
    Ok(std::fs::read(output)?)
}

fn add_voice(backing: &mut audio_post::Stereo, voice: &audio_post::Stereo) -> Result<()> {
    ensure!(backing.rate == voice.rate && backing.frames() > 0, "The backing track has no usable audio.");
    let frames = backing.frames();
    // A small decoder/model boundary difference can drop only an inaudible
    // tail. A sung tail beyond the recording needs a revised score.
    let excess = [voice.left.get(frames..), voice.right.get(frames..)];
    ensure!(excess.into_iter().flatten().flatten().all(|sample| sample.abs() <= 0.001),
        "The singing extends past the backing track. Shorten the vocal score and try again.");
    let mut gain = 1.0_f32;
    let mut peak = 0.0_f32;
    for (source, added) in [(&backing.left, &voice.left), (&backing.right, &voice.right)] {
        ensure!(source.iter().all(|sample| sample.is_finite() && sample.abs() <= 1.0),
            "The backing has invalid PCM or exceeds full scale.");
        for (&original, &vocal) in source.iter().zip(added) {
            ensure!(vocal.is_finite(), "The extracted vocal contains invalid PCM.");
            peak = peak.max(vocal.abs());
            if vocal > 0.0 {
                gain = gain.min((1.0 - original) / vocal);
            } else if vocal < 0.0 {
                gain = gain.min((-1.0 - original) / vocal);
            }
        }
    }
    ensure!(peak * gain > 0.001, "No audible vocal fits over the backing at its original level.");
    for (destination, added) in [(&mut backing.left, &voice.left), (&mut backing.right, &voice.right)] {
        for (sample, vocal) in destination.iter_mut().zip(added) {
            if *vocal != 0.0 {
                *sample += gain * vocal;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(left: Vec<f32>, right: Vec<f32>) -> audio_post::Stereo {
        audio_post::Stereo { rate: 48_000, left, right }
    }

    #[test]
    fn the_backing_keeps_its_grid_and_unaccompanied_samples_with_vocal_attenuated() {
        let mut backing = stereo(vec![0.25, 0.75, 0.5], vec![0.1, -0.5, -0.3]);
        let voice = stereo(vec![0.0, 0.5], vec![0.0, -0.5]);
        add_voice(&mut backing, &voice).unwrap();
        assert_eq!(backing.left, [0.25, 1.0, 0.5]);
        assert_eq!(backing.right, [0.1, -0.75, -0.3]);
    }

    #[test]
    fn an_audible_overrun_fails_before_any_audio_is_changed() {
        let mut backing = stereo(vec![0.25], vec![0.1]);
        let voice = stereo(vec![0.5, 0.5], vec![0.0, 0.0]);
        assert!(add_voice(&mut backing, &voice).is_err());
        assert_eq!(backing.left, [0.25]);
    }
}
