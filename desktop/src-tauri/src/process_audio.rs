use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAME_BYTES: usize = CHANNELS as usize * size_of::<f32>();
pub const MAX_SECONDS: u64 = 30;

pub fn capture(pid: u32, stop: &AtomicBool, timeout: Duration) -> Result<Vec<u8>, String> {
    use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};

    wasapi::initialize_mta()
        .ok()
        .map_err(|error| error.to_string())?;
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            wasapi::deinitialize();
        }
    }
    let _apartment = Apartment;
    let format = WaveFormat::new(
        32,
        32,
        SampleType::Float,
        SAMPLE_RATE as usize,
        CHANNELS as usize,
        None,
    );
    let mut client = AudioClient::new_application_loopback_client(pid, true).map_err(|error| {
        format!("Cannot record this window. Windows build 20348 or later is required for process audio: {error}")
    })?;
    client
        .initialize_client(
            &format,
            Direction::Capture,
            StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 0,
            },
            None,
        )
        .map_err(|error| error.to_string())?;
    let event = client
        .set_get_eventhandle()
        .map_err(|error| error.to_string())?;
    let reader = client
        .get_audiocaptureclient()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + timeout;
    let mut pcm = Vec::new();
    let mut packet = Vec::new();
    let max_bytes = SAMPLE_RATE as usize * MAX_SECONDS as usize * FRAME_BYTES;
    client.start_stream().map_err(|error| error.to_string())?;
    let result = (|| {
        while !stop.load(Ordering::Acquire) && Instant::now() < deadline && pcm.len() < max_bytes {
            let frames = reader
                .get_next_packet_size()
                .map_err(|error| error.to_string())?
                .unwrap_or(0) as usize;
            if frames == 0 {
                // An idle audio process may send no packets. Wake to check Stop in any case.
                let _ = event.wait_for_event(50);
                continue;
            }
            if frames > SAMPLE_RATE as usize {
                return Err("Windows returned an invalid capture buffer size".into());
            }
            packet.resize(frames * FRAME_BYTES, 0);
            let (read, _) = reader
                .read_from_device(&mut packet)
                .map_err(|error| error.to_string())?;
            let usable = (read as usize * FRAME_BYTES).min(max_bytes - pcm.len());
            pcm.extend_from_slice(&packet[..usable]);
        }
        encode_wav(&pcm)
    })();
    let _ = client.stop_stream();
    result
}

fn encode_wav(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut wav = Vec::new();
    let spec = hound::WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::new(Cursor::new(&mut wav), spec).map_err(|error| error.to_string())?;
    let mut audible = false;
    for bytes in raw.chunks_exact(size_of::<f32>()) {
        let value = f32::from_le_bytes(bytes.try_into().unwrap());
        let sample = (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        audible |= sample != 0;
        writer
            .write_sample(sample)
            .map_err(|error| error.to_string())?;
    }
    if !audible {
        return Err("No audio was recorded from the selected window".into());
    }
    writer.finalize().map_err(|error| error.to_string())?;
    Ok(wav)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_sensible_samples_and_refuses_silence() {
        assert!(encode_wav(&[0; FRAME_BYTES]).is_err());
        let samples = [0.125_f32, -0.125]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let wav = encode_wav(&samples).unwrap();
        let mut reader = hound::WavReader::new(Cursor::new(wav)).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(
            reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [4096, -4096]
        );
    }
}
