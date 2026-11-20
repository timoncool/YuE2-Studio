use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{EngineClient, engine_result};

pub(crate) const ENGINE_ID: &str = "minimaxmusic-cpp";

#[derive(Debug, Serialize)]
pub(crate) struct Profile {
    pub id: &'static str,
    pub label: &'static str,
    pub total_bytes: u64,
    pub requires_cuda13: bool,
    #[serde(skip)]
    files: [&'static str; 5],
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileStatus {
    #[serde(flatten)]
    pub profile: &'static Profile,
    pub ready: bool,
    pub missing: Vec<&'static str>,
}

#[cfg(test)]
const ROLES: [&str; 5] = ["lm", "depth", "cond", "dit", "vae"];
const FIELDS: [&str; 5] = [
    "lm_model",
    "depth_model",
    "cond_model",
    "dit_model",
    "vae_model",
];

// IDs and filenames match the published MiniMax Studio catalogue.
pub(crate) const PROFILES: &[Profile] = &[
    Profile {
        id: "minimal",
        label: "Minimal - Q3_K_M / Q4_K_M / Q3_K_M (8 GB cards)",
        total_bytes: 6540010752,
        requires_cuda13: false,
        files: [
            "mm3-lm-Q3_K_M.gguf",
            "mm3-depth-Q4_K_M.gguf",
            "MiniMax-Music3-condition_encoder-F32.gguf",
            "mm3-dit-Q3_K_M.gguf",
            "MiniMax-Music3-vocoder-F32.gguf",
        ],
    },
    Profile {
        id: "recommended-light",
        label: "Light - Q4_K_M / Q4_K_M / Q4_K_S (speed / low VRAM)",
        total_bytes: 7713891584,
        requires_cuda13: false,
        files: [
            "mm3-lm-Q4_K_M.gguf",
            "mm3-depth-Q4_K_M.gguf",
            "MiniMax-Music3-condition_encoder-F32.gguf",
            "mm3-dit-Q4_K_S.gguf",
            "MiniMax-Music3-vocoder-F32.gguf",
        ],
    },
    Profile {
        id: "balanced",
        label: "Balanced - Q6_K / Q8_0 / Q5_K_M",
        total_bytes: 9834362240,
        requires_cuda13: false,
        files: [
            "MiniMax-Music3-language_model-Q6_K.gguf",
            "MiniMax-Music3-rvq_depth_decoder-Q8_0.gguf",
            "MiniMax-Music3-condition_encoder-F32.gguf",
            "MiniMax-Music3-transformer-Q5_K_M.gguf",
            "MiniMax-Music3-vocoder-F32.gguf",
        ],
    },
    Profile {
        id: "quality-q8",
        label: "Recommended - Quality Q8_0",
        total_bytes: 12822947200,
        requires_cuda13: false,
        files: [
            "MiniMax-Music3-language_model-Q8_0.gguf",
            "MiniMax-Music3-rvq_depth_decoder-Q8_0.gguf",
            "MiniMax-Music3-condition_encoder-F32.gguf",
            "MiniMax-Music3-transformer-Q8_0.gguf",
            "MiniMax-Music3-vocoder-F32.gguf",
        ],
    },
    Profile {
        id: "quality-lm-nvfp4",
        label: "Quality, LM in NVFP4 - NVFP4 LM, Q8_0 depth decoder and DiT (CUDA 13 build; recommended on an RTX 50, which computes NVFP4 on its tensor cores)",
        total_bytes: 9351911872,
        requires_cuda13: true,
        files: [
            "mm3-lm-NVFP4.gguf",
            "MiniMax-Music3-rvq_depth_decoder-Q8_0.gguf",
            "MiniMax-Music3-condition_encoder-F32.gguf",
            "MiniMax-Music3-transformer-Q8_0.gguf",
            "MiniMax-Music3-vocoder-F32.gguf",
        ],
    },
    Profile {
        id: "quality-nvfp4",
        label: "NVFP4 - LM, depth and DiT in NVFP4 (CUDA 13 build; an RTX 50 computes it on its tensor cores)",
        total_bytes: 7848275200,
        requires_cuda13: true,
        files: [
            "mm3-lm-NVFP4.gguf",
            "mm3-depth-NVFP4.gguf",
            "MiniMax-Music3-condition_encoder-F32.gguf",
            "mm3-dit-NVFP4.gguf",
            "MiniMax-Music3-vocoder-F32.gguf",
        ],
    },
    Profile {
        id: "native",
        label: "Full native - BF16 / F32 original weights",
        total_bytes: 28600781952,
        requires_cuda13: false,
        files: [
            "MiniMax-Music3-language_model-BF16.gguf",
            "MiniMax-Music3-rvq_depth_decoder-BF16.gguf",
            "MiniMax-Music3-condition_encoder-F32.gguf",
            "MiniMax-Music3-transformer-F32.gguf",
            "MiniMax-Music3-vocoder-F32.gguf",
        ],
    },
];

#[derive(Debug, Deserialize)]
struct Props {
    models: ModelBuckets,
}

#[derive(Debug, Deserialize)]
struct ModelBuckets {
    lm: Vec<String>,
    depth: Vec<String>,
    cond: Vec<String>,
    dit: Vec<String>,
    vae: Vec<String>,
}

impl Props {
    fn profile_status(&self, profile: &'static Profile) -> ProfileStatus {
        let buckets = [
            &self.models.lm,
            &self.models.depth,
            &self.models.cond,
            &self.models.dit,
            &self.models.vae,
        ];
        let missing: Vec<_> = profile
            .files
            .iter()
            .zip(buckets)
            .filter_map(|(&file, bucket)| {
                (!bucket.iter().any(|candidate| candidate == file)).then_some(file)
            })
            .collect();
        ProfileStatus {
            profile,
            ready: missing.is_empty(),
            missing,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SongRequest<'a> {
    pub caption: &'a str,
    pub lyrics: &'a str,
    pub duration_seconds: Option<f64>,
    pub steps: Option<u32>,
    pub seed: Option<i64>,
    pub lm_seed: Option<i64>,
}

fn request_body(profile: &Profile, request: &SongRequest<'_>) -> Result<Value> {
    if request.caption.trim().is_empty() || request.lyrics.trim().is_empty() {
        bail!("MiniMax requires both a style description and lyrics");
    }
    let duration = request.duration_seconds.unwrap_or(60.0);
    if !duration.is_finite() || duration <= 0.0 {
        bail!("MiniMax duration must be greater than zero");
    }
    let steps = request.steps.unwrap_or(30);
    if steps < 2 {
        bail!("MiniMax needs at least two inference steps");
    }
    let mut body = json!({
        "caption": request.caption,
        "lyrics": request.lyrics,
        "duration": duration,
        "steps": steps,
        "lm_batch_size": 1,
        "synth_batch_size": 1,
        // Match the standalone studio: keep unencoded audio for the library importer.
        "output_format": "wav32",
    });
    if let Some(seed) = request.seed {
        body["seed"] = json!(seed);
    }
    if let Some(seed) = request.lm_seed {
        body["lm_seed"] = json!(seed);
    }
    for (field, file) in FIELDS.into_iter().zip(profile.files) {
        body[field] = json!(file);
    }
    Ok(body)
}

#[derive(Clone)]
pub(crate) struct Client(EngineClient);

impl Client {
    /// Explicit loopback listener chosen by the MiniMax supervisor.
    pub(crate) fn new(port: u16) -> Result<Self> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self(EngineClient {
            base_url: format!("http://127.0.0.1:{port}"),
            http,
            health_cache: Default::default(),
        }))
    }

    async fn props(&self) -> Result<Props> {
        serde_json::from_value(self.0.props().await?)
            .context("MiniMax server did not return its model buckets")
    }

    pub(crate) async fn profiles(&self) -> Result<Vec<ProfileStatus>> {
        let props = self.props().await?;
        Ok(PROFILES
            .iter()
            .map(|profile| props.profile_status(profile))
            .collect())
    }

    pub(crate) async fn submit(
        &self,
        profile_id: &str,
        request: &SongRequest<'_>,
    ) -> Result<String> {
        let profile = PROFILES
            .iter()
            .find(|profile| profile.id == profile_id)
            .with_context(|| format!("unknown MiniMax profile {profile_id}"))?;
        let body = request_body(profile, request)?;
        let status = self.props().await?.profile_status(profile);
        if !status.ready {
            bail!(
                "MiniMax profile {profile_id} is missing {}",
                status.missing.join(", ")
            );
        }
        let id = self.0.submit(body).await?.id;
        if id.is_empty() {
            bail!("MiniMax server returned an empty job id");
        }
        Ok(id)
    }

    pub(crate) async fn status(&self, job_id: &str) -> Result<String> {
        Ok(self.0.job(job_id).await?.status)
    }

    pub(crate) async fn cancel(&self, job_id: &str) -> Result<String> {
        Ok(self.0.cancel(job_id).await?.status)
    }

    pub(crate) async fn result(&self, job_id: &str) -> Result<Vec<engine_result::ResultTrack>> {
        let result = self.0.result(job_id).await?;
        let tracks = engine_result::parse_multipart_result(&result.content_type, &result.body)?;
        for track in &tracks {
            engine_result::audio_extension(&track.audio_content_type)?;
            if track.audio.is_empty() {
                bail!("MiniMax returned an empty audio track");
            }
        }
        Ok(tracks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::Query,
        http::header,
        routing::{get, post},
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[tokio::test]
    async fn minimax_profile_dispatches_the_five_weight_files_and_returns_native_result() {
        let profile = &PROFILES[0];
        let props = json!({"models": ROLES.into_iter().zip(profile.files)
            .map(|(role, filename)| (role.to_owned(), json!([filename])))
            .collect::<serde_json::Map<_, _>>()});
        let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
        let synth = {
            let seen = seen.clone();
            move |Json(body): Json<Value>| async move {
                seen.lock().unwrap().push(body);
                Json(json!({"id": "fixture"}))
            }
        };
        let app = Router::new()
            .route("/props", get(move || async move { Json(props) }))
            .route("/synth", post(synth))
            .route("/job", get(|Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(query["id"], "fixture");
                if query.contains_key("result") {
                    ([(header::CONTENT_TYPE, "multipart/mixed; boundary=fixture")],
                     "--fixture\r\nContent-Type: application/json\r\n\r\n{\"seed\":42}\r\n--fixture\r\nContent-Type: audio/wav\r\n\r\naudio\r\n--fixture--\r\n")
                } else {
                    ([(header::CONTENT_TYPE, "application/json")], r#"{"status":"completed"}"#)
                }
            }).post(|Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(query["id"], "fixture");
                assert_eq!(query["cancel"], "1");
                Json(json!({"status": "cancelled"}))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::new(listener.local_addr().unwrap().port()).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let profiles = client.profiles().await.unwrap();
        assert!(profiles[0].ready);
        assert!(!profiles[1].ready);

        let request = SongRequest {
            caption: "folk",
            lyrics: "[verse]\nThe morning light",
            duration_seconds: Some(3.2),
            steps: Some(2),
            seed: Some(42),
            ..Default::default()
        };
        assert!(client.submit("not-a-profile", &request).await.is_err());
        assert!(
            client
                .submit(profiles[1].profile.id, &request)
                .await
                .is_err()
        );
        assert!(
            client
                .submit(
                    profile.id,
                    &SongRequest {
                        lyrics: "",
                        ..request.clone()
                    }
                )
                .await
                .is_err()
        );
        let id = client.submit(profile.id, &request).await.unwrap();
        assert_eq!(client.status(&id).await.unwrap(), "completed");
        assert_eq!(client.cancel(&id).await.unwrap(), "cancelled");
        let result = client.result(&id).await.unwrap();
        assert_eq!(result[0].audio, b"audio");
        assert_eq!(result[0].replay_request["seed"], 42);
        let requests = seen.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["output_format"], "wav32");
        for (field, file) in FIELDS.into_iter().zip(profile.files) {
            assert_eq!(requests[0][field], file);
        }
        server.abort();
    }
}
