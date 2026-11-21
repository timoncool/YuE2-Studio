use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use tokio::sync::Mutex;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{EngineClient, engine_result, hardware};

pub(crate) const ENGINE_ID: &str = "minimaxmusic-cpp";

#[derive(Debug, Serialize)]
pub(crate) struct Profile {
    pub id: &'static str,
    pub label: &'static str,
    pub total_bytes: u64,
    pub requires_cuda13: bool,
    #[serde(skip)]
    files: [&'static str; 5],
    #[serde(skip)]
    sizes: [u64; 5],
}

#[derive(Debug, Serialize)]
pub(crate) struct ProfileStatus {
    #[serde(flatten)]
    pub profile: &'static Profile,
    pub ready: bool,
    pub missing: Vec<&'static str>,
    pub native_adapters: Vec<crate::adapters::Installed>,
}

#[cfg(test)]
const ROLES: [&str; 5] = ["lm", "depth", "cond", "dit", "vae"];
pub(super) const PROFILE_FILE_FIELDS: [&str; 5] = [
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
        sizes: [4589971808, 404836256, 100672192, 1138427712, 306102784],
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
        sizes: [5512456544, 404836256, 100672192, 1389823808, 306102784],
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
        sizes: [7048279328, 686513600, 100672192, 1692794336, 306102784],
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
        sizes: [9127257376, 686513600, 100672192, 2602401248, 306102784],
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
        sizes: [5656222048, 686513600, 100672192, 2602401248, 306102784],
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
        sizes: [5656222048, 401493920, 100672192, 1383784256, 306102784],
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
        sizes: [17174297792, 1292053888, 100672192, 9727655296, 306102784],
    },
];

/// Resolve the original files of a saved take, including older imports
/// whose profile ID was absent but whose full set of filenames was saved.
pub(crate) fn saved_profile(id: Option<&str>, replay: &Value) -> Result<&'static str> {
    if let Some(profile) = PROFILES.iter().find(|profile| Some(profile.id) == id) {
        for (field, file) in PROFILE_FILE_FIELDS.into_iter().zip(profile.files) {
            if let Some(saved) = replay.get(field) {
                if saved.as_str() != Some(file) {
                    bail!(
                        "MiniMax replay uses a different {field} from profile {}",
                        profile.id
                    );
                }
            }
        }
        return Ok(profile.id);
    }
    PROFILES
        .iter()
        .find(|profile| {
            PROFILE_FILE_FIELDS
                .into_iter()
                .zip(profile.files)
                .all(|(field, file)| replay[field].as_str() == Some(file))
        })
        .map(|profile| profile.id)
        .context("The original MiniMax model files do not match an available profile")
}

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

impl Profile {
    fn status_on_disk(&'static self, roots: &[PathBuf]) -> ProfileStatus {
        let missing = roots
            .iter()
            .map(|root| self.missing_at(root))
            .min_by_key(Vec::len)
            .unwrap_or_else(|| self.files.to_vec());
        ProfileStatus {
            profile: self,
            native_adapters: Vec::new(),
            ready: missing.is_empty(),
            missing,
        }
    }

