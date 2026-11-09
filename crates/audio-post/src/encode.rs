//! Turning a finished track into the file the library keeps.
//!
//! Engines hand over their audio unencoded, at the rate and precision the model
//! produced, and it is kept at the level it came: lossless FLAC by default
//! (libFLAC, the reference encoder), MP3 by LAME, the reference
//! MP3 encoder, when one is asked for. One encoder per format for every
//! engine, never one engine's own.

use anyhow::{anyhow, bail, Result};
use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, Quality};

use crate::Stereo;

/// Scales the track so its loudest part reaches full scale: the level that
/// all but `peak_clip` samples per million stay under becomes 1.0, and those
/// few are clipped. A processing stage the person chooses, never part of
/// keeping a song.
pub fn normalize_peak(audio: &mut Stereo, peak_clip: u32) {
    let peak_clip = peak_clip.min(999) as f64;
    let mut magnitudes: Vec<f32> = audio.left.iter().chain(&audio.right).map(|sample| sample.abs()).collect();
    if magnitudes.is_empty() {
        return;
    }
    let index = ((magnitudes.len() - 1) as f64 * (1.0 - peak_clip / 1_000_000.0)) as usize;
    let (_, reference, _) = magnitudes.select_nth_unstable_by(index, |a, b| a.total_cmp(b));
    let reference = *reference;
    if reference < 1e-6 {
        return;
    }
    let gain = 1.0 / reference;
    for sample in audio.left.iter_mut().chain(audio.right.iter_mut()) {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

/// The gain that fits a float signal into an integer format: 1 when its peak
/// is within full scale, else exactly enough to bring the peak to it. An
/// integer sample cannot hold more, and cutting the overs would distort.
pub fn fitting_gain(audio: &Stereo) -> f32 {
    let peak = audio.left.iter().chain(&audio.right).filter(|sample| sample.is_finite()).fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    if peak > 1.0 { 1.0 / peak } else { 1.0 }
}

/// Lossless FLAC at 24 bits, written by libFLAC, the reference encoder: the
/// model's float output with nothing taken away but the rounding below the
/// 24th bit, about -144 dB. Verify mode decodes every frame as it is written
/// and compares it with the input. The stream is built in memory - libFLAC
/// never opens a temp file, which is where imports failed on machines whose
/// `%TEMP%` is full, read-only, redirected, swept mid-encode by a cleaner, or
/// named in a code page libFLAC's own `fopen` cannot take. libFLAC cannot seek
/// a write sink, so it leaves STREAMINFO's sample count zero; the real count is
/// written in afterwards, because players and the library read the length there.
pub fn flac(audio: &Stereo) -> Result<Vec<u8>> {
    let frames = audio.frames();
    let gain = fitting_gain(audio);
    let samples: Vec<i32> = audio.left.iter().zip(&audio.right).flat_map(|(left, right)| [*left, *right]).map(|sample| flac_sample(sample, gain)).collect();
    let mut buffer: Vec<u8> = Vec::new();
    flac_frames(audio.rate, frames, &mut buffer, |encoder| {
        if encoder.process_interleaved(&samples, frames as u32).is_err() {
            bail!("encode FLAC: {:?}", encoder.state());
        }
        Ok(())
    })?;
    set_flac_total_samples(&mut buffer, frames as u64);
    Ok(buffer)
}

const FLAC_FULL: f32 = 8_388_607.0;

fn flac_sample(sample: f32, gain: f32) -> i32 {
    ((sample * gain).clamp(-1.0, 1.0) * FLAC_FULL).round() as i32
}

/// The 24-bit stereo libFLAC encoder `flac` writes with, its frames fed by `feed`.
fn flac_frames(rate: u32, frames: usize, sink: &mut dyn std::io::Write, feed: impl FnOnce(&mut flac_bound::FlacEncoder) -> Result<()>) -> Result<()> {
    use flac_bound::{FlacEncoder, WriteWrapper};
    let mut sink = WriteWrapper(sink);
    let mut encoder = FlacEncoder::new()
        .ok_or_else(|| anyhow!("libFLAC could not make an encoder"))?
        .channels(2)
        .bits_per_sample(24)
        .sample_rate(rate)
        .compression_level(8)
        .verify(true)
        .total_samples_estimate(frames as u64)
        .init_write(&mut sink)
        .map_err(|error| anyhow!("start the FLAC encoder: {error:?}"))?;
    feed(&mut encoder)?;
    encoder.finish().map_err(|encoder| anyhow!("finish the FLAC file: {:?}", encoder.state()))?;
    Ok(())
}

/// The pieces of a mono track at `out_rate`, as `Stereo::resampled` makes each
/// of its channels, one piece at a time.
fn mono_pieces(samples: &[f32], rate: u32, out_rate: u32, each: impl FnMut(&[f32]) -> Result<()>) -> Result<()> {
    if rate == out_rate {
        return samples.chunks(1 << 16).try_for_each(each);
    }
    crate::resample::mono_pieces(samples, rate, out_rate, each)
}

/// `fitting_gain` and the frame count of a mono track at `out_rate`, read
/// piece by piece.
fn mono_fit(samples: &[f32], rate: u32, out_rate: u32) -> Result<(f32, usize)> {
    let (mut peak, mut frames) = (0.0f32, 0usize);
    mono_pieces(samples, rate, out_rate, |piece| {
        peak = piece.iter().filter(|sample| sample.is_finite()).fold(peak, |peak, sample| peak.max(sample.abs()));
        frames += piece.len();
        Ok(())
    })?;
    Ok((if peak > 1.0 { 1.0 / peak } else { 1.0 }, frames))
}

/// `flac` of a mono track on both channels at `out_rate` - the file
/// `flac(&Stereo::new(x.clone(), x, rate).resampled(out_rate)?)` makes - written
/// into `sink` from where it stands, piece by piece: an audiobook of many hours
/// takes the memory of a piece, not of the track twice over. The track is read
/// twice, once for its peak.
pub fn flac_dual_mono<W: std::io::Read + std::io::Write + std::io::Seek>(samples: &[f32], rate: u32, out_rate: u32, sink: &mut W) -> Result<()> {
    use std::io::{SeekFrom, Write};
    let (gain, frames) = mono_fit(samples, rate, out_rate)?;
    let start = sink.stream_position()?;
    let mut both: Vec<i32> = Vec::new();
    let mut buffered = std::io::BufWriter::with_capacity(1 << 20, &mut *sink);
    flac_frames(out_rate, frames, &mut buffered, |encoder| {
        mono_pieces(samples, rate, out_rate, |piece| {
            both.clear();
            both.extend(piece.iter().flat_map(|sample| {
                let sample = flac_sample(*sample, gain);
                [sample, sample]
            }));
            if encoder.process_interleaved(&both, piece.len() as u32).is_err() {
                bail!("encode FLAC: {:?}", encoder.state());
            }
            Ok(())
        })
    })?;
    buffered.flush()?;
    drop(buffered);
    let end = sink.stream_position()?;
    let mut head = [0u8; 26];
    sink.seek(SeekFrom::Start(start))?;
    sink.read_exact(&mut head)?;
    set_flac_total_samples(&mut head, frames as u64);
    sink.seek(SeekFrom::Start(start))?;
    sink.write_all(&head)?;
    sink.seek(SeekFrom::Start(end))?;
    Ok(())
}

/// Writes the frame count into STREAMINFO's 36-bit total-samples field, which a
/// non-seekable write leaves zero. The field is in the 8 bytes at offset 18 -
/// after `fLaC`, the metadata-block header, and the block and frame sizes - and
/// shares them with the sample rate, channels and bit depth in the top 28 bits.
fn set_flac_total_samples(flac: &mut [u8], frames: u64) {
    const OFFSET: usize = 18;
    const MASK: u64 = (1u64 << 36) - 1;
    if flac.len() < OFFSET + 8 || &flac[0..4] != b"fLaC" {
        return;
    }
    let field = u64::from_be_bytes(flac[OFFSET..OFFSET + 8].try_into().unwrap());
    let field = (field & !MASK) | (frames & MASK);
    flac[OFFSET..OFFSET + 8].copy_from_slice(&field.to_be_bytes());
}

/// The MPEG-1 Layer III rates LAME encodes at, the highest first.
const BITRATES: &[(u32, Bitrate)] = &[
    (320, Bitrate::Kbps320),
    (256, Bitrate::Kbps256),
    (224, Bitrate::Kbps224),
    (192, Bitrate::Kbps192),
    (160, Bitrate::Kbps160),
    (128, Bitrate::Kbps128),
    (112, Bitrate::Kbps112),
    (96, Bitrate::Kbps96),
    (80, Bitrate::Kbps80),
    (64, Bitrate::Kbps64),
    (48, Bitrate::Kbps48),
    (40, Bitrate::Kbps40),
    (32, Bitrate::Kbps32),
];

/// Linear fades at both ends, in seconds; zero leaves that end as it is.
pub fn fade(audio: &mut Stereo, fade_in: f32, fade_out: f32) {
    let frames = audio.frames();
    let rate = audio.rate as f32;
    let fade_in = ((fade_in.max(0.0) * rate) as usize).min(frames);
    let fade_out = ((fade_out.max(0.0) * rate) as usize).min(frames);
    for i in 0..fade_in {
        let gain = i as f32 / fade_in as f32;
        audio.left[i] *= gain;
        audio.right[i] *= gain;
    }
    for i in 0..fade_out {
        let gain = i as f32 / fade_out as f32;
        let at = frames - 1 - i;
        audio.left[at] *= gain;
        audio.right[at] *= gain;
    }
}

/// The standard rate a requested bitrate becomes: the highest one not above it.
pub fn mp3_bitrate(kbps: u32) -> u32 {
    BITRATES.iter().find(|(rate, _)| *rate <= kbps).map_or(32, |(rate, _)| *rate)
}

/// Encodes stereo to constant-bitrate MP3 at the track's own sample rate,
/// which must be one MPEG-1 carries (32, 44.1 or 48 kHz): nothing is
/// resampled on the way.
pub fn mp3(audio: &Stereo, kbps: u32) -> Result<Vec<u8>> {
    let mut encoder = lame(audio.rate, kbps)?;
    let gain = fitting_gain(audio);
    let fitted;
    let audio = if gain < 1.0 {
        fitted = Stereo { left: audio.left.iter().map(|sample| sample * gain).collect(), right: audio.right.iter().map(|sample| sample * gain).collect(), rate: audio.rate };
        &fitted
    } else {
        audio
    };

    const PIECE: usize = 1 << 16;
    let mut out = Vec::with_capacity(audio.frames() * kbps as usize / 8 / audio.rate as usize * 1000 + 8192);
    for start in (0..audio.frames()).step_by(PIECE) {
        let end = (start + PIECE).min(audio.frames());
        out.reserve(mp3lame_encoder::max_required_buffer_size(end - start));
        encoder
            .encode_to_vec(DualPcm { left: &audio.left[start..end], right: &audio.right[start..end] }, &mut out)
            .map_err(|error| anyhow!("LAME encode: {error}"))?;
    }
    out.reserve(8192);
    encoder.flush_to_vec::<FlushNoGap>(&mut out).map_err(|error| anyhow!("LAME flush: {error}"))?;
    Ok(out)
}

/// The constant-bitrate stereo LAME encoder `mp3` writes with.
fn lame(rate: u32, kbps: u32) -> Result<mp3lame_encoder::Encoder> {
    if !matches!(rate, 32_000 | 44_100 | 48_000) {
        bail!("MP3 carries 32, 44.1 or 48 kHz; this track is {rate} Hz");
    }
    let bitrate = BITRATES.iter().find(|(standard, _)| *standard == mp3_bitrate(kbps)).map(|(_, bitrate)| *bitrate).expect("listed");
    let mut builder = Builder::new().ok_or_else(|| anyhow!("LAME could not start"))?;
    builder.set_num_channels(2).map_err(|error| anyhow!("LAME channels: {error}"))?;
    builder.set_sample_rate(rate).map_err(|error| anyhow!("LAME sample rate: {error}"))?;
    builder.set_brate(bitrate).map_err(|error| anyhow!("LAME bitrate: {error}"))?;
    builder.set_quality(Quality::Best).map_err(|error| anyhow!("LAME quality: {error}"))?;
    builder.set_to_write_vbr_tag(false).map_err(|error| anyhow!("LAME tag: {error}"))?;
    builder.build().map_err(|error| anyhow!("LAME: {error}"))
}

/// `mp3` of a mono track on both channels at `out_rate` - the stream
/// `mp3(&Stereo::new(x.clone(), x, rate).resampled(out_rate)?, kbps)` makes -
/// written into `sink` piece by piece: an audiobook of many hours takes the
/// memory of a piece, not of the track several times over. The track is read
/// twice, once for its peak.
pub fn mp3_dual_mono(samples: &[f32], rate: u32, out_rate: u32, kbps: u32, sink: &mut dyn std::io::Write) -> Result<()> {
    let mut encoder = lame(out_rate, kbps)?;
    let (gain, _) = mono_fit(samples, rate, out_rate)?;
    let mut fitted: Vec<f32> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    mono_pieces(samples, rate, out_rate, |piece| {
        let piece = if gain < 1.0 {
            fitted.clear();
            fitted.extend(piece.iter().map(|sample| sample * gain));
            &fitted[..]
        } else {
            piece
        };
        out.clear();
        out.reserve(mp3lame_encoder::max_required_buffer_size(piece.len()));
        encoder.encode_to_vec(DualPcm { left: piece, right: piece }, &mut out).map_err(|error| anyhow!("LAME encode: {error}"))?;
        sink.write_all(&out)?;
        Ok(())
    })?;
    out.clear();
    out.reserve(8192);
    encoder.flush_to_vec::<FlushNoGap>(&mut out).map_err(|error| anyhow!("LAME flush: {error}"))?;
    sink.write_all(&out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, rate: u32, amplitude: f32) -> Stereo {
        let n = (seconds * rate as f32) as usize;
        let left: Vec<f32> = (0..n).map(|i| amplitude * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin()).collect();
        Stereo::new(left.clone(), left, rate)
    }


    #[test]
    fn normalizing_brings_the_peak_to_full_scale() {
        let mut audio = tone(1.0, 48_000, 0.25);
        normalize_peak(&mut audio, 10);
        let peak = audio.left.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!((peak - 1.0).abs() < 1e-3, "peak {peak}");
    }

    #[test]
    fn bitrates_round_down_to_a_standard_one() {
        assert_eq!(mp3_bitrate(320), 320);
        assert_eq!(mp3_bitrate(300), 256);
        assert_eq!(mp3_bitrate(128), 128);
        assert_eq!(mp3_bitrate(10), 32);
    }

    #[test]
    fn lame_writes_a_stream_of_the_asked_size() {
        let audio = tone(3.0, 48_000, 0.5);
        let bytes = mp3(&audio, 320).unwrap();
        // 320 kbps for three seconds, give or take the padding frames
        let expected = 320_000 / 8 * 3;
        assert!(bytes.len() > expected * 9 / 10 && bytes.len() < expected * 12 / 10, "{} bytes", bytes.len());
        assert_eq!(&bytes[..2], &[0xFF, 0xFB], "an MPEG-1 Layer III frame header");
        assert!(mp3(&tone(1.0, 22_050, 0.5), 128).is_err());
    }

    /// A mono voice track, louder than full scale in places so the fitting gain
    /// is at work, and not a whole number of the resampler's chunks long.
    fn voice(seconds: f32, rate: u32) -> Vec<f32> {
        let n = (seconds * rate as f32) as usize + 77;
        (0..n).map(|i| 1.3 * (2.0 * std::f32::consts::PI * 210.0 * i as f32 / rate as f32).sin() * (i as f32 / 997.0).sin()).collect()
    }

    #[test]
    fn the_dual_mono_writers_make_the_files_the_whole_track_encoders_make() {
        let x = voice(2.5, 24_000);
        for out_rate in [24_000, 48_000] {
            let whole = flac(&Stereo::new(x.clone(), x.clone(), 24_000).resampled(out_rate).unwrap()).unwrap();
            let mut streamed = std::io::Cursor::new(Vec::new());
            flac_dual_mono(&x, 24_000, out_rate, &mut streamed).unwrap();
            assert!(streamed.into_inner() == whole, "FLAC at {out_rate} Hz differs");
        }
        for out_rate in [44_100, 48_000] {
            let whole = mp3(&Stereo::new(x.clone(), x.clone(), 24_000).resampled(out_rate).unwrap(), 192).unwrap();
            let mut streamed = Vec::new();
            mp3_dual_mono(&x, 24_000, out_rate, 192, &mut streamed).unwrap();
            assert!(streamed == whole, "MP3 at {out_rate} Hz differs");
        }
        let quiet: Vec<f32> = x.iter().map(|s| s * 0.5).collect();
        let whole = mp3(&Stereo::new(quiet.clone(), quiet.clone(), 48_000), 128).unwrap();
        let mut streamed = Vec::new();
        mp3_dual_mono(&quiet, 48_000, 48_000, 128, &mut streamed).unwrap();
        assert!(streamed == whole, "MP3 without resampling or gain differs");
        assert!(mp3_dual_mono(&quiet, 24_000, 22_050, 128, &mut Vec::new()).is_err());
    }

    #[test]
    fn a_flac_written_after_other_bytes_gets_its_length_in_its_own_header() {
        let x = voice(1.0, 24_000);
        let whole = flac(&Stereo::new(x.clone(), x.clone(), 24_000)).unwrap();
        let mut sink = std::io::Cursor::new(b"ID3 head".to_vec());
        sink.set_position(8);
        flac_dual_mono(&x, 24_000, 24_000, &mut sink).unwrap();
        let bytes = sink.into_inner();
        assert_eq!(&bytes[..8], b"ID3 head");
        assert!(bytes[8..] == whole[..]);
    }
}
