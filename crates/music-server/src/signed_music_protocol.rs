//! The music-http-v1 loopback protocol for a verified, Studio-owned child.
use std::{
    net::{Ipv4Addr, TcpListener},
    path::Path,
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use reqwest::{Client, Method, Response, redirect::Policy};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::{
    process::{Child, Command},
    time::{Instant, sleep},
};

pub(super) const NAME: &str = "music-http-v1";
const MAX_CONTROL_BYTES: usize = 1024 * 1024;
const MAX_RESULT_BYTES: usize = 384 * MAX_CONTROL_BYTES;

#[derive(Debug, Deserialize)]
struct Health {
    protocol: String,
    engine_id: String,
    version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Submission {
    pub model: String,
    pub mode: Mode,
    pub payload: Value,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Create,
    Replay,
}

#[derive(Debug, Deserialize)]
struct Accepted {
    id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Progress {
    pub status: Status,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Deserialize)]
pub(super) struct Track {
    pub audio_base64: String,
    pub replay: Value,
    #[serde(default)]
    pub settings: Value,
}

pub(super) struct Process {
    child: Child,
    client: Client,
    base: String,
    secret: String,
}

impl Process {
    pub async fn launch(
        executable: &Path,
        models: &Path,
        engine: &str,
        version: &str,
    ) -> Result<Self> {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
            .local_addr()?
            .port();
        let secret = format!(
            "{}{}",
            uuid::Uuid::now_v7().simple(),
            uuid::Uuid::now_v7().simple()
        );
        let directory = executable
            .parent()
            .context("verified executable has no parent")?;
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(6))
            .build()?;
        let mut command = Command::new(executable);
        command
            .current_dir(directory)
            .env_clear()
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("YUE_MUSIC_ADAPTER_HOST", Ipv4Addr::LOCALHOST.to_string())
            .env("YUE_MUSIC_ADAPTER_PORT", port.to_string())
            .env("YUE_MUSIC_ADAPTER_TOKEN", &secret)
            .env("YUE_MUSIC_ADAPTER_MODELS", models)
            .env("YUE_MUSIC_ADAPTER_ID", engine)
            .env("YUE_MUSIC_ADAPTER_VERSION", version);
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let windows =
                std::env::var_os("SystemRoot").context("Windows directory is unavailable")?;
            let system = std::path::PathBuf::from(windows).join("System32");
            command.env("PATH", std::env::join_paths([directory, system.as_path()])?);
            command.as_std_mut().creation_flags(0x0800_0000);
        }
        #[cfg(not(windows))]
        command.env("PATH", "/usr/bin:/bin");
        let child = command
            .spawn()
            .context("start the verified music adapter")?;
        let mut process = Self {
            child,
            client,
            base: format!("http://127.0.0.1:{port}"),
            secret,
        };
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(exit) = process.child.try_wait()? {
                anyhow::bail!("Music adapter exited before the handshake ({exit})");
            }
            match process.request(Method::GET, "/v1/health").send().await {
                Ok(response) if response.status().is_success() => {
                    let health: Health = read_json(response, MAX_CONTROL_BYTES).await?;
                    ensure!(
                        health.protocol == NAME
                            && health.engine_id == engine
                            && health.version == version,
                        "Music adapter handshake does not match the verified package"
                    );
                    return Ok(process);
                }
                Ok(response) if response.status() != reqwest::StatusCode::SERVICE_UNAVAILABLE => {
                    anyhow::bail!("Music adapter handshake failed ({})", response.status());
                }
                _ if Instant::now() >= deadline => {
                    anyhow::bail!("Music adapter handshake timed out")
                }
                _ => sleep(Duration::from_millis(150)).await,
            }
        }
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.secret)
    }

    pub async fn submit(&self, job: &Submission) -> Result<String> {
        let accepted: Accepted = read_json(
            self.request(Method::POST, "/v1/jobs")
                .json(job)
                .send()
                .await?,
            MAX_CONTROL_BYTES,
        )
        .await?;
        ensure!(
            !accepted.id.is_empty()
                && accepted.id.len() <= 128
                && accepted
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "Music adapter returned an invalid job ID"
        );
        Ok(accepted.id)
    }

    pub async fn status(&self, id: &str) -> Result<Progress> {
        read_json(
            self.request(Method::GET, &format!("/v1/jobs/{id}"))
                .send()
                .await?,
            MAX_CONTROL_BYTES,
        )
        .await
    }

    pub async fn result(&self, id: &str) -> Result<Track> {
        read_json(
            self.request(Method::GET, &format!("/v1/jobs/{id}/result"))
                .timeout(Duration::from_secs(90))
                .send()
                .await?,
            MAX_RESULT_BYTES,
        )
        .await
    }

    pub async fn cancel(&self, id: &str) -> Result<Progress> {
        read_json(
            self.request(Method::POST, &format!("/v1/jobs/{id}/cancel"))
                .send()
                .await?,
            MAX_CONTROL_BYTES,
        )
        .await
    }

    pub async fn stop(mut self) {
        // EOF also arrives when Studio exits without running this cleanup.
        drop(self.child.stdin.take());
        if tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}

async fn read_json<T: DeserializeOwned>(mut response: Response, limit: usize) -> Result<T> {
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= limit - body.len(),
            "Music adapter response exceeded its size limit"
        );
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let message = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|value| {
                value["error"]
                    .as_str()
                    .map(|text| text.chars().take(500).collect::<String>())
            })
            .unwrap_or_else(|| "request failed".into());
        anyhow::bail!("Music adapter HTTP {status}: {message}");
    }
    serde_json::from_slice(&body).context("invalid music adapter response")
}
