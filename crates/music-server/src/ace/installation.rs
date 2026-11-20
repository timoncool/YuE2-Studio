// The four weights of each set in ACE-Step Studio 318cc0ad (model_manager.rs).
const DIT_TURBO_Q4: Weight = Weight {
    name: "acestep-v15-turbo-Q4_K_M.gguf",
    bytes: 1_445_710_272,
};
const LM_0_6B_Q8: Weight = Weight {
    name: "acestep-5Hz-lm-0.6B-Q8_0.gguf",
    bytes: 709_846_656,
};
const TE_QWEN3_0_6B_Q8: Weight = Weight {
    name: "Qwen3-Embedding-0.6B-Q8_0.gguf",
    bytes: 784_144_960,
};
const VAE_STANDARD_BF16: Weight = Weight {
    name: "vae-BF16.gguf",
    bytes: 337_420_928,
};
const DIT_XL_TURBO_Q4: Weight = Weight {
    name: "acestep-v15-xl-turbo-Q4_K_M.gguf",
    bytes: 2_989_922_688,
};
const LM_1_7B_Q8: Weight = Weight {
    name: "acestep-5Hz-lm-1.7B-Q8_0.gguf",
    bytes: 1_975_837_568,
};
const DIT_XL_TURBO_Q6: Weight = Weight {
    name: "acestep-v15-xl-turbo-Q6_K.gguf",
    bytes: 4_098_812_992,
};
const DIT_XL_TURBO_MXFP4: Weight = Weight {
    name: "acestep-v15-xl-turbo-MXFP4.gguf",
    bytes: 2_660_726_464,
};
const DIT_XL_TURBO_Q8: Weight = Weight {
    name: "acestep-v15-xl-turbo-Q8_0.gguf",
    bytes: 5_305_828_736,
};
const LM_4B_Q8: Weight = Weight {
    name: "acestep-5Hz-lm-4B-Q8_0.gguf",
    bytes: 4_457_323_648,
};
const DIT_XL_TURBO_BF16: Weight = Weight {
    name: "acestep-v15-xl-turbo-BF16.gguf",
    bytes: 9_978_530_816,
};
const LM_4B_BF16: Weight = Weight {
    name: "acestep-5Hz-lm-4B-BF16.gguf",
    bytes: 8_384_454_592,
};
const TE_QWEN3_0_6B_BF16: Weight = Weight {
    name: "Qwen3-Embedding-0.6B-BF16.gguf",
    bytes: 1_196_890_592,
};
const PROFILES: &[Profile] = &[
    Profile { id: "minimal", label: "Minimal - 2B turbo Q4_K_M, LM 0.6B (4 GB cards)", weights: [DIT_TURBO_Q4, LM_0_6B_Q8, TE_QWEN3_0_6B_Q8, VAE_STANDARD_BF16] },
    Profile { id: "recommended-light", label: "Light - XL turbo Q4_K_M, LM 1.7B (8 GB cards)", weights: [DIT_XL_TURBO_Q4, LM_1_7B_Q8, TE_QWEN3_0_6B_Q8, VAE_STANDARD_BF16] },
    Profile { id: "balanced", label: "Balanced - XL turbo Q6_K, LM 1.7B (10 GB cards)", weights: [DIT_XL_TURBO_Q6, LM_1_7B_Q8, TE_QWEN3_0_6B_Q8, VAE_STANDARD_BF16] },
    Profile { id: "balanced-mxfp4", label: "Balanced MXFP4 - XL turbo MXFP4, LM 1.7B (8 GB cards). MXFP4: half the DiT memory of Q8_0; an RTX 50 computes it on its tensor cores directly", weights: [DIT_XL_TURBO_MXFP4, LM_1_7B_Q8, TE_QWEN3_0_6B_Q8, VAE_STANDARD_BF16] },
    Profile { id: "quality-q8", label: "Quality - XL turbo Q8_0, LM 4B Q8_0 (13 GB cards)", weights: [DIT_XL_TURBO_Q8, LM_4B_Q8, TE_QWEN3_0_6B_Q8, VAE_STANDARD_BF16] },
    Profile { id: "quality-mxfp4", label: "Quality MXFP4 - XL turbo MXFP4, LM 4B Q8_0 (13 GB cards). MXFP4: half the DiT memory of Q8_0; an RTX 50 computes it on its tensor cores directly", weights: [DIT_XL_TURBO_MXFP4, LM_4B_Q8, TE_QWEN3_0_6B_Q8, VAE_STANDARD_BF16] },
    Profile { id: "native", label: "Full native - XL turbo BF16, LM 4B BF16 (24 GB cards)", weights: [DIT_XL_TURBO_BF16, LM_4B_BF16, TE_QWEN3_0_6B_BF16, VAE_STANDARD_BF16] },
];

