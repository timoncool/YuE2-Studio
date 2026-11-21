use anyhow::{bail, Context, Result};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ResultTrack {
    pub replay_request: Value,
    pub audio_content_type: String,
    pub audio: Vec<u8>,
    /// The VAE latents the audio was decoded from (raw f32 [T, 64]), which
    /// the engine sends after each track's audio since its webui keeps them.
    pub latent: Option<Vec<u8>>,
}

pub fn parse_multipart_result(content_type: &str, body: &[u8]) -> Result<Vec<ResultTrack>> {
    let parts = parse_result_parts(content_type, body)?;
    let mut pending_replay = None;
    let mut tracks = Vec::new();

    for part in parts {
        let media_type = part.content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        if media_type == "application/json" {
            if pending_replay.is_some() {
                bail!("engine result has two replay parts without an audio part");
            }
            pending_replay = Some(serde_json::from_slice(&part.body).context("invalid JSON replay part")?);
        } else if media_type.starts_with("audio/") {
            let replay_request = pending_replay.take().context("audio part has no preceding replay request")?;
            tracks.push(ResultTrack { replay_request, audio_content_type: media_type, audio: part.body, latent: None });
        } else if media_type == "application/octet-stream" {
            let track = tracks.last_mut().filter(|track| pending_replay.is_none() && track.latent.is_none()).context("latent part does not follow a track's audio")?;
            track.latent = Some(part.body);
        } else {
            bail!("unsupported engine result part content type: {}", part.content_type);
        }
    }
    if pending_replay.is_some() {
        bail!("engine result ends with an unpaired replay request");
    }
    if tracks.is_empty() {
        bail!("engine result contains no audio tracks");
    }
    Ok(tracks)
}

pub fn audio_extension(content_type: &str) -> Result<&'static str> {
    match content_type {
        "audio/mpeg" => Ok("mp3"),
        "audio/wav" | "audio/x-wav" => Ok("wav"),
        other => bail!("unsupported engine audio content type: {other}"),
    }
}

pub(crate) struct Part {
    pub(crate) content_type: String,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn parse_result_parts(content_type: &str, body: &[u8]) -> Result<Vec<Part>> {
    let boundary = boundary_from_content_type(content_type)?;
    parse_parts(&boundary, body)
}

fn boundary_from_content_type(content_type: &str) -> Result<String> {
    let mut fields = content_type.split(';');
    if !fields.next().is_some_and(|value| value.trim().eq_ignore_ascii_case("multipart/mixed")) {
        bail!("engine result is not multipart/mixed");
    }
    let boundary = fields
        .find_map(|field| field.trim().strip_prefix("boundary=").or_else(|| field.trim().strip_prefix("Boundary=")))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .filter(|value| !value.is_empty())
        .context("multipart result has no boundary")?;
    if boundary.bytes().any(|byte| byte == b'\r' || byte == b'\n') {
        bail!("multipart boundary contains a line break");
    }
    Ok(boundary)
}

fn parse_parts(boundary: &str, body: &[u8]) -> Result<Vec<Part>> {
    let marker = [b"--".as_slice(), boundary.as_bytes()].concat();
    if !body.starts_with(&marker) { bail!("multipart result does not start with its boundary"); }
    let mut cursor = 0;
    let mut parts = Vec::new();
    loop {
        if !body[cursor..].starts_with(&marker) { bail!("multipart boundary is malformed"); }
        cursor += marker.len();
        if body[cursor..].starts_with(b"--\r\n") {
            cursor += 4;
            if cursor != body.len() { bail!("data follows the closing multipart boundary"); }
            break;
        }
        if !body[cursor..].starts_with(b"\r\n") { bail!("multipart boundary has no CRLF terminator"); }
        cursor += 2;
        let headers_end = find_bytes(&body[cursor..], b"\r\n\r\n").context("multipart part has no header terminator")? + cursor;
        let content_type = parse_headers(&body[cursor..headers_end])?;
        let content_start = headers_end + 4;
        let next_boundary = find_bytes(&body[content_start..], &[b"\r\n".as_slice(), marker.as_slice()].concat()).context("multipart part has no following boundary")? + content_start;
        parts.push(Part { content_type, body: body[content_start..next_boundary].to_vec() });
        cursor = next_boundary + 2;
    }
    Ok(parts)
}

fn parse_headers(raw: &[u8]) -> Result<String> {
    let headers = std::str::from_utf8(raw).context("multipart headers are not UTF-8")?;
    let mut content_type = None;
    for line in headers.split("\r\n") {
        let (name, value) = line.split_once(':').context("malformed multipart header")?;
        if name.trim().eq_ignore_ascii_case("content-type") {
            if content_type.replace(value.trim().to_owned()).is_some() { bail!("multipart part has duplicate Content-Type header"); }
        }
    }
    content_type.context("multipart part has no Content-Type header")
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upstream_json_audio_pairs_without_touching_audio_bytes() {
        let body = b"--yue2-batch-boundary\r\nContent-Type: application/json\r\n\r\n{\"style\":\"s\",\"lyrics\":\"l\",\"semantic_tokens\":\"1,2,3\",\"seed\":7}\r\n--yue2-batch-boundary\r\nContent-Type: audio/mpeg\r\n\r\nID3\x00\xff\r\n--yue2-batch-boundary--\r\n";
        let tracks = parse_multipart_result("multipart/mixed; boundary=yue2-batch-boundary", body).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].replay_request["semantic_tokens"], "1,2,3");
        assert_eq!(tracks[0].audio, b"ID3\x00\xff");
    }

    #[test]
    fn a_track_carries_the_latent_sent_after_its_audio() {
        let body = b"--b\r\nContent-Type: application/json\r\n\r\n{\"seed\":1}\r\n--b\r\nContent-Type: audio/mpeg\r\n\r\nA1\r\n--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: form-data; name=\"latent\"\r\n\r\nL1\r\n--b\r\nContent-Type: application/json\r\n\r\n{\"seed\":2}\r\n--b\r\nContent-Type: audio/mpeg\r\n\r\nA2\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\nL2\r\n--b--\r\n";
        let tracks = parse_multipart_result("multipart/mixed; boundary=b", body).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!((tracks[1].audio.as_slice(), tracks[1].latent.as_deref()), (b"A2".as_slice(), Some(b"L2".as_slice())));
        let stray = b"--b\r\nContent-Type: application/octet-stream\r\n\r\nL1\r\n--b--\r\n";
        assert!(parse_multipart_result("multipart/mixed; boundary=b", stray).is_err());
    }

    #[test]
    fn rejects_unpaired_or_unknown_parts() {
        let body = b"--b\r\nContent-Type: application/json\r\n\r\n{}\r\n--b--\r\n";
        assert!(parse_multipart_result("multipart/mixed; boundary=b", body).is_err());
    }
}
