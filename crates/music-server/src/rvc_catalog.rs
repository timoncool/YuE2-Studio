use anyhow::Context;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

mod archive;

use crate::{
    api_error, net,
    rvc_voices::{self, ApiFailure, StagedFile, Voice, VoiceOrigin},
    update_data_root, AppState,
};

const BUNDLED: &str = include_str!("../../../config/rvc-voice-catalog.json");

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    voices: Vec<CatalogVoice>,
}

#[derive(Deserialize, Serialize)]
struct CatalogVoice {
    id: String,
    name: String,
    source_repo: String,
    source_url: String,
    revision: String,
    license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    weights: Option<RemoteFile>,
    index: Option<RemoteFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive: Option<RemoteArchive>,
    metadata: Option<serde_json::Value>,
}

#[derive(Deserialize, Serialize)]
struct RemoteFile {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Deserialize, Serialize)]
struct RemoteArchive {
    #[serde(flatten)]
    file: RemoteFile,
    weights_sha256: String,
    index_sha256: String,
}

fn parse_hash(sha256: &str) -> anyhow::Result<[u8; 32]> {
    anyhow::ensure!(
        sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid SHA-256."
    );
    let mut hash = [0; 32];
    for (offset, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&sha256[offset * 2..offset * 2 + 2], 16)?;
    }
    Ok(hash)
}

impl RemoteFile {
    fn hash(&self) -> anyhow::Result<[u8; 32]> {
        parse_hash(&self.sha256)
    }

    fn name(&self) -> &str {
        self.url.rsplit('/').next().unwrap_or(&self.url)
    }
}

impl CatalogVoice {
    fn local_id(&self) -> anyhow::Result<String> {
        if let Some(archive) = &self.archive {
            return Ok(rvc_voices::voice_id(
                &parse_hash(&archive.weights_sha256)?,
                Some(&parse_hash(&archive.index_sha256)?),
            ));
        }
        let weights = self.weights.as_ref().context("Missing RVC weights.")?;
        let index = self.index.as_ref().map(RemoteFile::hash).transpose()?;
        Ok(rvc_voices::voice_id(&weights.hash()?, index.as_ref()))
    }

    fn download_bytes(&self) -> u64 {
        if let Some(archive) = &self.archive {
            return archive.file.size;
        }
        self.weights
            .as_ref()
            .map_or(0, |file| file.size)
            .saturating_add(self.index.as_ref().map_or(0, |file| file.size))
    }

    fn has_index(&self) -> bool {
        self.archive.is_some() || self.index.is_some()
    }

    fn origin(&self) -> VoiceOrigin {
        VoiceOrigin {
            catalog_id: self.id.clone(),
            name: self.name.clone(),
            source_repo: self.source_repo.clone(),
            revision: self.revision.clone(),
            source_url: self.source_url.clone(),
            license: self.license.clone(),
        }
    }
}

fn parse_manifest(source: &str) -> anyhow::Result<Vec<CatalogVoice>> {
    let manifest: Manifest = serde_json::from_str(source)?;
    anyhow::ensure!(
        manifest.schema_version == 1,
        "Unsupported RVC catalog version."
    );
    let mut ids = std::collections::HashSet::new();
    for voice in &manifest.voices {
        anyhow::ensure!(
            !voice.id.is_empty() && ids.insert(&voice.id),
            "Invalid or duplicate RVC catalog ID."
        );
        anyhow::ensure!(!voice.name.is_empty(), "RVC catalog voice has no name.");
        anyhow::ensure!(
            voice.source_repo.split('/').count() == 2
                && voice.source_repo.split('/').all(|part| !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))),
            "Invalid RVC source repository."
        );
        let revision = &voice.revision;
        anyhow::ensure!(
            revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid RVC catalog revision."
        );
        let repo = format!("https://huggingface.co/{}", voice.source_repo);
        anyhow::ensure!(
            voice.source_url == repo || voice.source_url.starts_with(&format!("{repo}/")),
            "Invalid RVC catalog source URL."
        );
        let prefix = format!("{repo}/resolve/{revision}/");
        anyhow::ensure!(
            (voice.weights.is_some() ^ voice.archive.is_some())
                && (voice.archive.is_none() || voice.index.is_none()),
            "Use either pinned raw RVC files or one pinned ZIP."
        );
        anyhow::ensure!(
            voice.download_bytes() <= rvc_voices::UPLOAD_LIMIT as u64,
            "RVC catalog files exceed the voice size limit."
        );
        if let Some(archive) = &voice.archive {
            validate_remote_file(&archive.file, &prefix, &[".zip"])?;
            parse_hash(&archive.weights_sha256)?;
            parse_hash(&archive.index_sha256)?;
        }
        if let Some(weights) = &voice.weights {
            validate_remote_file(weights, &prefix, &[".pth", ".pt"])?;
        }
        if let Some(index) = &voice.index {
            validate_remote_file(index, &prefix, &[".index"])?;
        }
    }
    Ok(manifest.voices)
}

