//! Engine manifests are data obtained from a named TUF target. Reading this
//! catalogue is offline and never installs or starts a process.
pub(super) mod install;

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::{ApiError, api_error, component_catalogue as channel, net};

#[derive(Debug, Deserialize, Serialize)]
struct Catalogue {
    schema: u32,
    engines: Vec<EnginePackage>,
}

#[derive(Debug, Deserialize, Serialize)]
struct EnginePackage {
    id: String,
    name: String,
    version: String,
    protocol: String,
    platform: String,
    executable: String,
    files: Vec<PackageFile>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PackageFile {
    name: String,
    bytes: u64,
    sha256: String,
    url: String,
}

// A component can install only direct children of its own version directory.
// Use the same portable filename rules even when browsing from another OS.
fn portable_name(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        });
    !device
        && value.len() <= 128
        && !value.ends_with('.')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn parse(bytes: &[u8]) -> Result<Catalogue> {
    let catalog: Catalogue = serde_json::from_slice(bytes)?;
    ensure!(catalog.schema == 1, "unsupported engine catalogue schema");
    let mut packages = HashSet::new();
    for package in &catalog.engines {
        for id in [
            &package.id,
            &package.version,
            &package.platform,
            &package.protocol,
        ] {
            ensure!(portable_name(id), "invalid engine package identifier");
        }
        ensure!(
            !package.name.trim().is_empty() && package.name.len() <= 160,
            "invalid engine display name"
        );
        ensure!(
            packages.insert((
                package.id.to_ascii_lowercase(),
                package.platform.to_ascii_lowercase()
            )),
            "duplicate engine for platform"
        );
        let mut names = HashSet::new();
        for file in &package.files {
            ensure!(
                portable_name(&file.name) && names.insert(file.name.to_ascii_lowercase()),
                "invalid or duplicate component file"
            );
            ensure!(
                file.bytes > 0
                    && file.sha256.len() == 64
                    && file
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
                "invalid component file pin"
            );
            let url = reqwest::Url::parse(&file.url).context("invalid component URL")?;
            ensure!(
                url.scheme() == "https"
                    && url.host().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.fragment().is_none(),
                "invalid component HTTPS source"
            );
        }
        ensure!(
            package
                .files
                .iter()
                .any(|file| file.name == package.executable),
            "executable absent from signed file list"
        );
    }
    Ok(catalog)
}

type Failure = (StatusCode, Json<ApiError>);

pub(super) async fn list() -> Result<Json<serde_json::Value>, Failure> {
    let load = async {
        match channel::cached_catalogue(channel::ENGINES).await? {
            Some(bytes) => {
                Ok(serde_json::json!({"source":"signed","engines":parse(&bytes)?.engines}))
            }
            None => Ok(serde_json::json!({"source":"bundled","engines":[]})),
        }
    };
    load.await.map(Json).map_err(|error: anyhow::Error| {
        api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}"))
    })
}

pub(super) async fn refresh() -> Result<Json<serde_json::Value>, Failure> {
    let client = net::download_client();
    let operation = channel::refresh_catalogue(channel::ENGINES, &client, parse);
    let catalog = tokio::time::timeout(std::time::Duration::from_secs(45), operation)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::GATEWAY_TIMEOUT,
                "Engine catalogue refresh timed out".into(),
            )
        })?
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    Ok(Json(
        serde_json::json!({"source":"signed","engines":catalog.engines}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signed_packages_can_name_new_engines_but_cannot_escape_their_install_directory() {
        let file = json!({"name":"worker.exe","bytes":40,"sha256":"a".repeat(64),"url":"https://example.org/worker.exe"});
        let package = json!({"id":"new-engine","name":"New engine","version":"1.1","platform":"windows-x86_64",
            "protocol":"music-http-v1","executable":"worker.exe","files":[file]});
        let input = |entry| serde_json::to_vec(&json!({"schema":1,"engines":[entry]})).unwrap();
        assert_eq!(
            parse(&input(package.clone())).unwrap().engines[0].id,
            "new-engine"
        );
        for bad in ["../worker.exe", "sub/worker.exe", "CON.dll", "engine."] {
            let mut changed = package.clone();
            changed["files"][0]["name"] = json!(bad);
            changed["executable"] = json!(bad);
            assert!(parse(&input(changed)).is_err());
        }
        let mut changed = package.clone();
        changed["files"].as_array_mut().unwrap().push(file.clone());
        assert!(parse(&input(changed)).is_err());
        let mut changed = package;
        changed["files"][0]["url"] = json!("file:///tmp/worker");
        assert!(parse(&input(changed)).is_err());
    }
}
