//! Short-lived installation state. The install request owns the entry and the
//! staging directory; cancelling it leaves the previous generation selected.
use axum::{Json, extract::Path};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::watch;

type Key = (String, String);
static ACTIVE: LazyLock<Mutex<HashMap<Key, Arc<Transfer>>>> = LazyLock::new(Mutex::default);

#[derive(Clone, Serialize)]
struct Status {
    stage: &'static str,
    file: Option<String>,
    total_bytes: u64,
    downloaded_bytes: u64,
    reused_bytes: u64,
}

pub(super) struct Transfer {
    status: Mutex<Status>,
    cancel: watch::Sender<bool>,
}

pub(super) struct Installation {
    key: Key,
    pub(super) transfer: Arc<Transfer>,
}

impl Installation {
    pub(super) fn begin(
        kind: &str,
        id: &str,
        total_bytes: u64,
    ) -> anyhow::Result<(Self, watch::Receiver<bool>)> {
        let key = (kind.to_owned(), id.to_owned());
        let mut jobs = ACTIVE.lock().expect("package progress");
        anyhow::ensure!(
            !jobs.contains_key(&key),
            "this package is already being installed"
        );
        let (cancel, signal) = watch::channel(false);
        let transfer = Arc::new(Transfer {
            status: Mutex::new(Status {
                stage: "verifying",
                file: None,
                total_bytes,
                downloaded_bytes: 0,
                reused_bytes: 0,
            }),
            cancel,
        });
        jobs.insert(key.clone(), transfer.clone());
        Ok((Self { key, transfer }, signal))
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        ACTIVE.lock().expect("package progress").remove(&self.key);
    }
}

impl Transfer {
    pub(super) fn stage(&self, stage: &'static str, file: Option<&str>) {
        let mut status = self.status.lock().expect("package progress");
        status.stage = stage;
        status.file = file.map(str::to_owned);
    }
    pub(super) fn downloaded(&self, bytes: u64) {
        self.status
            .lock()
            .expect("package progress")
            .downloaded_bytes += bytes;
    }
    pub(super) fn downloaded_bytes(&self) -> u64 {
        self.status
            .lock()
            .expect("package progress")
            .downloaded_bytes
    }
    pub(super) fn set_downloaded_bytes(&self, bytes: u64) {
        self.status
            .lock()
            .expect("package progress")
            .downloaded_bytes = bytes;
    }
    pub(super) fn reused(&self, bytes: u64) {
        self.status.lock().expect("package progress").reused_bytes += bytes;
    }
}

pub(crate) async fn status(Path(key): Path<Key>) -> Json<serde_json::Value> {
    let jobs = ACTIVE.lock().expect("package progress");
    let current = jobs
        .get(&key)
        .map(|job| job.status.lock().expect("package progress").clone());
    Json(serde_json::json!({"active":current}))
}

pub(crate) async fn cancel(Path(key): Path<Key>) -> Json<serde_json::Value> {
    let jobs = ACTIVE.lock().expect("package progress");
    let requested = jobs
        .get(&key)
        .is_some_and(|job| job.cancel.send(true).is_ok());
    Json(serde_json::json!({"cancel_requested":requested}))
}