use super::Launch;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

pub const ENGINE_ID: &str = "acestep-cpp";

#[derive(Clone, Copy)]
struct Weight {
    name: &'static str,
    bytes: u64,
}

struct Profile {
    id: &'static str,
    label: &'static str,
    weights: [Weight; 4],
}

#[derive(Debug, Serialize)]
pub struct InstalledProfile {
    pub id: &'static str,
    pub label: &'static str,
    pub total_bytes: u64,
    pub ready: bool,
    pub missing: Vec<&'static str>,
}

pub struct Installation {
    executable: Option<PathBuf>,
    models_root: PathBuf,
    data_root: PathBuf,
}

pub fn installation(data_root: &Path) -> Installation {
    let data_root = data_root.to_owned();
    let executable = match std::env::var_os("ACE_ENGINE_BIN") {
        Some(path) => {
            let path = PathBuf::from(path);
            usable_executable(&path).then_some(path)
        }
        None => {
            let managed = data_root.join("engines").join(ENGINE_ID);
            let packaged = crate::engine_bundle_root().with_file_name(ENGINE_ID);
            std::env::var_os("ACE_ENGINE_ROOT")
                .map(PathBuf::from)
                .map_or_else(
                    || {
                        [managed, packaged]
                            .iter()
                            .find_map(|root| find_executable(root))
                    },
                    |root| find_executable(&root),
                )
        }
    };
    let models_root = std::env::var_os("ACE_MODELS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_root.join("models").join(ENGINE_ID));
    Installation {
        executable,
        models_root,
        data_root,
    }
}

fn usable_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn find_executable(root: &Path) -> Option<PathBuf> {
    let binary = if cfg!(windows) {
        "ace-server.exe"
    } else {
        "ace-server"
    };
    [
        root.to_owned(),
        root.join("bin"),
        root.join("build").join("Release"),
        root.join("build"),
    ]
    .into_iter()
    .map(|dir| dir.join(binary))
    .find(|path| usable_executable(path))
}

impl Installation {
    pub fn engine_installed(&self) -> bool {
        self.executable.as_deref().is_some_and(usable_executable)
    }

    pub fn profiles(&self) -> Vec<InstalledProfile> {
        PROFILES
            .iter()
            .map(|profile| profile.status(&self.models_root))
            .collect()
    }

    pub fn launch(&self, profile_id: &str) -> Result<Launch> {
        let profile = PROFILES
            .iter()
            .find(|profile| profile.id == profile_id)
            .with_context(|| format!("unknown ACE-Step profile {profile_id}"))?;
        let executable = self
            .executable
            .as_ref()
            .filter(|path| usable_executable(path))
            .with_context(|| {
                format!(
                    "ACE-Step runtime is missing; expected ace-server under {} or ACE_ENGINE_BIN",
                    self.data_root.join("engines").join(ENGINE_ID).display()
                )
            })?;
        let models_root = profile.prepare_directory(&self.models_root)?;
        let adapters = self.data_root.join("adapters").join(ENGINE_ID);
        Ok(Launch {
            executable: executable.clone(),
            models_root,
            adapters_root: adapters.is_dir().then_some(adapters),
            device: std::env::var("ACE_DEVICE")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            cuda_backend: std::env::var_os("ACE_CUDA_BACKEND").map(PathBuf::from),
            log_path: self.data_root.join("logs").join("ace-server.log"),
        })
    }
}

impl Profile {
    fn status(&self, root: &Path) -> InstalledProfile {
        let missing: Vec<_> = self
            .weights
            .iter()
            .filter_map(|weight| {
                weight
                    .metadata(&root.join(weight.name))
                    .is_none()
                    .then_some(weight.name)
            })
            .collect();
        InstalledProfile {
            id: self.id,
            label: self.label,
            total_bytes: self.weights.iter().map(|weight| weight.bytes).sum(),
            ready: missing.is_empty(),
            missing,
        }
    }

    /// The ACE server picks the first text encoder found in its model directory.
    /// Give it one file per role. Keep the view beside the source weights so
    /// opening a profile adds directory entries without duplicating weights.
    fn prepare_directory(&self, root: &Path) -> Result<PathBuf> {
        let status = self.status(root);
        if !status.ready {
            bail!(
                "ACE-Step profile {} needs these exact files in {}: {}",
                self.id,
                root.display(),
                status.missing.join(", ")
            );
        }
        let root = root
            .canonicalize()
            .context("locate the ACE-Step models directory")?;
        let mut revision = DefaultHasher::new();
        let mut sources = Vec::with_capacity(self.weights.len());
        for weight in self.weights {
            let path = root
                .join(weight.name)
                .canonicalize()
                .with_context(|| format!("locate {}", weight.name))?;
            let metadata = weight.metadata(&path).with_context(|| {
                format!(
                    "{} must contain exactly {} bytes",
                    path.display(),
                    weight.bytes
                )
            })?;
            path.hash(&mut revision);
            metadata.modified()?.hash(&mut revision);
            metadata.len().hash(&mut revision);
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                metadata.dev().hash(&mut revision);
                metadata.ino().hash(&mut revision);
            }
            sources.push((weight, path, metadata.modified()?));
        }

        let directory =
            root.join(".studio-profiles")
                .join(format!("{}-{:016x}", self.id, revision.finish()));
        fs::create_dir_all(&directory)
            .with_context(|| format!("create the ACE-Step profile view {}", directory.display()))?;
        for (weight, source, modified) in sources {
            let target = directory.join(weight.name);
            if let Err(error) = fs::hard_link(&source, &target) {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error).with_context(|| format!("link {} into {}. ACE-Step profile files must be on the model directory's volume, which must support hard links and be writable; ACE_MODELS_ROOT can point to the actual weight directory", source.display(), directory.display()));
                }
            }
            if weight
                .metadata(&target)
                .and_then(|value| value.modified().ok())
                != Some(modified)
            {
                bail!(
                    "ACE-Step profile view has an unexpected file at {}",
                    target.display()
                );
            }
        }
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            if !self
                .weights
                .iter()
                .any(|weight| entry.file_name() == weight.name)
            {
                bail!(
                    "ACE-Step profile view contains an unexpected file: {}",
                    entry.path().display()
                );
            }
        }
        Ok(directory)
    }
}

