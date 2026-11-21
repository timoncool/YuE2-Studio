use axum::extract::{Multipart, Path as RoutePath, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use crate::{api_error, update_data_root, ApiError, AppState};

pub(super) const UPLOAD_LIMIT: usize = 2 * 1024 * 1024 * 1024;
const PICKLE_LIMIT: u64 = 4 * 1024 * 1024;
const MANIFEST: &str = "voice.json";

pub(super) type ApiFailure = (StatusCode, Json<ApiError>);

#[derive(Deserialize, Serialize)]
pub(super) struct Voice {
    pub(super) id: String,
    model_name: String,
    index_name: Option<String>,
    version: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<VoiceOrigin>,
}

impl Voice {
    pub(super) fn title(&self) -> &str {
        self.origin.as_ref().map_or(&self.model_name, |origin| &origin.name)
    }

    pub(super) fn has_index(&self) -> bool {
        self.index_name.is_some()
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct VoiceOrigin {
    pub(super) catalog_id: String,
    pub(super) name: String,
    pub(super) source_repo: String,
    pub(super) revision: String,
    pub(super) source_url: String,
    pub(super) license: Option<String>,
}

pub(super) struct StagedFile {
    pub(super) name: String,
    pub(super) hash: [u8; 32],
}

pub(super) fn voice_id(model: &[u8; 32], index: Option<&[u8; 32]>) -> String {
    let mut content = Sha256::new();
    content.update(model);
    if let Some(index) = index {
        content.update(index);
    }
    content
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn read_voice(root: &Path, id: &str) -> Option<Voice> {
    let path = root.join(id);
    let file = File::open(path.join(MANIFEST)).ok()?;
    let voice = serde_json::from_reader::<_, Voice>(file).ok()?;
    (voice.id == id
        && path.join("voice.pth").is_file()
        && (voice.index_name.is_none() || path.join("retrieval.index").is_file()))
    .then_some(voice)
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
        let name = folder.file_name();
        let Some(id) = name.to_str() else { continue };
        if folder.file_type()?.is_dir() && id.len() == 64 {
            voices.extend(read_voice(root, id));
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

#[derive(Serialize)]
pub(super) struct RemoveVoice {
    id: String,
    removed: bool,
}

pub(super) async fn remove(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<RemoveVoice>, ApiFailure> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid("Choose an installed RVC voice ID."));
    }
    let root = update_data_root(&state).join("rvc/voices");
    tokio::task::spawn_blocking(move || {
        let removed = remove_installed(&root, &id)?;
        Ok(Json(RemoveVoice { id, removed }))
    })
    .await
    .map_err(internal)?
}

fn remove_installed(root: &Path, id: &str) -> Result<bool, ApiFailure> {
    let target = root.join(id);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(api_error(
                StatusCode::CONFLICT,
                "The installed RVC voice folder is invalid.".into(),
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(internal(error)),
    }
    if read_voice(root, id).is_none() {
        return if target.try_exists().map_err(internal)? {
            Err(api_error(
                StatusCode::CONFLICT,
                "The installed RVC voice folder is incomplete.".into(),
            ))
        } else {
            Ok(false)
        };
    }
    let retired = tempfile::Builder::new()
        .prefix(".removed-")
        .tempdir_in(root)
        .map_err(internal)?;
    // The manifest, weights and index leave the installed store together.
    // A concurrent reinstall publishes to the old path, outside this temporary folder.
    match fs::rename(&target, retired.path().join("voice")) {
        Ok(()) => {
            if let Err(error) = retired.close() {
                eprintln!("[RVC] Could not clean up removed voice {id}: {error}");
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(internal(error)),
    }
}

pub(super) fn internal(error: impl std::fmt::Display) -> ApiFailure {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

pub(super) fn invalid(error: impl std::fmt::Display) -> ApiFailure {
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
    let stage = new_stage(&root).await?;
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
        *part = Some(StagedFile {
            name,
            hash: hash
                .finalize()
                .as_slice()
                .try_into()
                .expect("32-byte SHA-256 digest"),
        });
    }
    let model = model.ok_or_else(|| invalid("Select an RVC voice checkpoint."))?;
    install_staged(root, stage, model, index, None).await
}

pub(super) async fn new_stage(root: &Path) -> Result<tempfile::TempDir, ApiFailure> {
    tokio::fs::create_dir_all(root).await.map_err(internal)?;
    tempfile::Builder::new()
        .prefix(".import-")
        .tempdir_in(root)
        .map_err(internal)
}

pub(super) async fn install_staged(
    root: PathBuf,
    stage: tempfile::TempDir,
    model: StagedFile,
    index: Option<StagedFile>,
    origin: Option<VoiceOrigin>,
) -> Result<(StatusCode, Json<Voice>), ApiFailure> {
    tokio::task::spawn_blocking(move || {
        let version = checkpoint_version(&stage.path().join("voice.pth")).map_err(invalid)?;
        if index.is_some() {
            validate_index(&stage.path().join("retrieval.index"), version).map_err(invalid)?;
        }
        let id = voice_id(&model.hash, index.as_ref().map(|file| &file.hash));
        let voice = Voice {
            id,
            model_name: model.name,
            index_name: index.map(|file| file.name),
            version,
            origin,
        };
        let target = root.join(&voice.id);
        serde_json::to_writer(
            File::create(stage.path().join(MANIFEST)).map_err(internal)?,
            &voice,
        )
        .map_err(internal)?;
        match fs::rename(stage.path(), target) {
            Ok(()) => Ok((StatusCode::CREATED, Json(voice))),
            Err(rename_error) => {
                let existing =
                    read_voice(&root, &voice.id).ok_or_else(|| internal(rename_error))?;
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
        let name = entry.name()?;
        if name.ends_with("/data.pkl") || name == "data.pkl" {
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
        } else if (name.contains("/data/") || name.starts_with("data/")) && entry.size() > 0 {
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

#[cfg(test)]
mod removal_tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn removal_preserves_songs_and_the_pair_can_be_installed_again() {
        let profile = tempfile::tempdir().unwrap();
        let root = profile.path().join("rvc/voices");
        let audio = profile.path().join("finished.wav");
        fs::write(&audio, b"rendered audio").unwrap();
        let weights_data = {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("archive/data.pkl", options).unwrap();
            writer
                .write_all(b"enc_p.emb_phone.weight emb_g.weight dec.ups.0.weight")
                .unwrap();
            writer.start_file("archive/data/0", options).unwrap();
            writer.write_all(b"tensor").unwrap();
            writer.finish().unwrap().into_inner()
        };
        let mut index_data = vec![0; 37 + 256 * 4];
        index_data[..4].copy_from_slice(b"IwFl");
        index_data[4..8].copy_from_slice(&256u32.to_le_bytes());
        index_data[8..16].copy_from_slice(&1u64.to_le_bytes());
        index_data[32] = 1;
        index_data[33] = 1;
        let first_id = voice_id(
            &Sha256::digest(&weights_data).into(),
            Some(&Sha256::digest(&index_data).into()),
        );
        for _ in 0..2 {
            let stage = new_stage(&root).await.ok().unwrap();
            fs::write(stage.path().join("voice.pth"), &weights_data).unwrap();
            fs::write(stage.path().join("retrieval.index"), &index_data).unwrap();
            let weights = StagedFile {
                name: "voice.pth".into(),
                hash: Sha256::digest(&weights_data).into(),
            };
            let index = StagedFile {
                name: "voice.index".into(),
                hash: Sha256::digest(&index_data).into(),
            };
            let (status, Json(installed)) =
                install_staged(root.clone(), stage, weights, Some(index), None)
                    .await
                    .ok()
                    .unwrap();
            assert_eq!(status, StatusCode::CREATED);
            assert_eq!(installed.id, first_id);
            assert_eq!(stored(&root).unwrap().len(), 1);
            assert!(remove_installed(&root, &first_id).ok().unwrap());
            assert!(!remove_installed(&root, &first_id).ok().unwrap());
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
            assert_eq!(fs::read(&audio).unwrap(), b"rendered audio");
        }
    }

    #[test]
    fn invalid_voice_folder_is_retained_on_error() {
        let root = tempfile::tempdir().unwrap();
        let id = "1".repeat(64);
        let folder = root.path().join(&id);
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("voice.pth"), b"checkpoint").unwrap();
        fs::write(folder.join("retrieval.index"), b"index").unwrap();
        let (status, _) = remove_installed(root.path(), &id).err().unwrap();
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(fs::read(folder.join("voice.pth")).unwrap(), b"checkpoint");
        assert_eq!(fs::read(folder.join("retrieval.index")).unwrap(), b"index");
    }
}