    fn missing_at(&self, root: &Path) -> Vec<&'static str> {
        self.files
            .into_iter()
            .zip(self.sizes)
            .filter_map(|(file, expected)| {
                let exists = fs::metadata(root.join(file))
                    .is_ok_and(|entry| entry.is_file() && entry.len() == expected);
                (!exists).then_some(file)
            })
            .collect()
    }
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
            native_adapters: Vec::new(),
            ready: missing.is_empty(),
            missing,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sampling {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lm_cfg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lm_top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dit_cfg: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adapters: Vec<AdapterRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdapterRef {
    pub name: String,
    pub lm_scale: f64,
    pub dit_scale: f64,
}

impl Sampling {
    fn validate(&self) -> Result<()> {
        for (label, value) in [("LM CFG", self.lm_cfg), ("DiT CFG", self.dit_cfg)] {
            if value.is_some_and(|value| value < 0.0 || !(value as f32).is_finite()) {
                bail!("MiniMax {label} must be finite and nonnegative");
            }
        }
        if self.lm_top_k.is_some_and(|value| value < 1) {
            bail!("MiniMax LM Top-K must be a positive integer");
        }
        for adapter in &self.adapters {
            if adapter.name.is_empty() || adapter.name.starts_with('.') || adapter.name.contains(['/', '\\', ':'])
                || ![adapter.lm_scale, adapter.dit_scale].into_iter().all(|scale| (scale as f32).is_finite())
            {
                bail!("MiniMax adapter needs a folder name and finite scales");
            }
        }
        Ok(())
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
    pub sampling: Sampling,
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
    request.sampling.validate()?;
    if let Value::Object(parameters) = serde_json::to_value(&request.sampling)? {
        body.as_object_mut().expect("MiniMax request object").extend(parameters);
    }
    for (field, file) in PROFILE_FILE_FIELDS.into_iter().zip(profile.files) {
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
        self.submit_profile(profile, request_body(profile, request)?)
            .await
    }

    /// Render a saved semantic stream. The original profile binds model
    /// filenames even when a replay originating in an older Studio omitted them.
    pub(crate) async fn submit_replay(&self, profile_id: &str, mut body: Value) -> Result<String> {
        let profile = PROFILES
            .iter()
            .find(|profile| profile.id == profile_id)
            .with_context(|| format!("unknown MiniMax profile {profile_id}"))?;
        anyhow::ensure!(
            body["audio_codes"]
                .as_str()
                .is_some_and(|codes| !codes.trim().is_empty()),
            "MiniMax replay has no audio codes"
        );
        for (field, file) in PROFILE_FILE_FIELDS.into_iter().zip(profile.files) {
            body[field] = json!(file);
        }
        body["lm_batch_size"] = json!(1);
        body["synth_batch_size"] = json!(1);
        body["output_format"] = json!("wav32");
        self.submit_profile(profile, body).await
    }

    async fn submit_profile(&self, profile: &'static Profile, body: Value) -> Result<String> {
        let status = self.props().await?.profile_status(profile);
        if !status.ready {
            bail!(
                "MiniMax profile {} is missing {}",
                profile.id,
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

    #[test]
    fn startup_diagnosis_only_uses_the_current_mini_max_run() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("engine.log");
        let old = "CUDA error: out of memory\n";
        fs::write(&path, format!("{old}listener unavailable\n")).unwrap();
        assert!(
            startup_failure_context(&path, old.len() as u64).starts_with("MiniMax did not start.")
        );
        assert!(startup_failure_context(&path, 0).starts_with("MiniMax ran out of GPU memory"));
    }

    #[test]
    fn startup_memory_diagnosis_distinguishes_host_device_and_unknown_allocations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("engine.log");
        for (diagnostic, expected) in [
            (
                "[Load] Device backend: CPU\nfailed to allocate 4096 MB",
                "out of system memory",
            ),
            ("failed to allocate 4096 MB", "could not allocate memory"),
            ("cudaMalloc failed: out of memory", "out of GPU memory"),
            ("VK_ERROR_OUT_OF_DEVICE_MEMORY", "out of GPU memory"),
            (
                "[Load] Device backend: CUDA0\nVK_ERROR_OUT_OF_HOST_MEMORY",
                "out of system memory",
            ),
            (
                "[Load] Device backend: Vulkan0\nfailed to allocate 4096 MB",
                "out of GPU memory",
            ),
        ] {
            fs::write(&path, diagnostic).unwrap();
            let message = startup_failure_context(&path, 0);
            assert!(message.contains(expected), "{diagnostic}: {message}");
        }
    }

    #[test]
    fn minimax_missing_engine_and_truncated_profile_are_rejected_before_start() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        let mut installation = Installation {
            executable: None,
            roots: vec![path.to_path_buf()],
            runtime_dirs: vec![],
            adapters_root: None,
        };
        let profile = &PROFILES[0];
        assert!(!installation.engine_installed());
        assert!(
            installation
                .for_profile(profile)
                .unwrap_err()
                .to_string()
                .contains("mm-server is missing")
        );
        let executable = path.join("mm-server");
        let shown_file = path.join(profile.files[0]);
        fs::write(&executable, []).unwrap();
        fs::write(shown_file, b"GGUF").unwrap();
        installation.executable = Some(executable);
        assert!(installation.engine_installed());
        assert_eq!(installation.profiles()[0].missing, profile.files);
        assert!(
            installation
                .for_profile(profile)
                .unwrap_err()
                .to_string()
                .contains("profile minimal is incomplete")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn minimax_fixture_child() {
        let Ok(port) = env::var("YUE_MINIMAX_TEST_CHILD_PORT") else {
            return;
        };
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route(
                "/props",
                get(|| async {
                    Json(json!({"models": {"lm":[],"depth":[],"cond":[],"dit":[],"vae":[]}}))
                }),
            );
        axum::serve(listener, app).await.unwrap();
    }

    #[tokio::test]
    async fn minimax_starts_and_stops_an_owned_http_process() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut command = Command::new(env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "native_minimax::tests::minimax_fixture_child",
                "--nocapture",
            ])
            .env("YUE_MINIMAX_TEST_CHILD_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        drop(listener);
        let mut child = Running::spawn(command, port).unwrap();
        child.wait_ready(Duration::from_secs(10)).await.unwrap();
        let runtime = Runtime::default();
        *runtime.running.lock().await = Some(child);
        assert!(runtime.healthy().await);
        runtime.stop().await;
        assert!(!runtime.healthy().await);
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok());
    }

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
                    ([(header::CONTENT_TYPE, "application/json")], r#"{"status":"done"}"#)
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
        let old_body = request_body(profile, &request).unwrap();
        for key in ["lm_cfg", "lm_top_k", "dit_cfg"] {
            assert!(old_body.get(key).is_none());
        }
        let invalid = SongRequest { sampling: Sampling { lm_top_k: Some(0), ..Default::default() }, ..request.clone() };
        assert!(request_body(profile, &invalid).is_err());
        let request = SongRequest {
            sampling: Sampling { lm_cfg: Some(1.3), lm_top_k: Some(41), dit_cfg: Some(2.2),
                adapters: vec![AdapterRef { name: "test-dit".into(), lm_scale: 0.0, dit_scale: 0.65 }] },
            ..request
        };
        let id = client.submit(profile.id, &request).await.unwrap();
        assert_eq!(client.status(&id).await.unwrap(), "done");
        assert_eq!(client.cancel(&id).await.unwrap(), "cancelled");
        let result = client.result(&id).await.unwrap();
        assert_eq!(result[0].audio, b"audio");
        assert_eq!(result[0].replay_request["seed"], 42);
        let codes = "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16";
        for seed in [42, 43] {
            client
                .submit_replay(
                    profile.id,
                    json!({"caption":"folk","lyrics":"verse","audio_codes":codes,"seed":seed}),
                )
                .await
                .unwrap();
        }
        let requests = seen.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1]["audio_codes"], codes);
        assert_eq!(requests[2]["audio_codes"], codes);
        assert_eq!(requests[1]["seed"], 42);
        assert_eq!(requests[2]["seed"], 43);
        assert_eq!(requests[2]["dit_model"], profile.files[3]);
        assert_eq!(requests[2]["output_format"], "wav32");
        assert_eq!(requests[0]["output_format"], "wav32");
        assert_eq!(requests[0]["lm_cfg"], 1.3);
        assert_eq!(requests[0]["lm_top_k"], 41);
        assert_eq!(requests[0]["dit_cfg"], 2.2);
        assert_eq!(requests[0]["adapters"], json!([{"name":"test-dit","lm_scale":0.0,"dit_scale":0.65}]));
        for (field, file) in PROFILE_FILE_FIELDS.into_iter().zip(profile.files) {
            assert_eq!(requests[0][field], file);
        }
        server.abort();
    }
}

