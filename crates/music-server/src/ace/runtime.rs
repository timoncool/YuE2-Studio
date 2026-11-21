use super::AceClient;
use anyhow::{bail, Context, Result};
use std::fs::OpenOptions;
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub executable: PathBuf,
    pub models_root: PathBuf,
    pub adapters_root: Option<PathBuf>,
    pub device: Option<String>,
    pub cuda_backend: Option<PathBuf>,
    pub managed_data_root: Option<PathBuf>,
    pub log_path: PathBuf,
}

#[derive(Default)]
pub struct Runtime {
    running: Mutex<Option<Running>>,
}

struct Running {
    launch: Launch,
    child: Child,
    client: AceClient,
    lease: Weak<()>,
}

impl Running {
    fn client_for_job(&mut self) -> AceClient {
        let lease = self.lease.upgrade().unwrap_or_else(|| Arc::new(()));
        self.lease = Arc::downgrade(&lease);
        AceClient {
            _lease: Some(lease),
            ..self.client.clone()
        }
    }

    fn idle(&self) -> Result<()> {
        if self.lease.upgrade().is_some() {
            bail!("ACE-Step is busy with an active job");
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill().context("stop ACE-Step")?;
            self.child.wait().context("wait for ACE-Step to stop")?;
        }
        Ok(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl Runtime {
    /// Hold the returned client until the submitted job finishes or is cancelled.
    /// The client keeps the active process from being stopped or changed.
    pub async fn start(&self, launch: Launch) -> Result<AceClient> {
        let mut running = self.running.lock().await;
        if let Some(current) = running.as_mut() {
            if current.child.try_wait()?.is_none() {
                if current.launch == launch {
                    if current.lease.strong_count() > 0 || current.client.health().await {
                        return Ok(current.client_for_job());
                    }
                }
                current.idle()?;
            }
            current.stop()?;
        }
        *running = None;
        let mut process = launch.spawn()?;
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if let Some(status) = process.child.try_wait()? {
                bail!(
                    "ACE-Step exited with {status}. See {}",
                    process.launch.log_path.display()
                );
            }
            if process.client.health().await {
                let client = process.client_for_job();
                *running = Some(process);
                return Ok(client);
            }
            if Instant::now() >= deadline {
                bail!(
                    "ACE-Step did not become ready. See {}",
                    process.launch.log_path.display()
                );
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.lock().await;
        if let Some(current) = running.as_mut() {
            current.idle()?;
            current.stop()?;
        }
        *running = None;
        Ok(())
    }

    pub async fn health(&self) -> bool {
        let client = self
            .running
            .lock()
            .await
            .as_ref()
            .map(|current| current.client.clone());
        match client {
            Some(client) => client.health().await,
            None => false,
        }
    }
}

impl Launch {
    fn spawn(self) -> Result<Running> {
        let executable = self
            .executable
            .canonicalize()
            .context("locate the ACE-Step executable")?;
        let models_root = self
            .models_root
            .canonicalize()
            .context("locate the ACE-Step models")?;
        if !executable.is_file() || !models_root.is_dir() {
            bail!("ACE-Step requires a server executable and a model directory");
        }
        let cuda_backend = self
            .cuda_backend
            .as_ref()
            .map(std::fs::canonicalize)
            .transpose()
            .context("locate the ACE-Step CUDA backend")?;
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
            .local_addr()?
            .port();
        let mut command = Command::new(&executable);
        command
            .current_dir(
                executable
                    .parent()
                    .context("ACE-Step executable has no directory")?,
            )
            .arg("--models")
            .arg(models_root)
            .arg("--host")
            .arg(Ipv4Addr::LOCALHOST.to_string())
            .arg("--port")
            .arg(port.to_string())
            .stdin(Stdio::null());
        if let Some(adapters) = &self.adapters_root {
            let adapters = adapters
                .canonicalize()
                .context("locate the ACE-Step adapters")?;
            if !adapters.is_dir() {
                bail!("the ACE-Step adapter path must name a directory");
            }
            command.arg("--adapters").arg(adapters);
        }
        if let Some(device) = &self.device {
            command.arg("--device").arg(device);
        }
        if let Some(path) = music_core::process::own_libraries_first(&executable) {
            command.env("LD_LIBRARY_PATH", path);
        }
        dub_core::proc::outside_appimage(&mut command);
        if let Some(root) = &self.managed_data_root {
            let mut paths = crate::native_music_runtime::runtime_paths(&executable, root);
            if cfg!(target_os = "linux") {
                command.env("LD_LIBRARY_PATH", std::env::join_paths(paths)?);
            } else if cfg!(windows) {
                let inherited = std::env::var_os("PATH").unwrap_or_default();
                paths.extend(std::env::split_paths(&inherited));
                command.env("PATH", std::env::join_paths(paths)?);
            }
        }
        match cuda_backend {
            Some(path) => {
                command.env("STUDIO_CUDA_BACKEND", path);
            }
            None => {
                command.env_remove("STUDIO_CUDA_BACKEND");
            }
        }
        if let Some(parent) = self
            .log_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .with_context(|| format!("open the ACE-Step log {}", self.log_path.display()))?;
        command
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        let child = music_core::process::spawn_dying_with_parent(command)
            .with_context(|| format!("start ACE-Step at {}", executable.display()))?;
        music_core::process::adopt(&child);
        Ok(Running {
            launch: self,
            child,
            client: AceClient::new(&format!("http://127.0.0.1:{port}")),
            lease: Weak::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};

    #[tokio::test]
    async fn same_launch_reuses_process_and_profile_change_waits_for_job() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let http = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/health", get(|| async { "ok" })),
            )
            .await
            .unwrap();
        });
        #[cfg(windows)]
        let mut fixture = {
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            command
        };
        #[cfg(not(windows))]
        let mut fixture = {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };
        let child = fixture.spawn().unwrap();
        let pid = child.id();
        let launch = Launch {
            executable: PathBuf::new(),
            models_root: PathBuf::from("original"),
            adapters_root: None,
            device: None,
            cuda_backend: None,
            managed_data_root: None,
            log_path: PathBuf::new(),
        };
        let runtime = Runtime {
            running: Mutex::new(Some(Running {
                launch: launch.clone(),
                child,
                client: AceClient::new(&format!("http://{address}")),
                lease: Weak::new(),
            })),
        };
        let first = runtime.start(launch.clone()).await.unwrap();
        let retry = runtime.start(launch.clone()).await.unwrap();
        assert_eq!(first.base_url(), retry.base_url());
        let changed = Launch {
            models_root: PathBuf::from("other"),
            ..launch
        };
        assert!(runtime
            .start(changed)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("busy"));
        drop(first);
        assert!(runtime
            .stop()
            .await
            .unwrap_err()
            .to_string()
            .contains("busy"));
        assert_eq!(
            runtime.running.lock().await.as_ref().unwrap().child.id(),
            pid
        );
        drop(retry);
        assert!(runtime.health().await);
        runtime.stop().await.unwrap();
        assert!(!runtime.health().await);
        http.abort();
    }
}
