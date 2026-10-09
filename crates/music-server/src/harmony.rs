//! Chord variety and section order of a score the model plans: the engine's
//! `harmony` field (yue2.cpp `harmony.h`), checked here so the user is told
//! which knob is wrong instead of reading a failed job.
//!
//! The planner tends to loop one progression over the whole song. The engine
//! lowers the chords heard among the recent changes while a chord symbol is
//! written, can favour a root outside the key, keeps a section from opening the
//! way the one before did, and can hold the plan to the lyrics' sections in
//! order. Every control is off at zero.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::score::phrasing;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Harmony {
    /// `root` counts C, Cmaj7 and C/E as one chord, `spelling` tells them apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Logits a chord loses for its share of the recent changes, 0 to 64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    /// Recent chord changes remembered, 1 to 512.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// Chord symbols in a row one root holds for free, 0 to 64; 0 is no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_limit: Option<u32>,
    /// Root identity: logits a change to a root outside the key gains, 0 to 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outside_bonus: Option<f64>,
    /// The share of recent changes outside the key that stops the bonus, 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outside_limit: Option<f64>,
    /// Logits taken from a chord that would repeat how the previous section opened, 0 to 64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_strength: Option<f64>,
    /// Opening chords compared between sections, 1 to 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_open: Option<u32>,
    /// Hold the plan to the lyrics' sections, in order.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub follow_lyrics: bool,
}

impl Harmony {
    /// Whether anything is asked for: a request with every control at rest changes nothing.
    pub fn active(&self) -> bool {
        self.strength.is_some_and(|value| value > 0.0)
            || self.outside_bonus.is_some_and(|value| value > 0.0)
            || self.section_strength.is_some_and(|value| value > 0.0)
            || self.follow_lyrics
    }

    pub fn validate(&self) -> Result<(), String> {
        let within = |value: Option<f64>, low: f64, high: f64| value.is_none_or(|value| value.is_finite() && (low..=high).contains(&value));
        if self.identity.as_deref().is_some_and(|identity| identity != "root" && identity != "spelling") {
            return Err("harmony: identity must be root or spelling".into());
        }
        if !within(self.strength, 0.0, 64.0) {
            return Err("harmony: strength must be between 0 and 64".into());
        }
        if self.window.is_some_and(|value| !(1..=512).contains(&value)) {
            return Err("harmony: window must be between 1 and 512".into());
        }
        if self.hold_limit.is_some_and(|value| value > 64) {
            return Err("harmony: hold_limit must be between 0 and 64".into());
        }
        if !within(self.outside_bonus, 0.0, 20.0) {
            return Err("harmony: outside_bonus must be between 0 and 20".into());
        }
        if !within(self.outside_limit, 0.0, 1.0) {
            return Err("harmony: outside_limit must be between 0 and 1".into());
        }
        if !within(self.section_strength, 0.0, 64.0) {
            return Err("harmony: section_strength must be between 0 and 64".into());
        }
        if self.section_open.is_some_and(|value| !(1..=16).contains(&value)) {
            return Err("harmony: section_open must be between 1 and 16".into());
        }
        Ok(())
    }

    /// The engine's `harmony` field for a score planned from these lyrics; the
    /// sections to follow are the lyrics' tags as the score names them.
    pub fn engine_field(&self, lyrics: &str) -> Result<Value, String> {
        self.validate()?;
        let mut field = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let map = field.as_object_mut().expect("a struct serializes to an object");
        map.remove("follow_lyrics");
        if self.follow_lyrics {
            let labels = phrasing::section_labels(lyrics);
            if labels.is_empty() {
                return Err("harmony: follow_lyrics needs section tags such as [verse] in the lyrics".into());
            }
            if labels.len() > 64 {
                return Err("harmony: follow_lyrics takes up to 64 sections".into());
            }
            map.insert("follow".into(), serde_json::json!(labels));
        }
        Ok(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_asked_is_inactive() {
        assert!(!Harmony::default().active());
        assert!(!Harmony { strength: Some(0.0), window: Some(32), ..Harmony::default() }.active());
        assert!(Harmony { strength: Some(4.0), ..Harmony::default() }.active());
        assert!(Harmony { follow_lyrics: true, ..Harmony::default() }.active());
    }

    #[test]
    fn the_bounds_name_the_knob() {
        assert!(Harmony { strength: Some(65.0), ..Harmony::default() }.validate().unwrap_err().contains("strength"));
        assert!(Harmony { identity: Some("chord".into()), ..Harmony::default() }.validate().unwrap_err().contains("identity"));
        assert!(Harmony { window: Some(0), ..Harmony::default() }.validate().unwrap_err().contains("window"));
        assert!(Harmony { outside_limit: Some(f64::NAN), ..Harmony::default() }.validate().is_err());
        assert!(Harmony { strength: Some(8.0), hold_limit: Some(0), section_open: Some(16), ..Harmony::default() }.validate().is_ok());
    }

    #[test]
    fn the_lyrics_sections_become_the_order_to_follow() {
        let harmony = Harmony { strength: Some(6.0), follow_lyrics: true, ..Harmony::default() };
        let field = harmony.engine_field("[Intro]\n[Verse 1]\nwalking down\n[Hook]\nall the lights\n[Outro]").unwrap();
        assert_eq!(field, serde_json::json!({ "strength": 6.0, "follow": ["intro", "verse", "chorus", "outro"] }));
        assert!(harmony.engine_field("").unwrap_err().contains("section tags"));
        let plain = Harmony { strength: Some(6.0), identity: Some("spelling".into()), ..Harmony::default() };
        assert_eq!(plain.engine_field("").unwrap(), serde_json::json!({ "identity": "spelling", "strength": 6.0 }));
    }
}
