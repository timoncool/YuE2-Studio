use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};

/// Extracts and decodes the documented `choices[].delta.audio.data` SSE payloads.
/// The caller supplies the complete byte stream after transport has finished.
pub fn decode_audio_sse(body: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(body).context("OpenRouter music stream is not UTF-8 SSE")?;
    let mut encoded = String::new();
    let mut saw_done = false;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else { continue; };
        let data = data.trim_start();
        if data == "[DONE]" { saw_done = true; break; }
        let event: serde_json::Value = serde_json::from_str(data).context("invalid JSON event in OpenRouter music stream")?;
        if event.get("error").is_some_and(|error| !error.is_null())
            || event.pointer("/choices/0/finish_reason").and_then(serde_json::Value::as_str) == Some("error")
        {
            let message = event.pointer("/error/message").and_then(serde_json::Value::as_str)
                .unwrap_or("the provider stopped before completing the audio");
            bail!("OpenRouter music generation failed: {message}");
        }
        if let Some(chunk) = event.pointer("/choices/0/delta/audio/data").and_then(serde_json::Value::as_str) {
            encoded.push_str(chunk);
        }
    }
    if !saw_done { bail!("OpenRouter music stream ended before [DONE]"); }
    if encoded.is_empty() { bail!("OpenRouter music stream contained no delta.audio.data"); }
    STANDARD.decode(encoded).context("OpenRouter delta.audio.data is not valid base64")
}

/// Validate the full PCM payload before publishing a library song. A correct
/// header can still declare more frames than the provider actually delivered.
pub fn wav_duration(audio: &[u8]) -> Result<f64> {
    let mut reader = hound::WavReader::new(audio).context("invalid OpenRouter WAV header")?;
    let spec = reader.spec();
    if spec.sample_rate == 0 || reader.duration() == 0 {
        bail!("OpenRouter returned an empty WAV or invalid sample rate");
    }
    let duration = f64::from(reader.duration()) / f64::from(spec.sample_rate);
    match spec.sample_format {
        hound::SampleFormat::Int => reader.samples::<i32>().try_for_each(|sample| sample.map(drop)),
        hound::SampleFormat::Float => reader.samples::<f32>().try_for_each(|sample| sample.map(drop)),
    }.context("invalid OpenRouter WAV samples")?;
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn joins_audio_data_across_sse_events() {
        let body = b"data: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"SU\"}}}]}\n\ndata: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"Qz\"}}}]}\n\ndata: [DONE]\n";
        assert_eq!(decode_audio_sse(body).unwrap(), b"ID3");
    }
    #[test]
    fn rejects_provider_failure_after_audio() {
        for failure in [
            r#"{"error":{"message":"stream interrupted"}}"#,
            r#"{"choices":[{"finish_reason":"error"}]}"#,
        ] {
            let body = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"audio\":{{\"data\":\"UklGRg==\"}}}}}}]}}\n\ndata: {failure}\n\ndata: [DONE]\n"
            );
            assert!(decode_audio_sse(body.as_bytes()).unwrap_err().to_string().contains("generation failed"));
        }
    }
    #[test]
    fn requires_complete_nonempty_wav_frames() {
        let mut audio = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::new(&mut audio, spec).unwrap();
        writer.write_sample(1_i16).unwrap();
        writer.finalize().unwrap();
        let complete = audio.into_inner();
        assert_eq!(wav_duration(&complete).unwrap(), 1.0 / 8000.0);
        assert!(wav_duration(&complete[..complete.len()-1]).is_err());
        assert!(wav_duration(b"RIFF\x04\0\0\0WAVE").is_err());
    }
    #[test]
    fn rejects_incomplete_streams() { assert!(decode_audio_sse(b"data: {}\n").is_err()); }
}