fn validate_remote_file(
    file: &RemoteFile,
    prefix: &str,
    extensions: &[&str],
) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(&file.url)?;
    anyhow::ensure!(
        file.url.starts_with(prefix)
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path().starts_with(reqwest::Url::parse(prefix)?.path())
            && url.path().ends_with(file.name())
            && extensions
                .iter()
                .any(|ext| file.name().to_ascii_lowercase().ends_with(ext))
            && file.size > 0,
        "Invalid pinned RVC file."
    );
    file.hash()?;
    Ok(())
}

#[derive(Serialize)]
pub(super) struct ListedVoice {
    #[serde(flatten)]
    voice: CatalogVoice,
    installed_id: Option<String>,
    download_bytes: u64,
    has_index: bool,
}

pub(super) async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<ListedVoice>>, ApiFailure> {
    let root = update_data_root(&state).join("rvc/voices");
    tokio::task::spawn_blocking(move || {
        parse_manifest(BUNDLED)?
            .into_iter()
            .map(|voice| {
                let installed_id =
                    rvc_voices::read_voice(&root, &voice.local_id()?).map(|installed| installed.id);
                Ok(ListedVoice {
                    download_bytes: voice.download_bytes(),
                    has_index: voice.has_index(),
                    voice,
                    installed_id,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()
    })
    .await
    .map_err(rvc_voices::internal)?
    .map(Json)
    .map_err(rvc_voices::internal)
}

#[derive(Deserialize)]
pub(super) struct InstallRequest {
    id: String,
}

pub(super) async fn install(
    State(state): State<AppState>,
    Json(request): Json<InstallRequest>,
) -> Result<(StatusCode, Json<Voice>), ApiFailure> {
    let voice = parse_manifest(BUNDLED)
        .map_err(rvc_voices::internal)?
        .into_iter()
        .find(|voice| voice.id == request.id)
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "No RVC catalog voice with that ID.".into(),
            )
        })?;
    let root = update_data_root(&state).join("rvc/voices");
    install_catalog_voice(root, &net::download_client(), voice).await
}

async fn install_catalog_voice(
    root: PathBuf,
    client: &reqwest::Client,
    voice: CatalogVoice,
) -> Result<(StatusCode, Json<Voice>), ApiFailure> {
    let local_id = voice.local_id().map_err(rvc_voices::internal)?;
    let installed = tokio::task::spawn_blocking({
        let root = root.clone();
        move || rvc_voices::read_voice(&root, &local_id)
    })
    .await
    .map_err(rvc_voices::internal)?;
    if let Some(installed) = installed {
        return Ok((StatusCode::OK, Json(installed)));
    }
    let stage = rvc_voices::new_stage(&root).await?;
    let transfer_error =
        |error: anyhow::Error| api_error(StatusCode::BAD_GATEWAY, error.to_string());
    if let Some(source) = &voice.archive {
        let archive_path = stage.path().join("voice.zip");
        download(client, &source.file, &archive_path)
            .await
            .map_err(transfer_error)?;
        let weights_hash = parse_hash(&source.weights_sha256).map_err(rvc_voices::internal)?;
        let index_hash = parse_hash(&source.index_sha256).map_err(rvc_voices::internal)?;
        let (stage, weights, index) = tokio::task::spawn_blocking(move || {
            let (weights, index) =
                archive::unpack(&archive_path, stage.path(), weights_hash, index_hash)?;
            Ok::<_, anyhow::Error>((stage, weights, index))
        })
        .await
        .map_err(rvc_voices::internal)?
        .map_err(transfer_error)?;
        return rvc_voices::install_staged(root, stage, weights, Some(index), Some(voice.origin()))
            .await;
    }
    let weights = voice
        .weights
        .as_ref()
        .context("Missing RVC weights.")
        .map_err(rvc_voices::internal)?;
    let model = download(client, weights, &stage.path().join("voice.pth"))
        .await
        .map_err(transfer_error)?;
    let index = match &voice.index {
        Some(file) => Some(
            download(client, file, &stage.path().join("retrieval.index"))
                .await
                .map_err(transfer_error)?,
        ),
        None => None,
    };
    rvc_voices::install_staged(root, stage, model, index, Some(voice.origin())).await
}

async fn download(
    client: &reqwest::Client,
    file: &RemoteFile,
    path: &Path,
) -> anyhow::Result<StagedFile> {
    let expected = file.hash()?;
    let mut response = client.get(&file.url).send().await?.error_for_status()?;
    let mut output = tokio::fs::File::create(path).await?;
    let mut size = 0u64;
    let mut hash = Sha256::new();
    while let Some(chunk) = response.chunk().await? {
        size += chunk.len() as u64;
        anyhow::ensure!(
            size <= file.size,
            "RVC download exceeds the pinned size of {}.",
            file.name()
        );
        hash.update(&chunk);
        output.write_all(&chunk).await?;
    }
    output.flush().await?;
    anyhow::ensure!(
        size == file.size,
        "RVC download size does not match {}.",
        file.name()
    );
    anyhow::ensure!(
        hash.finalize().as_slice() == expected.as_slice(),
        "RVC download SHA-256 does not match {}.",
        file.name()
    );
    Ok(StagedFile {
        name: file.name().into(),
        hash: expected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[test]
    fn bundled_manifest_has_pinned_indexed_and_unindexed_voices() {
        let entries = parse_manifest(BUNDLED).unwrap();
        let indexed = entries
            .iter()
            .find(|voice| voice.name == "guanguanV1")
            .unwrap();
        let plain = entries
            .iter()
            .find(|voice| voice.name == "youzhanv2-xi")
            .unwrap();
        assert!(indexed.index.is_some());
        assert!(plain.index.is_none());
        assert_ne!(indexed.local_id().unwrap(), plain.local_id().unwrap());
        let tapok = entries
            .iter()
            .find(|voice| voice.id == "rvc-50ea3d733073550ef6f7")
            .unwrap();
        let metadata = tapok.metadata.as_ref().unwrap();
        assert_eq!(metadata["vocal_style"], "high");
        assert_eq!(metadata["sample_rate_hz"], 40000);
        assert_eq!(metadata["source_labels"][0], "Radio Tapok");
        assert!(parse_manifest(r#"{"schema_version":2,"voices":[]}"#).is_err());
    }

    #[tokio::test]
    async fn verifies_pinned_bytes_before_installing_into_the_local_voice_store() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("archive/data.pkl", stored).unwrap();
        zip.write_all(b"enc_p.emb_phone.weight emb_g.weight dec.ups.0.weight")
            .unwrap();
        zip.start_file("archive/data/0", stored).unwrap();
        zip.write_all(b"tensor").unwrap();
        let checkpoint = zip.finish().unwrap().into_inner();
        let expected = Sha256::digest(&checkpoint)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let size = checkpoint.len() as u64;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/voice.pth", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let router = axum::Router::new().route(
                "/voice.pth",
                axum::routing::get(move || {
                    let checkpoint = checkpoint.clone();
                    async move { checkpoint }
                }),
            );
            axum::serve(listener, router).await.unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let root = root.path().to_path_buf();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let voice = |hash| CatalogVoice {
            id: "example".into(),
            name: "Example".into(),
            source_repo: "owner/voices".into(),
            source_url: "https://huggingface.co/owner/voices".into(),
            revision: "a".repeat(40),
            license: None,
            weights: Some(RemoteFile {
                url: endpoint.clone(),
                size,
                sha256: hash,
            }),
            index: None,
            archive: None,
            metadata: None,
        };
        let bad = install_catalog_voice(root.clone(), &client, voice("a".repeat(64))).await;
        assert!(matches!(bad, Err((StatusCode::BAD_GATEWAY, _))));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        let good = install_catalog_voice(root.clone(), &client, voice(expected)).await;
        let (status, Json(installed)) = match good {
            Ok(result) => result,
            Err((status, Json(error))) => panic!("{status}: {}", error.error),
        };
        assert_eq!(status, StatusCode::CREATED);
        assert!(rvc_voices::read_voice(&root, &installed.id).is_some());
        let provenance: serde_json::Value = serde_json::from_reader(
            std::fs::File::open(root.join(&installed.id).join("voice.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(provenance["origin"]["catalog_id"], "example");
        assert_eq!(provenance["origin"]["name"], "Example");
        server.abort();
    }

    #[tokio::test]
    async fn zip_install_is_atomic_on_ambiguous_corrupt_and_mismatched_archives() {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let build_zip = |files: &[(&str, &[u8])]| {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for (name, bytes) in files {
                writer.start_file(*name, options).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap().into_inner()
        };
        let model = build_zip(&[
            (
                "archive/data.pkl",
                b"enc_p.emb_phone.weight emb_g.weight dec.ups.0.weight",
            ),
            ("archive/data/0", b"tensor"),
        ]);
        let mut index = vec![0u8; 37 + 768 * 4];
        index[..4].copy_from_slice(b"IwFl");
        index[4..8].copy_from_slice(&768u32.to_le_bytes());
        index[8..16].copy_from_slice(&1u64.to_le_bytes());
        index[32] = 1;
        index[33] = 1;
        let digest = |bytes: &[u8]| {
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let ambiguous = build_zip(&[
            ("nested/voice.pth", &model),
            ("retrieval.index", &index),
            ("extra.index", &index),
        ]);
        let missing = build_zip(&[("nested/voice.pth", &model)]);
        let mut duplicate = build_zip(&[
            ("voice.pth", &model),
            ("extra.pth", &model),
            ("retrieval.index", &index),
        ]);
        let duplicate_names: Vec<_> = duplicate
            .windows(9)
            .enumerate()
            .filter_map(|(offset, name)| (name == b"extra.pth").then_some(offset))
            .collect();
        assert_eq!(duplicate_names.len(), 2);
        for offset in duplicate_names {
            duplicate[offset..offset + 9].copy_from_slice(b"voice.pth");
        }
        let valid = build_zip(&[
            ("nested/voice.pth", &model),
            ("other/retrieval.index", &index),
            ("README.txt", b"model card"),
        ]);
        let mut bad_crc = valid.clone();
        let index_payload = bad_crc.windows(4).position(|part| part == b"IwFl").unwrap();
        bad_crc[index_payload] ^= 1;
        let variants = [
            (ambiguous, digest(&index), Some("exactly one")),
            (duplicate, digest(&index), Some("duplicate entries")),
            (missing, digest(&index), Some("exactly one")),
            (b"this is not a ZIP".to_vec(), digest(&index), None),
            (bad_crc, digest(&index), None),
            (valid.clone(), "0".repeat(64), Some("SHA-256")),
            (valid, digest(&index), None),
        ];
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let mut router = axum::Router::new();
        for (position, (payload, _, _)) in variants.iter().enumerate() {
            let payload = payload.clone();
            router = router.route(
                &format!("/{position}.zip"),
                axum::routing::get(move || {
                    let payload = payload.clone();
                    async move { payload }
                }),
            );
        }
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for (position, (payload, index_sha256, expected_error)) in variants.iter().enumerate() {
            let make_voice = || CatalogVoice {
                id: "zip-fixture".into(),
                name: "ZIP fixture".into(),
                source_repo: "owner/voices".into(),
                source_url: "https://huggingface.co/owner/voices".into(),
                revision: "a".repeat(40),
                license: None,
                weights: None,
                index: None,
                archive: Some(RemoteArchive {
                    file: RemoteFile {
                        url: format!("{base_url}/{position}.zip"),
                        size: payload.len() as u64,
                        sha256: digest(payload),
                    },
                    weights_sha256: digest(&model),
                    index_sha256: index_sha256.clone(),
                }),
                metadata: None,
            };
            let id = make_voice().local_id().unwrap();
            let outcome =
                install_catalog_voice(root.path().to_path_buf(), &client, make_voice()).await;
            if position + 1 < variants.len() {
                let (status, Json(error)) = outcome.err().expect("invalid ZIP rejected");
                assert_eq!(status, StatusCode::BAD_GATEWAY);
                if let Some(message) = expected_error {
                    assert!(error.error.contains(message), "{}", error.error);
                }
                assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
            } else {
                let (status, Json(installed)) = outcome.ok().expect("install ZIP");
                assert_eq!(status, StatusCode::CREATED);
                assert_eq!(installed.id, id);
                let dir = root.path().join(&id);
                assert_eq!(std::fs::read(dir.join("voice.pth")).unwrap(), model);
                assert_eq!(std::fs::read(dir.join("retrieval.index")).unwrap(), index);
                assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
                assert!(rvc_voices::read_voice(root.path(), &id).is_some());

                server.abort();
                let (repeat, Json(previous)) =
                    install_catalog_voice(root.path().to_path_buf(), &client, make_voice())
                        .await
                        .ok()
                        .expect("repeat without a download source");
                assert_eq!(repeat, StatusCode::OK);
                assert_eq!(previous.id, id);
                assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
            }
        }
        server.abort();
    }
}
