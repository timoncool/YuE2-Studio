//! The weights of the native ACE-Step and MiniMax Music3 sets, pinned to the same
//! repository commits and SHA-256 as the released ACE-Step Studio and MiniMax Music3
//! Studio, and the download of what a chosen set lacks into `models/<engine>/`.

use std::sync::{Arc, OnceLock};

use axum::{Json, extract::Path, http::StatusCode};
use serde::Deserialize;
use serde_json::Value;

use crate::downloads::{Asset, AssetKind, Downloader};
use crate::{ApiError, api_error};

macro_rules! weight {
    ($engine:literal, $repo:literal, $rev:literal, $name:literal, $size:literal, $sha:literal) => {
        (
            Asset {
                id: $name,
                label: $name,
                kind: AssetKind::Model,
                url: concat!("https://huggingface.co/", $repo, "/resolve/", $rev, "/", $name),
                relative_path: concat!("models/", $engine, "/", $name),
                bytes: $size,
                unzip_into: None,
                marker: $name,
                pick: &[],
                vram_gb: None,
                note: "",
            },
            $sha,
        )
    };
}

macro_rules! ace {
    ($name:literal, $size:literal, $sha:literal) => {
        weight!("acestep-cpp", "Serveurperso/ACE-Step-1.5-GGUF", "666ac70204440867d8c01ba4b119cc79c95b370a", $name, $size, $sha)
    };
}

macro_rules! ace_mxfp4 {
    ($name:literal, $size:literal, $sha:literal) => {
        weight!("acestep-cpp", "scragnog/Ace-Step-1.5-MXFP4-Quants", "05a04ec24513e44f21947a2d1ee8d1cae91d1095", $name, $size, $sha)
    };
}

macro_rules! minimax {
    ($name:literal, $size:literal, $sha:literal) => {
        weight!("minimaxmusic-cpp", "Serveurperso/MiniMax-Music3-GGUF", "9cdffedb54de2509ae55a6831a677645fb353a7d", $name, $size, $sha)
    };
}

macro_rules! minimax_light {
    ($name:literal, $size:literal, $sha:literal) => {
        weight!("minimaxmusic-cpp", "scragnog/MiniMax-Music3-GGUF", "6781ce79b21beb7413f6b2358cd4adb355217c3d", $name, $size, $sha)
    };
}

const ACE: [(Asset, &str); 13] = [
    ace!("acestep-v15-turbo-Q4_K_M.gguf", 1445710272, "55b4d8514850f3d0f82536f37e99673aaf48df802b5ae5b153eea32a2e2daa5e"),
    ace!("acestep-5Hz-lm-0.6B-Q8_0.gguf", 709846656, "bdaf9e292d4470f31c19cafeaca1b74936a114667e3a85e5d33b65247e9908ec"),
    ace!("Qwen3-Embedding-0.6B-Q8_0.gguf", 784144960, "972f23255e46adfe744a0eb9a0039f3c63988f65753b0968d776e8b27168c321"),
    ace!("vae-BF16.gguf", 337420928, "0599862ac5d15cd308e1d2e368373aea6c02e25ebd1737ad4a4562a0901b0ef8"),
    ace!("acestep-v15-xl-turbo-Q4_K_M.gguf", 2989922688, "475e60e8644b1f91eebc0cd25f24c060bed06e64a862b4e60ffafe634c740478"),
    ace!("acestep-5Hz-lm-1.7B-Q8_0.gguf", 1975837568, "726f99a82f050b32ebc5c5e36acaa9d7acd06bfe5e579a62b00e0d0d6d0b7ec6"),
    ace!("acestep-v15-xl-turbo-Q6_K.gguf", 4098812992, "e1f6665567e9bc0331b8dfd95c2b43a84e8bf114075c9b16abdd25ae97dc58fe"),
    ace_mxfp4!("acestep-v15-xl-turbo-MXFP4.gguf", 2660726464, "dfb0ba27f2dd25c6bb31ad7f1a7c8d6e5c538593c5fff92e3329d186c81751e4"),
    ace!("acestep-v15-xl-turbo-Q8_0.gguf", 5305828736, "4f1044fb646374fb5730e10f64325766883eb2fc02a643c1b403f2d61f39dc19"),
    ace!("acestep-5Hz-lm-4B-Q8_0.gguf", 4457323648, "972f91147a167f0c041f1b158d67985a82c0f6a852e68cdf70e46030cf08b1bc"),
    ace!("acestep-v15-xl-turbo-BF16.gguf", 9978530816, "10d445a71653a8fef9a16a8457d9fb66da432ac500b357cce028f36c489c1b63"),
    ace!("acestep-5Hz-lm-4B-BF16.gguf", 8384454592, "9fca9915919a1108da33d8d98fbe6c1a07bfee101d27098416650dcbde037b72"),
    ace!("Qwen3-Embedding-0.6B-BF16.gguf", 1196890592, "6f57900773fbd447b2943bc8126e2dc68ed009151ba89a47c14d1be05d4c73df"),
];

