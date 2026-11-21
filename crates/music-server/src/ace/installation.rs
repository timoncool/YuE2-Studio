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
    pub native_adapters: Vec<crate::adapters::Installed>,
}

pub struct Installation {
    executable: Option<PathBuf>,
    model_roots: Vec<PathBuf>,
    legacy_adapters: Vec<(PathBuf, PathBuf)>,
    data_root: PathBuf,
}

pub fn installation(data_root: &Path) -> Installation {
    let neighbors = studio_hub_client::neighbors::others(data_root);
    installation_from(
        data_root,
        crate::engine_bundle_root().with_file_name(ENGINE_ID),
        std::env::var_os("ACE_ENGINE_BIN").map(PathBuf::from),
        std::env::var_os("ACE_ENGINE_ROOT").map(PathBuf::from),
        std::env::var_os("ACE_MODELS_ROOT").map(PathBuf::from),
        &neighbors,
    )
}

fn installation_from(
    data_root: &Path,
    packaged: PathBuf,
    engine_bin: Option<PathBuf>,
    engine_root: Option<PathBuf>,
    models_root: Option<PathBuf>,
    neighbors: &[studio_hub_client::neighbors::Neighbor],
) -> Installation {
    let registered = neighbors
        .iter()
        .filter(|entry| entry.app == "ace")
        .map(|entry| entry.data_dir.as_path());
    let managed = data_root.join("engines").join(ENGINE_ID);
    let executable = match (engine_bin, engine_root) {
        (Some(path), _) => usable_executable(&path).then_some(path),
        (None, Some(root)) => find_executable(&root),
        (None, None) => crate::native_music_runtime::installed(data_root, ENGINE_ID)
            .or_else(|| [managed, packaged].iter().find_map(|root| find_executable(root)))
            .or_else(|| {
                registered
                    .clone()
                    .filter_map(Path::parent)
                    .find_map(|install| {
                        // The released Studio stores ace-server under resources/engine
                        // beside its portable data folder.
                        find_executable(&install.join("resources").join("engine"))
                    })
            }),
    };
    let legacy_adapters = registered
        .clone()
        .map(|root| (root.join("models").join(ENGINE_ID), root.join("adapters")))
        .collect();
    let model_roots = match models_root {
        Some(root) => vec![root],
        None => std::iter::once(data_root)
            .chain(registered)
            .map(|root| root.join("models").join(ENGINE_ID))
            .collect(),
    };
    Installation {
        executable,
        model_roots,
        legacy_adapters,
        data_root: data_root.into(),
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

fn contains_adapter(directory: &Path, name: &str) -> bool {
    if name.starts_with('.') || name.contains(['/', '\\', ':']) || name.is_empty() {
        return false;
    }
    let path = directory.join(name);
    if name.ends_with(".safetensors") && path.is_file() {
        return true;
    }
    if path.join("adapter_model.safetensors").is_file()
        || path.join("lokr_weights.safetensors").is_file()
    {
        return true;
    }
    path.join("adapter_config.json").is_file()
        && fs::read_dir(path).is_ok_and(|files| {
            files.filter_map(std::result::Result::ok).any(|file| {
                file.path()
                    .extension()
                    .is_some_and(|extension| extension == "safetensors")
                    && file.path().is_file()
            })
        })
}

impl Installation {
    pub(crate) fn executable(&self) -> Option<&Path> {
        self.executable.as_deref()
    }

    pub fn engine_installed(&self) -> bool {
        self.executable.as_deref().is_some_and(usable_executable)
    }

    pub fn adapter_runtime_update(&self) -> bool {
        self.executable.as_deref().is_some_and(|executable| {
            crate::native_music_runtime::requires_adapter_update(&self.data_root, ENGINE_ID, executable)
        })
    }

    pub fn profiles(&self) -> Vec<InstalledProfile> {
        PROFILES
            .iter()
            .map(|profile| {
                let (root, mut status) = self.profile_root(profile);
                if status.ready && self.engine_installed() {
                    status.native_adapters = self.profile_adapters(profile, root);
                }
                status
            })
            .collect()
    }

    fn profile_adapters(&self, profile: &Profile, root: &Path) -> Vec<crate::adapters::Installed> {
        let Ok(Some(adapters)) = self.adapter_root(root, None) else {
            return Vec::new();
        };
        let family = if profile.weights[0].name.contains("-xl-") { "xl" } else { "2b" };
        let library = crate::adapters::AdapterLibrary::at(adapters, ENGINE_ID);
        library.installed(None).into_iter().filter(|entry| {
            entry.meta.model.as_deref() == Some(family)
                && entry.meta.slots.iter().any(|slot| slot == "dit")
                && contains_adapter(library.root(), &entry.meta.id)
        }).collect()
    }

    fn profile_root(&self, profile: &Profile) -> (&Path, InstalledProfile) {
        // Choose a complete profile from a single installation. Incomplete
        // candidates are useful for reporting the fewest still-missing files.
        let mut candidates = self
            .model_roots
            .iter()
            .map(|root| (root.as_path(), profile.status(root)));
        let mut best = candidates
            .next()
            .expect("there is always a managed or explicit model directory");
        for candidate in candidates {
            if best.1.ready {
                break;
            }
            if candidate.1.missing.len() < best.1.missing.len() {
                best = candidate;
            }
        }
        best
    }

    pub fn launch(&self, profile_id: &str) -> Result<Launch> {
        let profile = PROFILES
            .iter()
            .find(|profile| profile.id == profile_id)
            .with_context(|| format!("unknown ACE-Step profile {profile_id}"))?;
        self.launch_profile(profile, None)
    }

    pub fn launch_replay(&self, profile_id: &str, replay: &serde_json::Value) -> Result<Launch> {
        let declared = PROFILES.iter().find(|profile| profile.id == profile_id);
        let slots = [
            ("synth_model", "dit", 0),
            ("lm_model", "lm", 1),
            ("vae", "vae", 3),
        ];
        if let Some(profile) = declared {
            if slots.iter().all(|(key, _, index)| {
                replay
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|name| name == profile.weights[*index].name)
            }) {
                return self.launch_profile(profile, Some(replay));
            }
        }

        let resolve = |(key, kind, index): (&str, &str, usize)| -> Result<Weight> {
            match replay.get(key).and_then(serde_json::Value::as_str) {
                Some(name) => released_replay_weight(kind, name)
                    .with_context(|| format!("ACE-Step replay model {name} is not in the released {kind} catalogue")),
                None => declared.map(|profile| profile.weights[index])
                    .with_context(|| format!("ACE-Step replay needs the original {key} filename or a recorded profile")),
            }
        };
        let dit = resolve(slots[0])?;
        let lm = resolve(slots[1])?;
        let vae = resolve(slots[2])?;
        // Older custom sets did not record the text encoder in their replay.
        // The released studio accepts either pinned encoder; prefer the one
        // named by a stored profile, otherwise look for the BF16 file first.
        let encoders =
            if declared.is_some_and(|profile| profile.weights[2].name == TE_QWEN3_0_6B_Q8.name) {
                [TE_QWEN3_0_6B_Q8, TE_QWEN3_0_6B_BF16]
            } else {
                [TE_QWEN3_0_6B_BF16, TE_QWEN3_0_6B_Q8]
            };
        let mut profile = Profile {
            id: "legacy-replay",
            label: "ACE-Step replay",
            weights: [dit, lm, encoders[0], vae],
        };
        if declared.is_none() && !self.profile_root(&profile).1.ready {
            profile.weights[2] = encoders[1];
            if !self.profile_root(&profile).1.ready {
                profile.weights[2] = encoders[0];
            }
        }
        self.launch_profile(&profile, Some(replay))
    }

    fn adapter_root(
        &self,
        models: &Path,
        replay: Option<&serde_json::Value>,
    ) -> Result<Option<PathBuf>> {
        let own = self.data_root.join("adapters").join(ENGINE_ID);
        let source_dir = self
            .legacy_adapters
            .iter()
            .find(|(root, _)| root == models)
            .map(|(_, path)| path);
        let Some(replay) = replay else {
            return Ok(if own.is_dir() {
                Some(own)
            } else {
                source_dir.filter(|path| path.is_dir()).cloned()
            });
        };
        let required: Vec<_> = super::adapter_names(replay).collect();
        if required.is_empty() {
            return Ok(None);
        }
        for name in &required {
            anyhow::ensure!(
                Path::new(name).file_name().and_then(|path| path.to_str()) == Some(*name),
                "invalid ACE-Step adapter name {name}"
            );
        }
        source_dir.into_iter().chain(std::iter::once(&own))
            .chain(self.legacy_adapters.iter().map(|(_, folder)| folder))
            .find(|folder| required.iter().all(|name| contains_adapter(folder, name)))
            .cloned().map(Some)
            .with_context(|| format!("the ACE-Step replay needs these adapters together in one adapter directory: {}", required.join(", ")))
    }

    fn launch_profile(
        &self,
        profile: &Profile,
        replay: Option<&serde_json::Value>,
    ) -> Result<Launch> {
        let executable = self
            .executable
            .as_ref()
            .filter(|path| usable_executable(path))
            .with_context(|| {
                format!(
                    "ACE-Step runtime is missing. Install it in Create or select an existing ace-server under {} or ACE_ENGINE_BIN",
                    self.data_root.join("engines").join(ENGINE_ID).display()
                )
            })?;
        let (source, _) = self.profile_root(profile);
        let adapters_root = self.adapter_root(source, replay)?;
        let models_root = profile.prepare_directory(source)?;
        let managed = crate::native_music_runtime::installed(&self.data_root, ENGINE_ID)
            .is_some_and(|path| path == *executable);
        let mut device = std::env::var("ACE_DEVICE").ok().filter(|value| !value.trim().is_empty());
        let mut cuda_backend = std::env::var_os("ACE_CUDA_BACKEND").map(PathBuf::from);
        if managed && cuda_backend.is_none() && device.as_deref().is_none_or(|label| label.starts_with("CUDA")) {
            cuda_backend = crate::native_music_runtime::cuda_backend(executable);
            if cuda_backend.is_some() && device.is_none() {
                device = Some("CUDA0".into());
            }
        }
        Ok(Launch {
            executable: executable.clone(),
            models_root,
            adapters_root,
            device,
            cuda_backend,
            managed_data_root: managed.then(|| self.data_root.clone()),
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
            native_adapters: Vec::new(),
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

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use studio_hub_client::neighbors::Neighbor;

    #[test]
    fn registered_portable_ace_keeps_weights_beside_their_source_volume() {
        let tempfile = std::env::temp_dir().join(format!("ace-discovery-{}", uuid::Uuid::now_v7()));
        let own = tempfile.join("new/data");
        let portable = tempfile.join("portable");
        let data = portable.join("data");
        let source = data.join("models").join(ENGINE_ID);
        let bundled = portable.join("resources/engine");
        let engine_name = if cfg!(windows) {
            "ace-server.exe"
        } else {
            "ace-server"
        };
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&bundled).unwrap();
        fs::write(bundled.join(engine_name), b"fixture").unwrap();
        let neighbor = Neighbor {
            app: "ace".into(),
            version: "3.7.0".into(),
            data_dir: data,
            seen: 1,
        };
        let profile = Profile {
            id: "portable",
            label: "Portable",
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
        for weight in profile.weights {
            fs::write(source.join(weight.name), b"abc").unwrap();
        }
        let empty_package = tempfile.join("missing-package");
        let discover = |bin, root| {
            installation_from(
                &own,
                empty_package.clone(),
                bin,
                None,
                root,
                std::slice::from_ref(&neighbor),
            )
        };
        let installed = discover(None, None);
        assert_eq!(installed.executable.unwrap(), bundled.join(engine_name));
        let installed = discover(None, None);
        assert_eq!(installed.profile_root(&profile).0, source);
        assert!(installed.profile_root(&profile).1.ready);
        let adapters = portable.join("data/adapters");
        let packaged_adapter = adapters.join("saved-lora");
        fs::create_dir_all(&packaged_adapter).unwrap();
        fs::write(packaged_adapter.join("adapter_config.json"), b"{}").unwrap();
        fs::write(packaged_adapter.join("saved-lora.safetensors"), b"fixture").unwrap();
        let manifest = |id, model| serde_json::json!({
            "id": id, "engine": ENGINE_ID, "name": id, "model": model,
            "slots": ["dit"], "origin": {"type": "imported"}
        });
        fs::write(packaged_adapter.join("adapter.json"), manifest("saved-lora", "2b").to_string()).unwrap();
        let wrong_family = adapters.join("wrong-family");
        fs::create_dir(&wrong_family).unwrap();
        fs::write(wrong_family.join("adapter.json"), manifest("wrong-family", "xl").to_string()).unwrap();
        fs::write(wrong_family.join("adapter_model.safetensors"), b"fixture").unwrap();
        let available = installed.profile_adapters(&profile, &source);
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].meta.id, "saved-lora");
        let replay = serde_json::json!({"adapters":[{"name":"saved-lora","scale":1}]});
        assert_eq!(
            installed.adapter_root(&source, Some(&replay)).unwrap(),
            Some(adapters)
        );
        assert!(installed
            .adapter_root(&source, Some(&serde_json::json!({"adapter":"missing"})))
            .is_err());
        assert!(profile
            .prepare_directory(installed.profile_root(&profile).0)
            .unwrap()
            .starts_with(source.canonicalize().unwrap()));

        let own_models = own.join("models").join(ENGINE_ID);
        fs::create_dir_all(&own_models).unwrap();
        for weight in profile.weights {
            fs::write(own_models.join(weight.name), b"xyz").unwrap();
        }
        assert_eq!(discover(None, None).profile_root(&profile).0, own_models);
        let explicit_missing = tempfile.join("explicit-missing");
        let explicit = discover(
            Some(explicit_missing.clone()),
            Some(explicit_missing.clone()),
        );
        assert!(!explicit.engine_installed());
        assert_eq!(explicit.profile_root(&profile).0, explicit_missing);
        assert!(!explicit.profile_root(&profile).1.ready);
        fs::remove_dir_all(tempfile).unwrap();
    }
}

// Additional variants from the released ACE-Step Studio 318cc0ad catalogue.
// Legacy custom sets store their concrete model filenames in the replay.
fn released_replay_weight(kind: &str, name: &str) -> Option<Weight> {
    let index = match kind {
        "dit" => 0,
        "lm" => 1,
        "vae" => 3,
        _ => return None,
    };
    if let Some(weight) = PROFILES
        .iter()
        .map(|profile| profile.weights[index])
        .find(|weight| weight.name == name)
    {
        return Some(weight);
    }
    let extra: &[(&str, u64)] = match kind {
        "dit" => &[
            ("acestep-v15-base-BF16.gguf", 4_791_642_016),
            ("acestep-v15-base-Q4_K_M.gguf", 1_445_710_208),
            ("acestep-v15-base-Q5_K_M.gguf", 1_700_140_160),
            ("acestep-v15-base-Q6_K.gguf", 1_970_472_000),
            ("acestep-v15-base-Q8_0.gguf", 2_549_527_936),
            ("acestep-v15-sft-BF16.gguf", 4_791_642_016),
            ("acestep-v15-sft-Q4_K_M.gguf", 1_445_710_208),
            ("acestep-v15-sft-Q5_K_M.gguf", 1_700_140_160),
            ("acestep-v15-sft-Q6_K.gguf", 1_970_472_000),
            ("acestep-v15-sft-Q8_0.gguf", 2_549_527_936),
            ("acestep-v15-sftturbo50-BF16.gguf", 4_791_642_016),
            ("acestep-v15-sftturbo50-Q8_0.gguf", 2_549_527_936),
            ("acestep-v15-turbo-BF16.gguf", 4_791_642_080),
            ("acestep-v15-turbo-Q5_K_M.gguf", 1_700_140_224),
            ("acestep-v15-turbo-Q6_K.gguf", 1_970_472_064),
            ("acestep-v15-turbo-Q8_0.gguf", 2_549_528_000),
            ("acestep-v15-turbo-continuous-BF16.gguf", 4_791_642_080),
            ("acestep-v15-turbo-continuous-Q4_K_M.gguf", 1_445_710_240),
            ("acestep-v15-turbo-continuous-Q5_K_M.gguf", 1_700_140_192),
            ("acestep-v15-turbo-continuous-Q6_K.gguf", 1_970_472_032),
            ("acestep-v15-turbo-continuous-Q8_0.gguf", 2_549_527_968),
            ("acestep-v15-turbo-shift1-BF16.gguf", 4_791_642_048),
            ("acestep-v15-turbo-shift1-Q4_K_M.gguf", 1_445_710_240),
            ("acestep-v15-turbo-shift1-Q5_K_M.gguf", 1_700_140_192),
            ("acestep-v15-turbo-shift1-Q6_K.gguf", 1_970_472_032),
            ("acestep-v15-turbo-shift1-Q8_0.gguf", 2_549_527_968),
            ("acestep-v15-turbo-shift3-BF16.gguf", 4_791_642_048),
            ("acestep-v15-turbo-shift3-Q4_K_M.gguf", 1_445_710_240),
            ("acestep-v15-turbo-shift3-Q5_K_M.gguf", 1_700_140_192),
            ("acestep-v15-turbo-shift3-Q6_K.gguf", 1_970_472_032),
            ("acestep-v15-turbo-shift3-Q8_0.gguf", 2_549_527_968),
            ("acestep-v15-xl-base-BF16.gguf", 9_978_530_784),
            ("acestep-v15-xl-base-Q4_K_M.gguf", 2_989_922_656),
            ("acestep-v15-xl-base-Q5_K_M.gguf", 3_527_566_432),
            ("acestep-v15-xl-base-Q6_K.gguf", 4_098_812_960),
            ("acestep-v15-xl-base-Q8_0.gguf", 5_305_828_704),
            ("acestep-v15-xl-sft-BF16.gguf", 9_978_530_784),
            ("acestep-v15-xl-sft-Q4_K_M.gguf", 2_989_922_656),
            ("acestep-v15-xl-sft-Q5_K_M.gguf", 3_527_566_432),
            ("acestep-v15-xl-sft-Q6_K.gguf", 4_098_812_960),
            ("acestep-v15-xl-sft-Q8_0.gguf", 5_305_828_704),
            ("acestep-v15-xl-sftturbo50-BF16.gguf", 9_978_530_816),
            ("acestep-v15-xl-sftturbo50-Q4_K_M.gguf", 2_989_922_720),
            ("acestep-v15-xl-sftturbo50-Q5_K_M.gguf", 3_527_566_496),
            ("acestep-v15-xl-sftturbo50-Q6_K.gguf", 4_098_813_024),
            ("acestep-v15-xl-sftturbo50-Q8_0.gguf", 5_305_828_768),
            ("acestep-v15-xl-turbo-Q5_K_M.gguf", 3_527_566_464),
            (
                "acestep-v15-merge-base-sft-xl-ta-0.5-BF16.gguf",
                9_978_530_784,
            ),
            (
                "acestep-v15-merge-base-sft-xl-ta-0.5-Q4_K_M.gguf",
                2_989_922_688,
            ),
            (
                "acestep-v15-merge-base-sft-xl-ta-0.5-Q5_K_M.gguf",
                3_527_566_464,
            ),
            (
                "acestep-v15-merge-base-sft-xl-ta-0.5-Q6_K.gguf",
                4_098_812_992,
            ),
            (
                "acestep-v15-merge-base-sft-xl-ta-0.5-Q8_0.gguf",
                5_305_828_736,
            ),
            (
                "acestep-v15-merge-base-turbo-xl-ta-0.5-BF16.gguf",
                9_978_530_784,
            ),
            (
                "acestep-v15-merge-base-turbo-xl-ta-0.5-Q4_K_M.gguf",
                2_989_922_688,
            ),
            (
                "acestep-v15-merge-base-turbo-xl-ta-0.5-Q5_K_M.gguf",
                3_527_566_464,
            ),
            (
                "acestep-v15-merge-base-turbo-xl-ta-0.5-Q6_K.gguf",
                4_098_812_992,
            ),
            (
                "acestep-v15-merge-base-turbo-xl-ta-0.5-Q8_0.gguf",
                5_305_828_736,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.3-BF16.gguf",
                9_978_530_784,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q4_K_M.gguf",
                2_989_922_688,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q5_K_M.gguf",
                3_527_566_464,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q6_K.gguf",
                4_098_812_992,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q8_0.gguf",
                5_305_828_736,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.7-BF16.gguf",
                9_978_530_784,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q4_K_M.gguf",
                2_989_922_688,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q5_K_M.gguf",
                3_527_566_464,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q6_K.gguf",
                4_098_812_992,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q8_0.gguf",
                5_305_828_736,
            ),
            ("acestep-v15-base-MXFP4.gguf", 1_281_885_888),
            (
                "acestep-v15-merge-base-turbo-xl-ta-0.5-MXFP4.gguf",
                2_660_726_464,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.3-MXFP4.gguf",
                2_660_726_464,
            ),
            (
                "acestep-v15-merge-sft-turbo-xl-ta-0.7-MXFP4.gguf",
                2_660_726_464,
            ),
            ("acestep-v15-sft-MXFP4.gguf", 1_281_885_888),
            ("acestep-v15-sftturbo50-MXFP4.gguf", 1_281_885_888),
            ("acestep-v15-turbo-MXFP4.gguf", 1_281_885_952),
            ("acestep-v15-turbo-continuous-MXFP4.gguf", 1_281_885_920),
            ("acestep-v15-turbo-shift1-MXFP4.gguf", 1_281_885_920),
            ("acestep-v15-turbo-shift3-MXFP4.gguf", 1_281_885_920),
            ("acestep-v15-xl-base-MXFP4.gguf", 2_660_726_432),
            ("acestep-v15-xl-sft-MXFP4.gguf", 2_660_726_432),
            ("acestep-v15-xl-sftturbo50-MXFP4.gguf", 2_660_726_496),
            ("acestep_1.5_xl_turbo_regrind_v1-Q4_K_M.gguf", 2_989_922_688),
            ("acestep_1.5_xl_turbo_regrind_v1-Q6_K.gguf", 4_098_812_992),
            ("acestep_1.5_xl_turbo_regrind_v1-Q8_0.gguf", 5_305_828_736),
        ],
        "lm" => &[
            ("acestep-5Hz-lm-0.6B-BF16.gguf", 1_331_108_128),
            ("acestep-5Hz-lm-1.7B-BF16.gguf", 3_713_827_104),
            ("acestep-5Hz-lm-4B-Q5_K_M.gguf", 3_025_965_984),
            ("acestep-5Hz-lm-4B-Q6_K.gguf", 3_442_713_504),
        ],
        "vae" => &[
            ("scragvae-BF16.gguf", 337_420_928),
            ("acestep_1.5_vae_Regrind_V10b-BF16.gguf", 337_420_960),
            (
                "acestep_1.5_vae_Regrind_V10b_blend50-BF16.gguf",
                337_420_928,
            ),
            ("acestep_1.5_vae_Regrind_V9b-BF16.gguf", 337_420_928),
            ("acestep_1.5_vae_Regrind_V9b_blend50-BF16.gguf", 337_420_928),
        ],
        _ => return None,
    };
    extra
        .iter()
        .find(|(filename, _)| *filename == name)
        .map(|&(name, bytes)| Weight { name, bytes })
}