/// An AppState can keep one of these. The caller holds the shared music-engine
/// lease through polling and import, so a backend switch waits for that job.
#[derive(Default)]
pub(crate) struct Runtime {
    running: Mutex<Option<Running>>,
}

impl Runtime {
    /// Search the current installation and the old registered MiniMax
    /// installations. Merely reading the catalogue never starts a process.
    pub(crate) fn installation(data_root: &Path) -> Installation {
        Installation::discover(data_root)
    }

    pub(crate) async fn start(&self, profile_id: &str, data_root: &Path) -> Result<Client> {
        let profile = PROFILES
            .iter()
            .find(|profile| profile.id == profile_id)
            .with_context(|| format!("unknown MiniMax profile {profile_id}"))?;
        let install = Self::installation(data_root);
        let launch = install.for_profile(profile)?;

        let mut running = self.running.lock().await;
        if let Some(current) = running.as_mut() {
            if current.launch.as_ref() == Some(&launch) && current.child.try_wait()?.is_none() {
                // A long decode may briefly delay HTTP. Let the caller surface
                // the health failure; never spawn over an owned live process.
                if current.client.0.health().await {
                    return Ok(current.client.clone());
                }
                bail!("The MiniMax process is running but its health endpoint did not respond");
            }
            running.take();
        }

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .context("reserve a loopback port for MiniMax")?;
        let port = listener.local_addr()?.port();
        let log = data_root.join("minimax-engine.log");
        let log_start = fs::metadata(&log).map_or(0, |meta| meta.len());
        let command = launch.command(port, data_root)?;
        drop(listener);
        let mut current = Running::spawn(command, port)?;
        if let Err(error) = current.wait_ready(Duration::from_secs(30)).await {
            drop(current);
            let context = startup_failure_context(&log, log_start);
            return Err(error).context(context);
        }
        current.launch = Some(launch);
        let client = current.client.clone();
        *running = Some(current);
        Ok(client)
    }

