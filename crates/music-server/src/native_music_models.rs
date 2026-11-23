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

/// The memory a set may take: the card's own on a graphics card; on a Mac, whose memory macOS and the studio share,
/// half of it up to 16 GiB and three quarters above.
fn set_budget_gb(hardware: &crate::hardware::Hardware) -> f64 {
    if !cfg!(target_os = "macos") {
        return hardware.total_vram_gb;
    }
    let unified = hardware.total_vram_gb.max(hardware.total_ram_gb);
    unified * if unified <= 16.0 { 0.5 } else { 0.75 }
}

/// The main set the card holds with the most memory to spare for quality: the largest whose video memory fits, the
/// smallest when none does.
fn recommended(sets: &[crate::components::ModelSet], vram_gb: f64) -> Option<String> {
    let main: Vec<_> = sets.iter().filter(|set| !set.advanced && set.vram_gb > 0.0).collect();
    let fits = main.iter().filter(|set| set.vram_gb <= vram_gb).max_by(|a, b| a.vram_gb.total_cmp(&b.vram_gb));
    fits.or_else(|| main.iter().min_by(|a, b| a.vram_gb.total_cmp(&b.vram_gb))).map(|set| set.id.clone())
}

/// Adds what the window needs to offer a download to each profile row of
/// `/v1/music/native/<engine>/profiles`.
pub(crate) async fn describe(engine: &str, profiles: &mut Value) {
    let Some(rows) = profiles.as_array_mut().filter(|_| native(engine)) else { return };
    let installing = lock(engine).is_some_and(|lock| lock.try_lock().is_err());
    let sets = crate::components::sets(engine);
    let recommended = recommended(&sets, set_budget_gb(&crate::hardware::hardware()));
    for row in rows {
        let Some(id) = row["id"].as_str().map(str::to_owned) else { continue };
        let set = sets.iter().find(|set| set.id == id);
        row["advanced"] = Value::Bool(set.is_some_and(|set| set.advanced));
        if let Some(set) = set {
            row["labels"] = serde_json::to_value(&set.label).unwrap_or(Value::Null);
            row["roles"] = serde_json::to_value(&set.roles).unwrap_or(Value::Null);
        }
        row["recommended"] = Value::Bool(recommended.as_deref() == Some(id.as_str()));
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

/// The ACE-Step DiT files of the catalogue: a song can render with any of them in place of its set's own.
fn dit_assets() -> Vec<&'static Asset> {
    crate::components::files_under("")
        .into_iter()
        .filter(|(_, file)| file.part.as_deref() == Some("dit") && file.path.starts_with("models/acestep-cpp/"))
        .filter_map(|(id, _)| crate::components::asset(&id).ok())
        .collect()
}

fn dit_scope(file: &str) -> String {
    format!("{}:dit:{file}", crate::ace::ENGINE_ID)
}

/// Every DiT the ACE-Step form offers, whether it is on disk and how its download goes.
pub(crate) async fn dits() -> Result<Json<Value>, Failure> {
    let downloader = downloader().ok_or_else(|| api_error(StatusCode::SERVICE_UNAVAILABLE, "Studio data folder is unavailable.".into()))?;
    let mut rows = Vec::new();
    for asset in dit_assets() {
        let installed = std::fs::metadata(downloader.path_of(asset)).is_ok_and(|file| file.len() == asset.bytes);
        let download = downloader.active_for(&dit_scope(asset.id)).await;
        rows.push(serde_json::json!({"file": asset.id, "bytes": asset.bytes, "installed": installed, "download": download}));
    }
    Ok(Json(serde_json::json!({"dits": rows})))
}

#[derive(Deserialize)]
pub(crate) struct DitRequest {
    file: String,
}

pub(crate) async fn install_dit(Json(request): Json<DitRequest>) -> Result<Json<Value>, Failure> {
    let downloader = downloader().ok_or_else(|| api_error(StatusCode::SERVICE_UNAVAILABLE, "Studio data folder is unavailable.".into()))?;
    let asset = dit_assets().into_iter().find(|asset| asset.id == request.file)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("The catalogue has no ACE-Step DiT {}.", request.file)))?;
    if std::fs::metadata(downloader.path_of(asset)).is_ok_and(|file| file.len() == asset.bytes) {
        return Ok(Json(serde_json::json!({"ready": true})));
    }
    let scope = dit_scope(asset.id);
    if downloader.active_for(&scope).await.is_some_and(|download| !download.done) {
        return Ok(Json(serde_json::json!({"installing": true})));
    }
    downloader.clear_finished(&scope).await;
    tokio::spawn(async move {
        if let Err(error) = downloader.install_all(&scope, &[asset]).await {
            downloader.fail(&scope, format!("{error:#}")).await;
        }
        crate::mcp::announce("music_pair");
    });
    Ok(Json(serde_json::json!({"installing": true})))
}

pub(crate) async fn cancel_dit(Json(request): Json<DitRequest>) -> Result<Json<Value>, Failure> {
    let downloader = downloader().ok_or_else(|| api_error(StatusCode::SERVICE_UNAVAILABLE, "Studio data folder is unavailable.".into()))?;
    Ok(Json(serde_json::json!({"cancelled": downloader.cancel_for(&dit_scope(&request.file)).await})))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_recommended_set_is_the_largest_main_one_the_card_holds() {
        let ace = crate::components::sets(crate::ace::ENGINE_ID);
        assert_eq!(super::recommended(&ace, 8.0).as_deref(), Some("recommended-light"));
        assert_eq!(super::recommended(&ace, 12.0).as_deref(), Some("balanced"));
        assert_eq!(super::recommended(&ace, 24.0).as_deref(), Some("quality-q8"));
        assert_eq!(super::recommended(&ace, 2.0).as_deref(), Some("minimal"));
        let minimax = crate::components::sets(crate::native_minimax::ENGINE_ID);
        assert_eq!(super::recommended(&minimax, 16.0).as_deref(), Some("quality-q8"));
        assert!(minimax.iter().filter(|set| !set.advanced).count() == 4);
    }

    #[test]
    fn the_form_offers_every_ace_dit_of_the_catalogue_but_full_weights() {
        let dits = super::dit_assets();
        for file in ["acestep-v15-base-Q8_0.gguf", "acestep-v15-xl-base-Q4_K_M.gguf", "acestep-v15-xl-turbo-Q8_0.gguf", "acestep_1.5_xl_turbo_regrind_v1-Q8_0.gguf"] {
            assert!(dits.iter().any(|asset| asset.id == file), "{file}");
        }
        for asset in &dits {
            assert!(!asset.id.contains("BF16"), "{}", asset.id);
            assert!(!asset.url.contains("/resolve/main/") && crate::components::sha256(asset.url).is_some(), "{}", asset.url);
        }
    }

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
