use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use tokio::io::AsyncWriteExt;

use crate::{api_error, update_data_root, ApiError, AppState};

pub(super) const UPLOAD_LIMIT: usize = 2 * 1024 * 1024 * 1024;
const PICKLE_LIMIT: u64 = 4 * 1024 * 1024;
const MANIFEST: &str = "voice.json";

type ApiFailure = (StatusCode, Json<ApiError>);

#[derive(Deserialize, Serialize)]
pub(super) struct Voice {
    id: String,
    model_name: String,
    index_name: Option<String>,
    version: Option<u8>,
}

fn stored(root: &Path) -> io::Result<Vec<Voice>> {
    let folders = match fs::read_dir(root) {
        Ok(folders) => folders,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut voices = Vec::new();
    for folder in folders {
        let folder = folder?;
        let id = folder.file_name();
        if !folder.file_type()?.is_dir() || id.len() != 64 {
            continue;
        }
        let path = folder.path();
        let Ok(file) = File::open(path.join(MANIFEST)) else {
            continue;
        };
        let Ok(voice) = serde_json::from_reader::<_, Voice>(file) else {
            continue;
        };
        if id == voice.id.as_str()
            && path.join("voice.pth").is_file()
            && (voice.index_name.is_none() || path.join("retrieval.index").is_file())
        {
            voices.push(voice);
        }
    }
    voices.sort_by(|a, b| a.model_name.cmp(&b.model_name).then(a.id.cmp(&b.id)));
    Ok(voices)
}

pub(super) async fn list(State(state): State<AppState>) -> Result<Json<Vec<Voice>>, ApiFailure> {
    let root = update_data_root(&state).join("rvc/voices");
    tokio::task::spawn_blocking(move || stored(&root))
        .await
        .map_err(internal)?
        .map(Json)
        .map_err(internal)
}

fn internal(error: impl std::fmt::Display) -> ApiFailure {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn invalid(error: impl std::fmt::Display) -> ApiFailure {
    api_error(StatusCode::BAD_REQUEST, error.to_string())
}

fn file_name(
    field: &axum::extract::multipart::Field<'_>,
    allowed: &[&str],
) -> Result<String, ApiFailure> {
    let name = field
        .file_name()
        .and_then(|name| name.rsplit(['/', '\\']).next())
        .filter(|name| name.len() <= 255 && !name.chars().any(char::is_control))
        .ok_or_else(|| invalid("Select a local RVC file."))?;
    let ext = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    if !allowed
        .iter()
        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
    {
        return Err(invalid(format!(
            "Expected a {} file.",
            allowed.join(" or ")
        )));
    }
    Ok(name.to_owned())
}

pub(super) async fn import(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Voice>), ApiFailure> {
    let root = update_data_root(&state).join("rvc/voices");
    tokio::fs::create_dir_all(&root).await.map_err(internal)?;
    let stage = tempfile::Builder::new()
        .prefix(".import-")
        .tempdir_in(&root)
        .map_err(internal)?;
    let mut model = None;
    let mut index = None;
    while let Some(mut field) = multipart.next_field().await.map_err(invalid)? {
        let (part, name, target) = match field.name() {
            Some("model") if model.is_none() => {
                (&mut model, file_name(&field, &["pth", "pt"])?, "voice.pth")
            }
            Some("index") if index.is_none() => (
                &mut index,
                file_name(&field, &["index"])?,
                "retrieval.index",
            ),
            _ => {
                return Err(invalid(
                    "Send one RVC voice and at most one retrieval index.",
                ))
            }
        };
        let mut file = tokio::fs::File::create(stage.path().join(target))
            .await
            .map_err(internal)?;
        let mut hash = Sha256::new();
        while let Some(chunk) = field.chunk().await.map_err(invalid)? {
            hash.update(&chunk);
            file.write_all(&chunk).await.map_err(internal)?;
        }
        file.flush().await.map_err(internal)?;
        *part = Some((name, hash.finalize()));
    }
    let (model_name, model_hash) =
        model.ok_or_else(|| invalid("Select an RVC voice checkpoint."))?;
    tokio::task::spawn_blocking(move || {
        let version = checkpoint_version(&stage.path().join("voice.pth")).map_err(invalid)?;
        if index.is_some() {
            validate_index(&stage.path().join("retrieval.index"), version).map_err(invalid)?;
        }
        let mut content = Sha256::new();
        content.update(model_hash);
        let index_name = index.map(|(name, hash)| {
            content.update(hash);
            name
        });
        let id = format!("{:x}", content.finalize());
        let voice = Voice {
            id,
            model_name,
            index_name,
            version,
        };
        let target = root.join(&voice.id);
        serde_json::to_writer(
            File::create(stage.path().join(MANIFEST)).map_err(internal)?,
            &voice,
        )
        .map_err(internal)?;
        match fs::rename(stage.path(), &target) {
            Ok(()) => Ok((StatusCode::CREATED, Json(voice))),
            Err(rename_error) => {
                let Ok(file) = File::open(target.join(MANIFEST)) else {
                    return Err(internal(rename_error));
                };
                let existing = serde_json::from_reader(file).map_err(internal)?;
                Ok((StatusCode::OK, Json(existing)))
            }
        }
    })
    .await
    .map_err(internal)?
}

// Inspect a bounded archive member as bytes. The inference engine performs
// full tensor validation with its native checkpoint reader.
fn checkpoint_version(path: &Path) -> anyhow::Result<Option<u8>> {
    let mut archive = zip::ZipArchive::new(File::open(path)?)?;
    let mut metadata = None;
    let mut has_storage = false;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        anyhow::ensure!(
            entry.compression() == zip::CompressionMethod::Stored,
            "Use an uncompressed PyTorch RVC checkpoint."
        );
        if entry.name().ends_with("/data.pkl") || entry.name() == "data.pkl" {
            anyhow::ensure!(
                metadata.is_none() && entry.size() <= PICKLE_LIMIT,
                "Invalid RVC checkpoint metadata."
            );
            let mut bytes = Vec::new();
            entry.take(PICKLE_LIMIT + 1).read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() as u64 <= PICKLE_LIMIT,
                "Invalid RVC checkpoint metadata."
            );
            metadata = Some(bytes);
        } else if (entry.name().contains("/data/") || entry.name().starts_with("data/"))
            && entry.size() > 0
        {
            has_storage = true;
        }
    }
    let data =
        metadata.ok_or_else(|| anyhow::anyhow!("RVC checkpoint has no PyTorch metadata."))?;
    let voice_keys: &[&[u8]] = &[
        b"enc_p.emb_phone.weight",
        b"emb_g.weight",
        b"dec.ups.0.weight",
    ];
    anyhow::ensure!(
        has_storage
            && voice_keys
                .iter()
                .all(|key| data.windows(key.len()).any(|part| part == *key)),
        "Select an exported RVC inference voice, with its encoder, decoder and speaker weights."
    );

    for key in [
        b"X\x07\x00\x00\x00version".as_slice(),
        b"\x8c\x07version".as_slice(),
    ] {
        if let Some(position) = data.windows(key.len()).position(|part| part == key) {
            let value = &data[position + key.len()..];
            let memo_size = match value.first() {
                Some(b'q') => 2,
                Some(b'r') => 5,
                Some(0x94) => 1,
                _ => 0,
            };
            let value = value.get(memo_size..).unwrap_or_default();
            for (name, version) in [(b'1', 1), (b'2', 2)] {
                if value.starts_with(&[b'X', 2, 0, 0, 0, b'v', name])
                    || value.starts_with(&[0x8c, 2, b'v', name])
                {
                    return Ok(Some(version));
                }
            }
        }
    }
    Ok(None)
}

fn validate_index(path: &Path, version: Option<u8>) -> anyhow::Result<()> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 37];
    file.read_exact(&mut header)?;
    let dimension = u32::from_le_bytes(header[4..8].try_into()?);
    let count = u64::from_le_bytes(header[8..16].try_into()?);
    let min_size = count
        .checked_mul(u64::from(dimension) * 4)
        .and_then(|size| size.checked_add(37));
    anyhow::ensure!(
        &header[..4] == b"IwFl"
            && matches!(dimension, 256 | 768)
            && count > 0
            && header[32] != 0
            && header[33..37] == [1, 0, 0, 0]
            && min_size.is_some_and(|size| file.metadata().is_ok_and(|meta| meta.len() >= size)),
        "Select a filled FAISS IndexIVFFlat retrieval index."
    );
    anyhow::ensure!(
        !matches!((version, dimension), (Some(1), 768) | (Some(2), 256)),
        "The retrieval index dimension does not match the RVC voice version."
    );
    Ok(())
}