    /// Only checks the loopback listener of our own child.
    pub(crate) async fn healthy(&self) -> bool {
        let mut running = self.running.lock().await;
        let Some(current) = running.as_mut() else {
            return false;
        };
        matches!(current.child.try_wait(), Ok(None)) && current.client.0.health().await
    }

    pub(crate) async fn stop(&self) {
        self.running.lock().await.take();
    }
}

fn startup_failure_context(log: &Path, start: u64) -> String {
    let diagnostic = (|| -> std::io::Result<String> {
        let mut file = fs::File::open(log)?;
        let tail = file.metadata()?.len().saturating_sub(64 * 1024);
        file.seek(SeekFrom::Start(start.max(tail)))?;
        let mut bytes = Vec::new();
        file.take(64 * 1024).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).to_lowercase())
    })()
    .unwrap_or_default();
    let backend = diagnostic.lines().rev().find_map(|line| {
        line.split_once("device backend:")
            .or_else(|| line.split_once("lm backend:"))
            .map(|(_, backend)| backend.trim_start())
    });
    let host_allocation = crate::describes_exhausted_ram(&diagnostic)
        || diagnostic.contains("vk_error_out_of_host_memory")
        || diagnostic.contains("failed to allocate host memory");
    let gpu_allocation = diagnostic.lines().any(|line| {
        line.contains("vk_error_out_of_device_memory")
            || (line.contains("cuda") || line.contains("vulkan"))
                && crate::describes_exhausted_memory(line)
    });
    let reason = if host_allocation {
        "MiniMax ran out of system memory while loading."
    } else if gpu_allocation {
        "MiniMax ran out of GPU memory while loading."
    } else if crate::describes_exhausted_memory(&diagnostic) {
        match backend {
            Some(name) if name.starts_with("cpu") => {
                "MiniMax ran out of system memory while loading."
            }
            Some(name)
                if ["cuda", "vulkan", "metal"]
                    .iter()
                    .any(|prefix| name.starts_with(prefix)) =>
            {
                "MiniMax ran out of GPU memory while loading."
            }
            _ => "MiniMax could not allocate memory while loading.",
        }
    } else {
        "MiniMax did not start."
    };
    format!("{reason} See {}", log.display())
}