impl Weight {
    fn metadata(self, path: &Path) -> Option<fs::Metadata> {
        path.metadata()
            .ok()
            .filter(|metadata| metadata.is_file() && metadata.len() == self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_uses_the_release_profile_ids_and_exact_totals() {
        assert_eq!(PROFILES.len(), 7);
        let root = Path::new("this-path-does-not-need-to-exist");
        let minimal = PROFILES[0].status(root);
        assert_eq!(minimal.id, "minimal");
        assert_eq!(minimal.total_bytes, 3_277_122_816);
        assert_eq!(minimal.missing.len(), 4);
        assert!(!minimal.ready);
        let native = PROFILES[6].status(root);
        assert_eq!(native.id, "native");
        assert_eq!(native.total_bytes, 19_897_296_928);
    }

    #[test]
    fn profile_view_uses_all_four_exact_sizes_and_keeps_previous_links() {
        let root = std::env::temp_dir().join(format!("ace-profile-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let profile = Profile {
            id: "test",
            label: "Test",
            weights: [
                Weight {
                    name: "dit.gguf",
                    bytes: 3,
                },
                Weight {
                    name: "lm.gguf",
                    bytes: 3,
                },
                Weight {
                    name: "encoder.gguf",
                    bytes: 3,
                },
                Weight {
                    name: "vae.gguf",
                    bytes: 3,
                },
            ],
        };
        for name in ["dit.gguf", "lm.gguf", "vae.gguf"] {
            fs::write(root.join(name), b"old").unwrap();
        }
        fs::write(root.join("encoder.gguf"), b"").unwrap();
        assert_eq!(profile.status(&root).missing, vec!["encoder.gguf"]);
        assert!(profile.prepare_directory(&root).is_err());
        fs::write(root.join("encoder.gguf"), b"old").unwrap();
        fs::write(root.join("other.gguf"), b"other quantization").unwrap();
        let first = profile.prepare_directory(&root).unwrap();
        assert_eq!(fs::read_dir(&first).unwrap().count(), 4);
        assert_eq!(profile.prepare_directory(&root).unwrap(), first);
        let current = root.join("encoder.gguf");
        fs::remove_file(&current).unwrap();
        assert_eq!(fs::read(first.join("encoder.gguf")).unwrap(), b"old");
        fs::write(&current, b"new").unwrap();
        let different_time = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(current)
            .unwrap()
            .set_modified(different_time)
            .unwrap();
        let second = profile.prepare_directory(&root).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(first.join("encoder.gguf")).unwrap(), b"old");
        assert_eq!(fs::read(second.join("encoder.gguf")).unwrap(), b"new");
        fs::remove_dir_all(&root).unwrap();
    }
}
