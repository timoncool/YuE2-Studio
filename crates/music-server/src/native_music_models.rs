//! Downloading the weights of the native ACE-Step and MiniMax Music3 sets into
//! `models/<engine>/`. The sets and their files come from the component catalogue.

use std::sync::{Arc, OnceLock};

use axum::{Json, extract::Path, http::StatusCode};
use serde::Deserialize;
use serde_json::Value;

use crate::downloads::{Asset, Downloader};
use crate::{ApiError, api_error};

fn native(engine: &str) -> bool {
    engine == crate::ace::ENGINE_ID || engine == crate::native_minimax::ENGINE_ID
}

fn downloader() -> Option<&'static Arc<Downloader>> {
    static DOWNLOADER: OnceLock<Option<Arc<Downloader>>> = OnceLock::new();
    DOWNLOADER
        .get_or_init(|| crate::studio_data_root().map(|root| Arc::new(Downloader::new(root))))
        .as_ref()
}

fn lock(engine: &str) -> Option<&'static tokio::sync::Mutex<()>> {
    static ACE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static MINIMAX_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    match engine {
        crate::ace::ENGINE_ID => Some(&ACE_LOCK),
        crate::native_minimax::ENGINE_ID => Some(&MINIMAX_LOCK),
        _ => None,
    }
}

fn scope(engine: &str, profile: &str) -> String {
    format!("{engine}:{profile}")
}

/// The catalogue files of a set that are not yet in the studio's own `models/<engine>`;
/// a file an older studio holds is taken from it by the downloader.
fn missing_assets(engine: &str, profile: &str) -> anyhow::Result<Vec<&'static Asset>> {
    use anyhow::Context;
    let downloader = downloader().context("missing studio data folder")?;
    let set = crate::components::sets(engine)
        .into_iter()
        .find(|set| set.id == profile)
        .with_context(|| format!("the component catalogue has no {engine} set {profile}"))?;
    let mut missing = Vec::new();
    for id in set.roles.values() {
        let asset = crate::components::asset(id)?;
        if !std::fs::metadata(downloader.path_of(asset)).is_ok_and(|file| file.len() == asset.bytes) {
            missing.push(asset);
        }
    }
    missing.dedup_by_key(|asset| asset.id);
    Ok(missing)
}

/// Adds what the window needs to offer a download to each profile row of
/// `/v1/music/native/<engine>/profiles`.
pub(crate) async fn describe(engine: &str, profiles: &mut Value) {
    let Some(rows) = profiles.as_array_mut().filter(|_| native(engine)) else { return };
    let installing = lock(engine).is_some_and(|lock| lock.try_lock().is_err());
    for row in rows {
        let Some(id) = row["id"].as_str().map(str::to_owned) else { continue };
        let assets = missing_assets(engine, &id);
        let download = match downloader() {
            Some(downloader) => downloader.active_for(&scope(engine, &id)).await,
            None => None,
        };
        let busy = installing && download.as_ref().is_some_and(|download| !download.done);
        match &assets {
            Ok(assets) => {
                row["installable"] = Value::Bool(true);
                row["missing_bytes"] = Value::from(assets.iter().map(|asset| asset.bytes).sum::<u64>());
            }
            Err(error) => {
                row["installable"] = Value::Bool(false);
                row["install_error"] = Value::String(format!("{error:#}"));
            }
        }
        row["installing"] = Value::Bool(busy);
        row["download"] = serde_json::to_value(download).unwrap_or(Value::Null);
        row["install_path"] = Value::String(format!("/v1/music/native/{engine}/profiles/install"));
    }
}

#[derive(Deserialize)]
pub(crate) struct InstallRequest {
    model: String,
}

type Failure = (StatusCode, Json<ApiError>);

pub(crate) async fn install(Path(engine): Path<String>, Json(request): Json<InstallRequest>) -> Result<Json<Value>, Failure> {
    let (Some(downloader), Some(lock)) = (downloader(), lock(&engine)) else {
        return Err(api_error(StatusCode::NOT_FOUND, format!("No native model sets for {engine}.")));
    };
    let assets = missing_assets(&engine, &request.model)
        .map_err(|error| api_error(StatusCode::NOT_FOUND, format!("{error:#}")))?;
    if assets.is_empty() {
        return Ok(Json(serde_json::json!({"ready": true})));
    }
    let Ok(guard) = lock.try_lock() else {
        return Ok(Json(serde_json::json!({"installing": true})));
    };
    let scope = scope(&engine, &request.model);
    downloader.clear_finished(&scope).await;
    tokio::spawn(async move {
        let _guard = guard;
        if let Err(error) = downloader.install_all(&scope, &assets).await {
            downloader.fail(&scope, format!("{error:#}")).await;
        }
        crate::mcp::announce("music_pair");
    });
    Ok(Json(serde_json::json!({"installing": true})))
}

pub(crate) async fn cancel(Path(engine): Path<String>, Json(request): Json<InstallRequest>) -> Result<Json<Value>, Failure> {
    let Some(downloader) = downloader().filter(|_| native(&engine)) else {
        return Err(api_error(StatusCode::NOT_FOUND, format!("No native model sets for {engine}.")));
    };
    let cancelled = downloader.cancel_for(&scope(&engine, &request.model)).await;
    Ok(Json(serde_json::json!({"cancelled": cancelled})))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_native_set_has_every_role_with_a_pinned_download() {
        for (engine, roles) in [
            (crate::ace::ENGINE_ID, &crate::ace::ROLES[..]),
            (crate::native_minimax::ENGINE_ID, &crate::native_minimax::ROLES[..]),
        ] {
            let sets = crate::components::sets(engine);
            assert!(!sets.is_empty(), "{engine} has no sets in the catalogue");
            for set in sets {
                for role in roles {
                    let id = set.roles.get(*role).unwrap_or_else(|| panic!("{engine}/{} has no {role}", set.id));
                    let asset = crate::components::asset(id).unwrap();
                    assert!(!asset.url.contains("/resolve/main/"), "{}", asset.url);
                    assert!(crate::components::sha256(asset.url).is_some(), "{}", asset.url);
                    assert!(asset.relative_path.starts_with(&format!("models/{engine}/")), "{}", asset.relative_path);
                }
            }
        }
    }
}