/// Same resource and model-folder conventions as the native MiniMax manager.
/// An explicit override is authoritative, even if its target is missing.
pub(crate) struct Installation {
    executable: Option<PathBuf>,
    roots: Vec<PathBuf>,
    runtime_dirs: Vec<PathBuf>,
    adapters_root: Option<PathBuf>,
}

#[cfg(test)]
#[test]
fn native_adapter_inventory_requires_matching_engine_and_weights() {
    let root = tempfile::tempdir().unwrap();
    let install = Installation {
        executable: None, roots: Vec::new(), runtime_dirs: Vec::new(), adapters_root: Some(root.path().to_owned()),
    };
    for (id, engine, weights) in [("valid", ENGINE_ID, true), ("missing", ENGINE_ID, false), ("foreign", "acestep-cpp", true)] {
        let directory = root.path().join(id);
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("adapter.json"), json!({
            "id": id, "name": id, "engine": engine, "slots": ["dit"], "origin": {"type":"imported"}
        }).to_string()).unwrap();
        if weights { fs::write(directory.join("model.safetensors"), b"fixture").unwrap(); }
    }
    let adapters = install.native_adapters();
    assert_eq!(adapters.len(), 1);
    assert_eq!(adapters[0].meta.id, "valid");
}

impl Installation {
    fn discover(data_root: &Path) -> Self {
        let neighbors: Vec<_> = studio_hub_client::neighbors::others(data_root)
            .into_iter()
            .filter(|entry| entry.app == "minimax")
            .collect();
        let data_dirs: Vec<_> = std::iter::once(data_root.to_path_buf())
            .chain(neighbors.into_iter().map(|entry| entry.data_dir))
            .collect();
        let mut binaries = Vec::new();
        if let Ok(current) = env::current_exe() {
            if let Some(parent) = current.parent() {
                binaries.push(parent.join("resources/minimaxmusic-cpp"));
                if let Some(bundle) = parent.parent() {
                    binaries.push(bundle.join("Resources/resources/minimaxmusic-cpp"));
                    binaries.push(bundle.join("lib/YuE2 Studio/resources/minimaxmusic-cpp"));
                }
            }
        }
        for root in &data_dirs {
            if let Some(parent) = root.parent() {
                binaries.push(parent.join("resources/minimaxmusic-cpp"));
            }
        }
        if cfg!(target_os = "linux") {
            binaries.push(PathBuf::from(
                "/usr/lib/MiniMax Music3 Studio/resources/minimaxmusic-cpp",
            ));
        }
        if cfg!(target_os = "macos") {
            binaries.push(PathBuf::from("/Applications/MiniMax Music3 Studio.app/Contents/Resources/resources/minimaxmusic-cpp"));
        }
        for key in ["ProgramFiles", "LOCALAPPDATA"] {
            if cfg!(windows) {
                if let Some(root) = env::var_os(key) {
                    let mut root = PathBuf::from(root);
                    if key == "LOCALAPPDATA" {
                        root.push("Programs");
                    }
                    binaries.push(root.join("MiniMax Music3 Studio/resources/minimaxmusic-cpp"));
                }
            }
        }
        let bundle = env::var_os("MINIMAX_MM_SERVER_ROOT").map(PathBuf::from);
        let name = if cfg!(windows) {
            "mm-server.exe"
        } else {
            "mm-server"
        };
        let executable = override_path("YUE_MINIMAX_MM_SERVER_BIN", "MINIMAX_MM_SERVER_BIN")
            .or_else(|| match &bundle {
                Some(dir) => Some(dir.join(name)),
                None => crate::native_music_runtime::installed(data_root, ENGINE_ID).or_else(|| binaries
                    .into_iter()
                    .map(|dir| dir.join(name))
                    .find(|file| file.is_file())),
            });
        let roots = override_path("YUE_MINIMAX_MODELS_ROOT", "MINIMAX_MUSIC_MODELS_ROOT")
            .map(|root| vec![root])
            .unwrap_or_else(|| {
                data_dirs
                    .iter()
                    .map(|dir| dir.join("models").join(ENGINE_ID))
                    .chain(
                        executable
                            .as_deref()
                            .and_then(Path::parent)
                            .map(|dir| dir.join("models")),
                    )
                    .collect()
            });
        let runtime_dirs = data_dirs
            .iter()
            .map(|dir| {
                let runtime = dir.join("engine-runtime");
                if cfg!(target_os = "linux") {
                    runtime.join("cuda")
                } else {
                    runtime
                }
            })
            .collect();
        let adapters_root = std::iter::once(data_root.join("adapters").join(ENGINE_ID))
            .chain(data_dirs.iter().skip(1).map(|dir| dir.join("adapters")))
            .filter_map(|dir| dir.canonicalize().ok())
            .find(|dir| !native_adapters_at(dir).is_empty());
        Self { executable, roots, runtime_dirs, adapters_root }
    }

