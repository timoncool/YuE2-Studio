//! Exact byte-token boundaries from the loaded checkpoint, mapped to Unicode letters.
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use crate::{api_error, ApiError, AppState};

#[derive(Deserialize)]
pub struct Request {
    #[serde(default)] style: String,
    #[serde(default)] lyrics: String,
    #[serde(default = "full")] cot: String,
}
fn full() -> String { "full".into() }
#[derive(Deserialize)]
struct Piece { id: i32, start: usize, end: usize, bytes: Vec<u8> }
#[derive(Deserialize)]
struct Encoded { text: String, lyrics: String, lyrics_start_bytes: usize, lyrics_end_bytes: usize, pieces: Vec<Piece> }

fn mapped(encoded: Encoded, raw: &str) -> Result<Value, String> {
    let body = raw.trim();
    if encoded.lyrics != body { return Ok(json!({ "available": false, "problem": "The checkpoint normalizes these letters to NFC. Token cuts cannot be drawn on the text as typed." })); }
    let start = encoded.lyrics_start_bytes;
    let end = encoded.lyrics_end_bytes;
    if start > end || encoded.text.get(start..end) != Some(body) { return Err("Invalid lyric span from checkpoint tokenizer".into()); }
    let mut position = 0;
    for piece in &encoded.pieces {
        if piece.start != position || piece.end != position + piece.bytes.len() || encoded.text.as_bytes().get(piece.start..piece.end) != Some(piece.bytes.as_slice()) { return Err("Checkpoint token bytes do not reproduce the prompt".into()); }
        position = piece.end;
    }
    if position != encoded.text.len() { return Err("Incomplete checkpoint token stream".into()); }
    let lead = raw[..raw.len() - raw.trim_start().len()].chars().count();
    let mut boundaries: Vec<usize> = body.char_indices().map(|(at, _)| at).collect();
    boundaries.push(body.len());
    let mut cuts = BTreeSet::new();
    let mut torn = BTreeSet::new();
    let mut spans = Vec::new();
    for piece in encoded.pieces {
        if piece.start < end && piece.end > start {
            let a = piece.start.saturating_sub(start);
            let b = piece.end.min(end) - start;
            let from = boundaries.partition_point(|&at| at <= a).saturating_sub(1);
            let to = boundaries.partition_point(|&at| at < b);
            spans.push(json!({ "id": piece.id, "start": lead + from, "end": lead + to }));
        }
        if piece.start > start && piece.start < end {
            let byte = piece.start - start;
            let next = boundaries.partition_point(|&at| at < byte);
            if boundaries.get(next) != Some(&byte) { torn.insert(lead + next.saturating_sub(1)); }
            if next < boundaries.len() - 1 { cuts.insert(lead + next); }
        }
    }
    Ok(json!({ "available": true, "cuts": cuts, "torn": torn, "tokens": spans.len(), "spans": spans }))
}

pub async fn tokenize(State(state): State<AppState>, Json(request): Json<Request>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if request.style.len() + request.lyrics.len() > 65536 || !matches!(request.cot.as_str(), "full" | "melody" | "off") { return Err(api_error(StatusCode::BAD_REQUEST, "Invalid cot or text longer than 65536 bytes".into())); }
    if !state.music_server.health().await { return Ok(Json(json!({ "available": false, "problem": "Token cuts appear when the music engine is running with its checkpoint vocabulary. The editor works without token cuts." }))); }
    let props = state.music_server.props().await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    if props.get("tokenize_text").and_then(Value::as_bool) != Some(true) { return Ok(Json(json!({ "available": false, "problem": "This music engine does not expose its checkpoint tokenizer. Update the engine to show exact token cuts." }))); }
    let response = state.music_server.http.post(state.music_server.url("/tokenize-text")).json(&json!({ "style": request.style.trim(), "lyrics": request.lyrics.trim(), "cot": request.cot })).timeout(std::time::Duration::from_secs(10)).send().await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    if !response.status().is_success() { return Err(api_error(StatusCode::BAD_GATEWAY, format!("Checkpoint tokenizer refused the request ({})", response.status()))); }
    let encoded: Encoded = response.json().await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    mapped(encoded, &request.lyrics).map(Json).map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encoded(text: &str, cuts: &[usize]) -> Encoded {
        let mut edges = vec![0]; edges.extend(cuts); edges.push(text.len());
        Encoded { text: text.into(), lyrics: text.into(), lyrics_start_bytes: 0, lyrics_end_bytes: text.len(), pieces: edges.windows(2).enumerate().map(|(id, span)| Piece { id: id as i32, start: span[0], end: span[1], bytes: text.as_bytes()[span[0]..span[1]].to_vec() }).collect() }
    }
    #[test] fn maps_unicode_and_torn_emoji_with_actual_counts() {
        let result = mapped(encoded("а😀б", &[2, 4, 6]), "  а😀б ").unwrap();
        assert_eq!(result["cuts"], json!([3, 4])); assert_eq!(result["torn"], json!([3])); assert_eq!(result["tokens"], 4);
        assert_eq!(result["spans"][1]["start"], 3); assert_eq!(result["spans"][2]["start"], 3);
    }
    #[test] fn rejects_fabricated_bytes_and_normalized_raw_mapping() {
        let mut bad = encoded("one", &[]); bad.pieces[0].bytes = b"two".to_vec(); assert!(mapped(bad, "one").is_err());
        assert_eq!(mapped(encoded("é", &[]), "e\u{301}").unwrap()["available"], false);
    }
}
