use axum::{extract::Path, http::StatusCode, Json};
use serde::Serialize;
use std::{
    collections::{hash_map::Entry, HashMap},
    path::{Path as FilePath, PathBuf},
    sync::{Mutex, OnceLock},
};

use crate::{api_error, rvc_voices::ApiFailure};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Preparing,
    DownloadingWeights,
    DownloadingIndex,
    Verifying,
}

#[derive(Clone)]
struct Active {
    stage: Stage,
    file: Option<PathBuf>,
    total_bytes: u64,
}

#[derive(Serialize)]
pub(super) struct Progress {
    stage: Stage,
    downloaded_bytes: u64,
    total_bytes: u64,
}

fn active() -> &'static Mutex<HashMap<String, Active>> {
    static PROGRESS: OnceLock<Mutex<HashMap<String, Active>>> = OnceLock::new();
    PROGRESS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) struct Installation<'a>(&'a str);

impl<'a> Installation<'a> {
    pub(super) fn begin(id: &'a str) -> Result<Self, ApiFailure> {
        match active()
            .lock()
            .expect("RVC install progress")
            .entry(id.to_owned())
        {
            Entry::Vacant(entry) => {
                entry.insert(Active {
                    stage: Stage::Preparing,
                    file: None,
                    total_bytes: 0,
                });
                Ok(Self(id))
            }
            Entry::Occupied(_) => Err(api_error(
                StatusCode::CONFLICT,
                "This RVC voice is already being installed.".into(),
            )),
        }
    }
}

impl Drop for Installation<'_> {
    fn drop(&mut self) {
        active()
            .lock()
            .expect("RVC install progress")
            .remove(self.0);
    }
}

pub(super) fn downloading(id: &str, stage: Stage, file: &FilePath, total_bytes: u64) {
    if let Some(install) = active().lock().expect("RVC install progress").get_mut(id) {
        *install = Active {
            stage,
            file: Some(file.to_owned()),
            total_bytes,
        };
    }
}

pub(super) fn verifying(id: &str) {
    if let Some(install) = active().lock().expect("RVC install progress").get_mut(id) {
        *install = Active {
            stage: Stage::Verifying,
            file: None,
            total_bytes: 0,
        };
    }
}

pub(super) async fn get(Path(id): Path<String>) -> Json<Option<Progress>> {
    let current = active()
        .lock()
        .expect("RVC install progress")
        .get(&id)
        .cloned();
    let Some(Active {
        stage,
        file,
        total_bytes,
    }) = current
    else {
        return Json(None);
    };
    let downloaded_bytes = match file {
        Some(path) => tokio::fs::metadata(path)
            .await
            .map_or(0, |metadata| metadata.len().min(total_bytes)),
        None => 0,
    };
    Json(Some(Progress {
        stage,
        downloaded_bytes,
        total_bytes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn progress_reports_bytes_from_the_staged_file_and_ends_on_drop() {
        let id = "voice-with-progress-test";
        let installation = Installation::begin(id).ok().unwrap();
        assert!(Installation::begin(id).is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"12345").unwrap();
        downloading(id, Stage::DownloadingIndex, file.path(), 8);
        let progress = get(Path(id.into())).await.0.unwrap();
        assert!(matches!(progress.stage, Stage::DownloadingIndex));
        assert_eq!((progress.downloaded_bytes, progress.total_bytes), (5, 8));
        verifying(id);
        assert!(matches!(
            get(Path(id.into())).await.0.unwrap().stage,
            Stage::Verifying
        ));
        drop(installation);
        assert!(get(Path(id.into())).await.0.is_none());
    }
}