    pub(crate) fn engine_installed(&self) -> bool {
        self.executable.as_ref().is_some_and(|file| file.is_file())
    }

    pub(crate) fn executable(&self) -> Option<&Path> {
        self.executable.as_deref()
    }

    pub(crate) fn profiles(&self) -> Vec<ProfileStatus> {
        let adapters = self.native_adapters();
        PROFILES.iter().map(|profile| {
            let mut status = profile.status_on_disk(&self.roots);
            if status.ready && self.engine_installed() {
                status.native_adapters = adapters.clone();
            }
            status
        }).collect()
    }

    pub(crate) fn native_adapters(&self) -> Vec<crate::adapters::Installed> {
        let Some(root) = self.adapters_root.as_ref() else { return Vec::new() };
        native_adapters_at(root)
    }

    fn for_profile(&self, profile: &'static Profile) -> Result<Launch> {
        let executable = self.executable.as_ref().filter(|file| file.is_file())
            .context("MiniMax mm-server is missing. Install the runtime in Create or set YUE_MINIMAX_MM_SERVER_BIN to its executable.")?;
        let models_root = self
            .roots
            .iter()
            .find(|root| profile.missing_at(root).is_empty())
            .with_context(|| {
                format!(
                    "MiniMax profile {} is incomplete: {}",
                    profile.id,
                    profile.status_on_disk(&self.roots).missing.join(", ")
                )
            })?;
        Ok(Launch {
            executable: fs::canonicalize(executable)
                .context("resolve the installed MiniMax engine")?,
            models_root: fs::canonicalize(models_root)
                .context("resolve the installed MiniMax model folder")?,
            runtime_dirs: self.runtime_dirs.clone(),
            adapters_root: self.adapters_root.clone(),
        })
    }
}

fn native_adapters_at(root: &Path) -> Vec<crate::adapters::Installed> {
    let library = crate::adapters::AdapterLibrary::at(root.to_owned(), ENGINE_ID);
    library.installed(None).into_iter().filter(|entry| {
        let id = &entry.meta.id;
        entry.error.is_none() && !id.is_empty() && !id.starts_with('.') && !id.contains(['/', '\\', ':'])
            && entry.meta.slots.iter().any(|slot| matches!(slot.as_str(), "lm" | "dit"))
            && fs::read_dir(root.join(id)).is_ok_and(|files| files.flatten().any(|file|
                file.path().is_file() && file.path().extension().is_some_and(|extension| extension == "safetensors")))
    }).collect()
}