const MINIMAX: [(Asset, &str); 18] = [
    minimax_light!("mm3-lm-Q3_K_M.gguf", 4589971808, "65a1ee35212547a95ed19067c5b3bc3492725d55e754729e693e4141396064a5"),
    minimax_light!("mm3-depth-Q4_K_M.gguf", 404836256, "77e729d6e0599ed7928a3c0f13a36e1d3ef5f7de9259eaf01a19445549fa958e"),
    minimax!("MiniMax-Music3-condition_encoder-F32.gguf", 100672192, "ebb69ec6e6d730b4dcc48ba1b51da4f201514cbaaa7cae0c8d6259d3b178efb0"),
    minimax_light!("mm3-dit-Q3_K_M.gguf", 1138427712, "f1a67119c34ede23e27240d3ebd630fa492247e1d0184263574482302ab48f3f"),
    minimax!("MiniMax-Music3-vocoder-F32.gguf", 306102784, "4eaa451e54fa755cfe7b0fd15b0bfe64458db35b822e1d250488d0a2a363507d"),
    minimax_light!("mm3-lm-Q4_K_M.gguf", 5512456544, "8d8e21098e1027b963776bbe9790609191e95610caa6e7cb53c26830b3efc94b"),
    minimax_light!("mm3-dit-Q4_K_S.gguf", 1389823808, "e20449c3f6a73a8cc2672947f2ee41728b12f0dcfd351c1c9a34a04a1c193031"),
    minimax!("MiniMax-Music3-language_model-Q6_K.gguf", 7048279328, "ca605cbae894696f9c111bfef33023b29722e93fe2c287071250b0af1a0e5416"),
    minimax!("MiniMax-Music3-rvq_depth_decoder-Q8_0.gguf", 686513600, "7da73a953747f3f40857ba266a53eeb5487adbe473d4e7a53c99d5eaf9e5c3d5"),
    minimax!("MiniMax-Music3-transformer-Q5_K_M.gguf", 1692794336, "35d1ed8902237215064a4bb5a9bf3e03507d65b89146bea9e24e915cb4c5c8ce"),
    minimax!("MiniMax-Music3-language_model-Q8_0.gguf", 9127257376, "9ffa190eb5892c5c0829014dc3070b9ce8d2464636bbaae884b08f1bf6de0ad3"),
    minimax!("MiniMax-Music3-transformer-Q8_0.gguf", 2602401248, "cbadca0600f325ba9263ea4dcf0d71d0361abf0733303398b9995fbadac6b38e"),
    minimax_light!("mm3-lm-NVFP4.gguf", 5656222048, "5d1e9e238c447cd5671d5695431d7df433d04ab18550d9f8ac6616b7c14e0e3e"),
    minimax_light!("mm3-depth-NVFP4.gguf", 401493920, "33b9cf7b2324157ec13d5d9b105cc80e94104d93647229a9f83b9824efcba415"),
    minimax_light!("mm3-dit-NVFP4.gguf", 1383784256, "99fa7349f3e06fa79af4a4ab7b89dab490caeb034e543e83a51844353abf41ed"),
    minimax!("MiniMax-Music3-language_model-BF16.gguf", 17174297792, "6fd8a735ed9bba12c620f86504bb062a4ed5ba2237615f318dcfe14ee911c4ae"),
    minimax!("MiniMax-Music3-rvq_depth_decoder-BF16.gguf", 1292053888, "d566347637257b86e4ce598907caa257be9794f70fc38861da70f0d8bd2f0686"),
    minimax!("MiniMax-Music3-transformer-F32.gguf", 9727655296, "ffcf7873d475448ba7b18b62b2495d7e1f2941f9fe5cba315557f6307367ae59"),
];

