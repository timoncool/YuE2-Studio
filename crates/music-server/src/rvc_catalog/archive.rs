use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

use crate::rvc_voices::{StagedFile, UPLOAD_LIMIT};

const MAX_ENTRIES: usize = 1024;

pub(super) fn unpack(
    archive_path: &Path,
    stage: &Path,
    weights_hash: [u8; 32],
    index_hash: [u8; 32],
) -> anyhow::Result<(StagedFile, StagedFile)> {
    let mut archive = zip::ZipArchive::new(File::open(archive_path)?)?;
    anyhow::ensure!(
        archive.len() <= MAX_ENTRIES,
        "RVC ZIP has too many entries."
    );
    let mut weights = None;
    let mut index = None;
    let mut size = 0u64;
    for position in 0..archive.len() {
        let entry = archive.by_index(position)?;
        let name = entry.name()?;
        let normalized = name.replace('\\', "/");
        anyhow::ensure!(
            entry.enclosed_name().is_some()
                && !normalized.starts_with('/')
                && !normalized.contains(':')
                && !normalized.split('/').any(|part| part == "..")
                && !name.chars().any(char::is_control)
                && !entry.is_symlink(),
            "RVC ZIP contains an unsafe entry."
        );
        if !entry.is_file() {
            continue;
        }
        size = size
            .checked_add(entry.size())
            .filter(|size| *size <= UPLOAD_LIMIT as u64)
            .ok_or_else(|| anyhow::anyhow!("RVC ZIP exceeds the unpacked voice size limit."))?;
        let basename = normalized.rsplit('/').next().unwrap_or_default();
        let lowercase = basename.to_ascii_lowercase();
        let slot = if lowercase.ends_with(".pth") || lowercase.ends_with(".pt") {
            &mut weights
        } else if lowercase.ends_with(".index") {
            &mut index
        } else {
            continue;
        };
        anyhow::ensure!(
            slot.is_none(),
            "RVC ZIP must contain exactly one checkpoint and one retrieval index."
        );
        anyhow::ensure!(
            entry.size() > 0 && basename.len() <= 255,
            "RVC ZIP contains an empty or invalid voice entry."
        );
        *slot = Some((position, basename.to_owned()));
    }
    let (Some((weights_position, weights_name)), Some((index_position, index_name))) =
        (weights, index)
    else {
        anyhow::bail!("RVC ZIP must contain exactly one checkpoint and one retrieval index.");
    };
    write_verified(
        archive.by_index(weights_position)?,
        &stage.join("voice.pth"),
        weights_hash,
        &weights_name,
    )?;
    write_verified(
        archive.by_index(index_position)?,
        &stage.join("retrieval.index"),
        index_hash,
        &index_name,
    )?;
    drop(archive);
    fs::remove_file(archive_path)?;
    Ok((
        StagedFile {
            name: weights_name,
            hash: weights_hash,
        },
        StagedFile {
            name: index_name,
            hash: index_hash,
        },
    ))
}

fn write_verified(
    mut input: zip::read::ZipFile<'_, File>,
    path: &Path,
    expected: [u8; 32],
    display_name: &str,
) -> anyhow::Result<()> {
    let mut output = File::create_new(path)?;
    let size = input.size();
    let mut copied = 0u64;
    let mut hash = Sha256::new();
    let mut chunk = [0u8; 65536];
    loop {
        let count = input.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        copied += count as u64;
        if copied > size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "RVC ZIP entry exceeds its declared size.",
            )
            .into());
        }
        hash.update(&chunk[..count]);
        output.write_all(&chunk[..count])?;
    }
    anyhow::ensure!(
        copied == size && hash.finalize().as_slice() == expected.as_slice(),
        "RVC ZIP entry size or SHA-256 does not match {display_name}."
    );
    output.flush()?;
    Ok(())
}
