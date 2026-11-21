use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};

/// Extract a pinned runtime into a new staging directory. CUDA 12 and CUDA 13
/// have the same library basename, so every path is kept relative to the root.
pub(super) fn unpack(archive: &Path, stage: &Path) -> Result<Vec<PathBuf>> {
    let mut magic = [0u8; 2];
    File::open(archive)?.read_exact(&mut magic)?;
    let mut written = Vec::new();
    if magic == [0x1f, 0x8b] {
        let mut links = Vec::new();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(File::open(archive)?));
        for entry in tar.entries()? {
            let mut entry = entry?;
            let kind = entry.header().entry_type();
            if kind.is_dir() {
                continue;
            }
            let path = relative_path(&entry.path()?)?;
            let destination = stage.join(path);
            if kind.is_symlink() {
                let target = entry.link_name()?.context("runtime link has no target")?;
                let target = relative_path(&target)?;
                ensure!(
                    target.components().count() == 1,
                    "runtime link must name a sibling file"
                );
                let sibling = destination.with_file_name(target);
                links.push((destination, sibling));
            } else {
                ensure!(
                    kind.is_file(),
                    "runtime archive member is neither a file nor a library link"
                );
                let mode = entry.header().mode().ok();
                write_file(&destination, &mut entry, mode)?;
                written.push(destination);
            }
        }
        while !links.is_empty() {
            let (ready, pending): (Vec<_>, Vec<_>) = links
                .into_iter()
                .partition(|(_, target)| written.contains(target));
            ensure!(
                !ready.is_empty(),
                "runtime archive contains an unresolved library link"
            );
            for (destination, target) in ready {
                fs::hard_link(target, &destination)?;
                written.push(destination);
            }
            links = pending;
        }
    } else {
        let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name()?;
            let destination = stage.join(relative_path(Path::new(&*name))?);
            let mode = entry.unix_mode();
            ensure!(
                !mode.is_some_and(|mode| mode & 0o170000 == 0o120000),
                "unexpected link in runtime ZIP"
            );
            write_file(&destination, &mut entry, mode)?;
            written.push(destination);
        }
    }
    Ok(written)
}

fn relative_path(path: &Path) -> Result<PathBuf> {
    let raw = path.to_str().context("runtime path is not UTF-8")?;
    ensure!(
        !raw.contains(['\\', ':']) && !raw.split('/').any(|part| part == ".."),
        "unsafe runtime archive path {raw}"
    );
    let mut relative = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::Normal(part) => {
                let name = part.to_str().context("invalid runtime archive name")?;
                ensure!(
                    super::engine_catalogue::portable_name(name),
                    "unsafe runtime member {name}"
                );
                relative.push(part);
            }
            _ => anyhow::bail!("unsafe runtime archive path {raw}"),
        }
    }
    ensure!(
        !relative.as_os_str().is_empty(),
        "empty runtime archive path"
    );
    Ok(relative)
}

fn write_file(path: &Path, source: &mut impl Read, mode: Option<u32>) -> Result<()> {
    fs::create_dir_all(path.parent().context("runtime member has no parent")?)?;
    let mut output = File::create_new(path)?;
    io::copy(source, &mut output)?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        output.set_permissions(fs::Permissions::from_mode(mode & 0o777))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn preserves_both_cuda_libraries() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.zip");
        let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
        for (name, bytes) in [
            ("cuda12/ggml-cuda.dll", b"twelve".as_slice()),
            ("cuda13/ggml-cuda.dll", b"thirteen"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let output = temp.path().join("stage");
        assert_eq!(unpack(&archive, &output).unwrap().len(), 2);
        assert_eq!(
            fs::read(output.join("cuda12/ggml-cuda.dll")).unwrap(),
            b"twelve"
        );
        assert_eq!(
            fs::read(output.join("cuda13/ggml-cuda.dll")).unwrap(),
            b"thirteen"
        );
    }

    #[test]
    fn rejects_escaping_and_windows_device_paths() {
        for name in [
            "../outside.dll",
            "/absolute.dll",
            "cuda13/../escaped.dll",
            "C:/outside.dll",
            "dir\\file",
            "cOn.dll",
            "file.dll:other",
        ] {
            assert!(relative_path(Path::new(name)).is_err(), "{name}");
        }
    }
}
