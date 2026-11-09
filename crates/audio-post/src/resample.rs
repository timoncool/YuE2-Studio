//! Band-limited sample rate conversion for whole tracks.

use anyhow::{Context, Result};
use audioadapter::Adapter;
use audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Indexing, Resampler};

/// One channel, band-limited: what lies above the new Nyquist is filtered
/// out instead of folding back into the band as noise.
pub fn mono(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>> {
    let input = vec![samples.iter().map(|&s| s as f64).collect::<Vec<_>>()];
    let adapter = SequentialSliceOfVecs::new(&input, 1, samples.len()).context("wrap the audio for resampling")?;
    let mut resampler = Fft::<f64>::new(from as usize, to as usize, 1024, 1, FixedSync::Both).context("build the resampler")?;
    let output = resampler.process_all(&adapter, samples.len(), None).context("resample the audio")?;
    Ok((0..output.frames()).map(|frame| output.read_sample(0, frame).unwrap_or(0.0) as f32).collect())
}

/// `mono` piece by piece: the same chunks through the same resampler give the
/// same frames, but only one chunk is held at a time, so an hours-long track
/// costs no more memory than a second of it. `each` gets the frames in order.
pub fn mono_pieces(samples: &[f32], from: u32, to: u32, mut each: impl FnMut(&[f32]) -> Result<()>) -> Result<()> {
    let mut resampler = Fft::<f64>::new(from as usize, to as usize, 1024, 1, FixedSync::Both).context("build the resampler")?;
    let expected = (resampler.resample_ratio() * samples.len() as f64).ceil() as usize;
    let mut to_trim = resampler.output_delay();
    let chunk = resampler.input_frames_next();
    let mut input = vec![vec![0.0f64; chunk]];
    let out_len = resampler.output_frames_max();
    let mut output = vec![vec![0.0f64; out_len]];
    let mut piece: Vec<f32> = Vec::with_capacity(out_len);
    let (mut at, mut sent) = (0usize, 0usize);
    while sent < expected {
        let left = samples.len() - at;
        let take = left.min(chunk);
        for (slot, sample) in input[0].iter_mut().zip(&samples[at..at + take]) {
            *slot = *sample as f64;
        }
        // the whole clip's last chunk is read as partial even when it is full, and then silence pushes the delay out
        let indexing = Indexing { partial_len: (left <= chunk).then_some(take), ..Default::default() };
        let adapter = SequentialSliceOfVecs::new(&input, 1, chunk).context("wrap the audio for resampling")?;
        let mut out = SequentialSliceOfVecs::new_mut(&mut output, 1, out_len).context("wrap the resampled audio")?;
        let (_, produced) = resampler.process_into_buffer(&adapter, &mut out, Some(&indexing)).context("resample the audio")?;
        at += take;
        let skip = to_trim.min(produced);
        to_trim -= skip;
        let keep = (produced - skip).min(expected - sent);
        piece.clear();
        piece.extend(output[0][skip..skip + keep].iter().map(|&s| s as f32));
        sent += keep;
        if !piece.is_empty() {
            each(&piece)?;
        }
    }
    Ok(())
}

pub fn stereo(left: &[f32], right: &[f32], from: u32, to: u32) -> Result<(Vec<f32>, Vec<f32>)> {
    let frames = left.len().min(right.len());
    let input = vec![left[..frames].iter().map(|&s| s as f64).collect::<Vec<_>>(), right[..frames].iter().map(|&s| s as f64).collect()];
    let adapter = SequentialSliceOfVecs::new(&input, 2, frames).context("wrap the audio for resampling")?;
    let mut resampler = Fft::<f64>::new(from as usize, to as usize, 1024, 2, FixedSync::Both).context("build the resampler")?;
    let output = resampler.process_all(&adapter, frames, None).context("resample the audio")?;
    let produced = output.frames();
    let mut left = Vec::with_capacity(produced);
    let mut right = Vec::with_capacity(produced);
    for frame in 0..produced {
        left.push(output.read_sample(0, frame).unwrap_or(0.0) as f32);
        right.push(output.read_sample(1, frame).unwrap_or(0.0) as f32);
    }
    Ok((left, right))
}

#[cfg(test)]
mod tests {
    #[test]
    fn piece_by_piece_is_the_whole_track_resampled_at_once() {
        for (from, to, n) in [(24_000, 48_000, 2048 * 3), (24_000, 44_100, 51_239), (48_000, 24_000, 999), (24_000, 48_000, 0)] {
            let x: Vec<f32> = (0..n).map(|i| (i as f32 * 0.031).sin() * 0.8).collect();
            let whole = super::mono(&x, from, to).unwrap();
            let mut pieces = Vec::new();
            super::mono_pieces(&x, from, to, |p| {
                pieces.extend_from_slice(p);
                Ok(())
            })
            .unwrap();
            assert_eq!(pieces.len(), whole.len(), "{from} -> {to}, {n} frames");
            assert!(pieces == whole, "{from} -> {to}, {n} frames");
        }
    }
}
