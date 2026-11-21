use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};

use futures_util::{Stream, StreamExt};

/// Extracts the documented choices[].delta.audio.data payloads and stops at
/// the provider's terminal marker, even when the HTTP stream stays open.
pub async fn read_audio_sse(
    stream: impl Stream<Item = reqwest::Result<axum::body::Bytes>>,
) -> Result<Vec<u8>> {
    let mut stream = std::pin::pin!(stream);
    let mut decoder = AudioSse::default();
    while let Some(chunk) = stream.next().await {
        if decoder.push(&chunk?)? {
            return decoder.finish();
        }
    }
    // The previous decoder also accepted a final data line without a newline.
    if !decoder.pending.is_empty() {
        decoder.done = AudioSse::line(&decoder.pending, &mut decoder.encoded)?;
    }
    decoder.finish()
}

#[derive(Default)]
struct AudioSse {
    pending: Vec<u8>,
    encoded: String,
    received: usize,
    done: bool,
}

impl AudioSse {
    fn push(&mut self, bytes: &[u8]) -> Result<bool> {
        const MAX_STREAM_BYTES: usize = 128 * 1024 * 1024;
        for fragment in bytes.split_inclusive(|byte| *byte == b'\n') {
            if fragment.len() > MAX_STREAM_BYTES.saturating_sub(self.received) {
                bail!("OpenRouter music stream is too large");
            }
            self.received += fragment.len();
            if let Some(line) = fragment.strip_suffix(b"\n") {
                self.pending.extend_from_slice(line);
                self.done = Self::line(&self.pending, &mut self.encoded)?;
                self.pending.clear();
                if self.done {
                    return Ok(true);
                }
            } else {
                self.pending.extend_from_slice(fragment);
            }
        }
        Ok(false)
    }

    fn line(line: &[u8], encoded: &mut String) -> Result<bool> {
        let line = std::str::from_utf8(line).context("OpenRouter music stream is not UTF-8 SSE")?;
        let Some(data) = line.trim_end_matches('\r').strip_prefix("data:") else {
            return Ok(false);
        };
        let data = data.trim_start();
        if data == "[DONE]" {
            return Ok(true);
        }
        let event: serde_json::Value = serde_json::from_str(data)
            .context("invalid JSON event in OpenRouter music stream")?;
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
        Ok(false)
    }

    fn finish(self) -> Result<Vec<u8>> {
        if !self.done { bail!("OpenRouter music stream ended before [DONE]"); }
        if self.encoded.is_empty() { bail!("OpenRouter music stream contained no delta.audio.data"); }
        STANDARD.decode(self.encoded).context("OpenRouter delta.audio.data is not valid base64")
    }
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

    fn decode_audio_sse(body: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = AudioSse::default();
        decoder.push(body)?;
        decoder.finish()
    }

    async fn read_loopback_sse(
        wav: &[u8],
        marker: bool,
    ) -> Result<Vec<u8>> {
        use axum::{body::{Body, Bytes}, http::header, response::IntoResponse, routing::get, Router};
        use futures_util::stream;
        use std::{convert::Infallible, time::Duration};

        let encoded = STANDARD.encode(wav);
        let mut body = String::new();
        for chunk in encoded.as_bytes().chunks(11) {
            let payload = serde_json::json!({
                "choices": [{ "delta": { "audio": { "data": std::str::from_utf8(chunk).unwrap() } } }]
            });
            body.push_str(&format!("data: {payload}\r\n\r\n"));
        }
        if marker {
            body.push_str("data: [DONE]\r\n\r\n");
        }
        let pieces: Vec<_> = body.as_bytes().chunks(7).map(Bytes::copy_from_slice).collect();
        let router = Router::new().route("/", get(move || {
            let pieces = pieces.clone();
            async move {
                let fragmented = stream::iter(pieces.into_iter().map(Ok::<_, Infallible>));
                let body = if marker {
                    Body::from_stream(fragmented.chain(stream::pending()))
                } else {
                    Body::from_stream(fragmented)
                };
                ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move { axum::serve(listener, router).await });
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            let response = reqwest::Client::builder().no_proxy().build()?.get(url).send().await?;
            read_audio_sse(response.bytes_stream()).await
        }).await;
        server.abort();
        result.context("the HTTP connection stayed open past [DONE]")?
    }

    #[tokio::test]
    async fn fragmented_wav_returns_at_done_without_http_eof() {
        let mut audio = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::new(&mut audio, spec).unwrap();
        writer.write_sample(1_i16).unwrap();
        writer.finalize().unwrap();
        let wav = audio.into_inner();

        let decoded = read_loopback_sse(&wav, true).await.unwrap();
        assert_eq!(decoded, wav);
        assert_eq!(wav_duration(&decoded).unwrap(), 1.0 / 8000.0);
        let error = read_loopback_sse(&wav, false).await.unwrap_err();
        assert!(error.to_string().contains("ended before [DONE]"), "{error}");
    }

    #[test]
    fn joins_audio_data_across_sse_events() {
        let body = b"data: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"SU\"}}}]}\n\ndata: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"Qz\"}}}]}\n\ndata: [DONE]\n";
        let mut decoder = AudioSse::default();
        for byte in body.chunks(1) {
            if decoder.push(byte).unwrap() {
                break;
            }
        }
        assert_eq!(decoder.finish().unwrap(), b"ID3");
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