fn override_path(primary: &str, legacy: &str) -> Option<PathBuf> {
    env::var_os(primary)
        .or_else(|| env::var_os(legacy))
        .map(PathBuf::from)
}

#[derive(Debug, PartialEq, Eq)]
struct Launch {
    executable: PathBuf,
    models_root: PathBuf,
    runtime_dirs: Vec<PathBuf>,
    adapters_root: Option<PathBuf>,
}

impl Launch {
    fn command(&self, port: u16, data_root: &Path) -> Result<Command> {
        let folder = self
            .executable
            .parent()
            .context("MiniMax engine has no containing folder")?;
        let mut command = Command::new(&self.executable);
        command
            .current_dir(folder)
            .arg("--models")
            .arg(&self.models_root)
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--max-batch",
                "1",
            ])
            .env_remove("MM3_CUDA_BACKEND")
            .stdin(Stdio::null());
        if let Some(root) = &self.adapters_root {
            command.arg("--adapters").arg(root);
        }
        let machine = hardware::hardware();
        if machine.nvidia && !cfg!(target_os = "macos") {
            let compute = machine
                .compute_capability
                .context("Could not read the NVIDIA card for MiniMax")?;
            let driver = machine
                .driver_major
                .context("Could not read the NVIDIA driver for MiniMax")?;
            let build = hardware::cuda_build(compute, driver).with_context(|| {
                format!(
                    "MiniMax CUDA is unavailable: {}",
                    hardware::cuda_unavailable_reason(&machine)
                )
            })?;
            let library = if cfg!(windows) {
                "ggml-cuda.dll"
            } else {
                "libggml-cuda.so"
            };
            let backend = folder.join(build.folder()).join(library);
            if backend.is_file() {
                command.env("MM3_CUDA_BACKEND", backend);
            } else if !folder.join(library).is_file() {
                bail!(
                    "MiniMax {} backend is missing from {}",
                    build.folder(),
                    folder.display()
                );
            }
            command.args(["--device", "CUDA0"]);
            if hardware::accumulates_in_fp16() {
                command.arg("--clamp-fp16");
            }
        }

        // Use the engine bundle and its installed runtime before inherited
        // search paths, without borrowing the YuE2 AppImage libraries.
        dub_core::proc::outside_appimage(&mut command);
        let mut dirs = vec![folder.to_path_buf()];
        dirs.extend(self.runtime_dirs.iter().filter(|dir| dir.is_dir()).cloned());
        if cfg!(windows) {
            dirs.push(crate::engine_bundle_root());
        }
        if cfg!(target_os = "linux") {
            command.env("LD_LIBRARY_PATH", env::join_paths(dirs)?);
        } else if cfg!(windows) {
            if let Some(path) = env::var_os("PATH") {
                dirs.extend(env::split_paths(&path));
            }
            command.env("PATH", env::join_paths(dirs)?);
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        fs::create_dir_all(data_root)?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(data_root.join("minimax-engine.log"))?;
        command
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        Ok(command)
    }
}

struct Running {
    child: Child,
    client: Client,
    launch: Option<Launch>,
}

impl Running {
    fn spawn(command: Command, port: u16) -> Result<Self> {
        let client = Client::new(port)?;
        let child = music_core::process::spawn_dying_with_parent(command)
            .context("start MiniMax mm-server")?;
        music_core::process::adopt(&child);
        Ok(Self {
            child,
            client,
            launch: None,
        })
    }

    async fn wait_ready(&mut self, timeout: Duration) -> Result<()> {
        tokio::time::timeout(timeout, async {
            loop {
                if let Some(status) = self.child.try_wait()? {
                    bail!("MiniMax mm-server exited with {status}");
                }
                if self.client.0.health().await {
                    self.client.props().await?;
                    if let Some(status) = self.child.try_wait()? {
                        bail!("MiniMax mm-server exited with {status}");
                    }
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .context("waiting for the MiniMax loopback listener timed out")?
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
