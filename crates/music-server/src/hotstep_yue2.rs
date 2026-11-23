//! YuE2's native HOT-Step request and its compatible published model files.
use super::*;
use anyhow::ensure;

pub(super) fn payload(request: &mut CreateMusicJobRequest) -> anyhow::Result<Value> {
    ensure!(!request.style.trim().is_empty(), "Enter a YuE2 style prompt.");
    ensure!(
        request.adapters.is_empty() && request.cover_of.is_none() && !request.abc_continue,
        "This HOT-Step YuE2 profile accepts style, lyrics, a complete ABC score or saved semantic tokens."
    );
    let duration = *request.duration_seconds.get_or_insert(30.0);
    let steps = *request.steps.get_or_insert(8);
    let seed = *request.seed.get_or_insert(1234);
    let cot = request.cot.as_deref().unwrap_or("full");
    ensure!((1.0..=300.0).contains(&duration) && (1..=100).contains(&steps) && seed >= 0,
        "Enter a duration from 1 to 300 seconds, 1–100 steps and a nonnegative seed.");
    ensure!(matches!(cot, "off" | "melody" | "full"), "Choose a valid YuE2 planning mode.");
    let cap = (duration * 25.0).round() as u32;
    let sampling = request.semantic_sampling.as_ref();
    let max = sampling.and_then(|sampling| sampling.max_tokens).unwrap_or(cap);
    let min = sampling.and_then(|sampling| sampling.min_tokens).unwrap_or(200.min(max));
    ensure!(min <= max && (1..=cap).contains(&max), "YuE2 semantic token limits must fit the duration.");
    let mut body = serde_json::json!({
        "style": request.style, "lyrics": request.lyrics, "seed": seed, "ode_steps": steps,
        "cot": cot, "semantic_min_tokens": min, "semantic_max_tokens": max
    });
    if let Some(abc) = request.abc.as_deref().filter(|abc| !abc.trim().is_empty()) {
        ensure!(cot != "off", "ABC needs melody or full mode.");
        body["abc"] = Value::from(abc);
    }
    if let Some(tokens) = &request.semantic_tokens {
        let codes = tokens.split(',').map(|token| token.trim().parse::<u16>()).collect::<Result<Vec<_>, _>>()?;
        ensure!(codes.len() <= cap as usize && codes.iter().all(|&code| code < 32768),
            "YuE2 semantic tokens must fit the duration and the codec vocabulary.");
        body["codec_ids"] = serde_json::json!(codes);
    }
    if let Some(cfg) = request.cfg_scale {
        ensure!((0.0..=20.0).contains(&cfg), "YuE2 guidance must be between 0 and 20.");
        body["cfg_scale"] = Value::from(cfg);
    }
    if let Some(sampling) = sampling {
        for (key, value) in [
            ("semantic_temperature", serde_json::json!(sampling.temperature)),
            ("semantic_top_p", serde_json::json!(sampling.top_p)),
            ("semantic_top_k", serde_json::json!(sampling.top_k)),
            ("semantic_repetition_penalty", serde_json::json!(sampling.repetition_penalty)),
            ("semantic_penalty_window", serde_json::json!(sampling.penalty_window)),
        ] {
            if !value.is_null() {
                body[key] = value;
            }
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_short_score_and_uses_the_protocol_fields() {
        let mut request = serde_json::from_value(serde_json::json!({
            "style":"piano", "lyrics":"Hold the light", "abc":"X:1\nK:C\nCDEF|",
            "cot":"full", "steps":2, "duration_seconds":4, "seed":1221,
            "semantic_sampling":{"min_tokens":100,"max_tokens":100,"temperature":0.9}
        })).unwrap();
        let body = payload(&mut request).unwrap();
        assert_eq!(body["ode_steps"], 2);
        assert_eq!(body["semantic_min_tokens"], 100);
        assert_eq!(body["semantic_max_tokens"], 100);
        assert_eq!(body["abc"], "X:1\nK:C\nCDEF|");
        assert!(body.get("codec_ids").is_none());
    }
}