fn weights(engine: &str) -> Option<&'static [(Asset, &'static str)]> {
    match engine {
        crate::ace::ENGINE_ID => Some(&ACE),
        crate::native_minimax::ENGINE_ID => Some(&MINIMAX),
        _ => None,
    }
}

pub(crate) fn sha256(url: &str) -> Option<&'static str> {
    ACE.iter().chain(MINIMAX.iter()).find(|(asset, _)| asset.url == url).map(|(_, sha)| *sha)
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

fn profile_weights(engine: &str, profile: &str) -> Option<Vec<(&'static str, u64)>> {
    match engine {
        crate::ace::ENGINE_ID => crate::ace::profile_weights(profile),
        crate::native_minimax::ENGINE_ID => crate::native_minimax::profile_weights(profile),
        _ => None,
    }
}

/// A set runs from one folder, so what it lacks is counted in the studio's own
/// `models/<engine>`; a file an older studio holds is taken from it by the downloader.
fn missing_assets(engine: &str, profile: &str) -> Option<Vec<&'static Asset>> {
    let downloader = downloader()?;
    let table = weights(engine)?;
    profile_weights(engine, profile)?
        .into_iter()
        .filter_map(|(name, bytes)| {
            let pinned = table.iter().find(|(asset, _)| asset.label == name && asset.bytes == bytes);
            match pinned {
                Some((asset, _)) if std::fs::metadata(downloader.path_of(asset)).is_ok_and(|file| file.len() == bytes) => None,
                Some((asset, _)) => Some(Some(asset)),
                None => Some(None),
            }
        })
        .collect()
}

/// Adds what the window needs to offer a download to each profile row of
/// `/v1/music/native/<engine>/profiles`.
pub(crate) async fn describe(engine: &str, profiles: &mut Value) {
    let Some(rows) = profiles.as_array_mut().filter(|_| weights(engine).is_some()) else { return };
    let installing = lock(engine).is_some_and(|lock| lock.try_lock().is_err());
    for row in rows {
        let Some(id) = row["id"].as_str().map(str::to_owned) else { continue };
        let assets = missing_assets(engine, &id);
        let download = match downloader() {
            Some(downloader) => downloader.active_for(&scope(engine, &id)).await,
            None => None,
        };
        let busy = installing && download.as_ref().is_some_and(|download| !download.done);
        row["installable"] = Value::Bool(assets.is_some());
        row["missing_bytes"] = assets
            .as_ref()
            .map_or(Value::Null, |assets| Value::from(assets.iter().map(|asset| asset.bytes).sum::<u64>()));
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
    let assets = missing_assets(&engine, &request.model).ok_or_else(|| {
        api_error(StatusCode::NOT_FOUND, format!("No pinned download for the {engine} set {}.", request.model))
    })?;
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
    let Some(downloader) = downloader().filter(|_| weights(&engine).is_some()) else {
        return Err(api_error(StatusCode::NOT_FOUND, format!("No native model sets for {engine}.")));
    };
    let cancelled = downloader.cancel_for(&scope(&engine, &request.model)).await;
    Ok(Json(serde_json::json!({"cancelled": cancelled})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_of_every_native_set_has_a_pinned_download() {
        let sets = crate::ace::profile_ids()
            .map(|id| (crate::ace::ENGINE_ID, id))
            .chain(crate::native_minimax::PROFILES.iter().map(|profile| (crate::native_minimax::ENGINE_ID, profile.id)));
        for (engine, id) in sets {
            for (name, bytes) in profile_weights(engine, id).unwrap() {
                let pinned = weights(engine).unwrap().iter().find(|(asset, _)| asset.label == name);
                assert!(pinned.is_some_and(|(asset, _)| asset.bytes == bytes), "{engine} {id}: {name} has no download of {bytes} bytes");
            }
        }
        for (asset, sha) in ACE.iter().chain(MINIMAX.iter()) {
            assert_eq!(sha.len(), 64, "{}", asset.label);
            assert!(!asset.url.contains("/resolve/main/"), "{}", asset.url);
            assert_eq!(crate::pin_sha::sha256(asset.url), Some(*sha));
        }
    }
}
