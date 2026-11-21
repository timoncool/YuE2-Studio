use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ApiError, AppState, api_error};
use crate::downloads::{Asset, AssetKind, DownloadProgress, Downloader};
use anyhow::{Context, ensure};

const SCOPE: &str = "yue2-engine-hip";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum Gfx {
    #[serde(rename = "gfx1036")]
    Gfx1036,
    #[serde(rename = "gfx1100")]
    Gfx1100,
}

impl Gfx {
    fn name(self) -> &'static str {
        match self {
            Self::Gfx1036 => "gfx1036",
            Self::Gfx1100 => "gfx1100",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Adapter {
    #[serde(rename = "PNPDeviceID")]
    pnp_device_id: Option<String>,
    driver_version: Option<String>,
}

#[derive(Clone, Debug)]
struct Host {
    gfx: Gfx,
    driver: Option<String>,
}

fn select_host(adapters: &[Adapter]) -> Result<Host, String> {
    let mut amd = adapters.iter().filter_map(|adapter| {
        let id = adapter.pnp_device_id.as_ref()?.to_ascii_uppercase();
        id.starts_with(r"PCI\VEN_1002&").then_some((id, adapter))
    });
    let Some((id, adapter)) = amd.next() else {
        return Err(
            "No AMD PCI display adapter was detected for HIP. Choose Vulkan or Auto.".into(),
        );
    };
    if amd.next().is_some() {
        return Err("Multiple AMD adapters were detected. Their ROCm order cannot be verified. Choose Vulkan or Auto.".into());
    }
    let device = id
        .split('&')
        .nth(1)
        .and_then(|field| field.strip_prefix("DEV_"));
    let gfx = match device {
        Some("13C0") => Gfx::Gfx1036,
        // ROCm's PCI map identifies these devices as gfx1100.
        Some("7448" | "7449" | "744A" | "744C" | "745E") => Gfx::Gfx1100,
        _ => {
            return Err(
                "No HIP package matches this AMD PCI device. Choose Vulkan or Auto.".into(),
            );
        }
    };
    Ok(Host {
        gfx,
        driver: adapter.driver_version.clone(),
    })
}

fn query_host() -> Result<Host, String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("Native YuE2 HIP components are available on Windows x64.".into());
    }
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or_else(|| "Windows system directory could not be identified.".to_string())?;
    let mut command =
        std::process::Command::new(windows.join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    command.args(["-NoProfile", "-NonInteractive", "-Command",
        "ConvertTo-Json -InputObject @(Get-CimInstance Win32_VideoController | Select-Object PNPDeviceID,DriverVersion) -Compress"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let adapters = command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice::<Vec<Adapter>>(&output.stdout).ok())
        .ok_or_else(|| {
            "Unable to identify AMD PCI devices on this Windows host. Choose Vulkan or Auto."
                .to_string()
        })?;
    select_host(&adapters)
}

fn detect_host(refresh: bool) -> Result<Host, String> {
    use std::time::{Duration, Instant};
    type Report = (Instant, Result<Host, String>);
    static HOST: std::sync::Mutex<Option<Report>> = std::sync::Mutex::new(None);
    let mut cached = HOST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !refresh
        && let Some((checked, result)) = cached.as_ref()
        && checked.elapsed() < Duration::from_secs(3)
    {
        return result.clone();
    }
    let result = query_host();
    *cached = Some((Instant::now(), result.clone()));
    result
}
struct Package {
    gfx: Gfx,
    revision: &'static str,
    sha256: &'static str,
    asset: Asset,
}

fn package(gfx: Gfx) -> Option<&'static Package> {
    PACKAGES.iter().find(|package| package.gfx == gfx)
}

const ENGINE_REVISION: &str = "188d13b3d619cc0fd13f47db9ff64e0bd815d577";
const ENGINE_SHA: &str = "b6ef627d585b57ebbb5c284b7492c42e341f45dc7a1e5002288ccd3574363864";

const PACKAGES: &[Package] = &[
    Package {
        gfx: Gfx::Gfx1036,
        revision: ENGINE_REVISION,
        sha256: "7ccf312e5236c190b25bca9d119cc8e2062ac49fa7fde228a5aabde1aa5f085c",
        asset: Asset {
            id: "yue2-hip-gfx1036",
            label: "YuE2 HIP (gfx1036)",
            kind: AssetKind::Runtime,
            url: "https://github.com/timoncool/yue2.cpp/releases/download/studio-hip-188d13b-4e38eee/yue2-windows-gfx1036.zip",
            relative_path: "yue2-windows-gfx1036.zip",
            bytes: 105_088_399,
            unzip_into: None,
            marker: "",
            pick: &[],
            vram_gb: None,
            note: "ROCm 10.1.0 Windows x64 runtime",
        },
    },
    Package {
        gfx: Gfx::Gfx1100,
        revision: ENGINE_REVISION,
        sha256: "4f977f2a017ea8e93936801f8566caa641c971260ef1d4a2a3ca6531109e5c0e",
        asset: Asset {
            id: "yue2-hip-gfx1100",
            label: "YuE2 HIP (gfx1100)",
            kind: AssetKind::Runtime,
            url: "https://github.com/timoncool/yue2.cpp/releases/download/studio-hip-188d13b-4e38eee/yue2-windows-gfx1100.zip",
            relative_path: "yue2-windows-gfx1100.zip",
            bytes: 115_027_012,
            unzip_into: None,
            marker: "",
            pick: &[],
            vram_gb: None,
            note: "ROCm 10.1.0 Windows x64 runtime",
        },
    },
];

pub(super) fn archive_sha(url: &str) -> Option<&'static str> {
    PACKAGES
        .iter()
        .find(|package| package.asset.url == url)
        .map(|package| package.sha256)
}

#[derive(Deserialize)]
struct Build {
    gpu_target: String,
    rocm: String,
    engine_revision: String,
    engine_sha256: String,
}

fn executable(root: &Path, package: &Package) -> anyhow::Result<PathBuf> {
    let manifest =
        fs::read(root.join("BUILD.json")).context("Read installed HIP build metadata")?;
    let build: Build = serde_json::from_slice(&manifest)?;
    ensure!(
        build.gpu_target == package.gfx.name()
            && build.rocm == "10.1.0"
            && build.engine_revision == package.revision,
        "The installed HIP build does not match this device and engine revision."
    );
    ensure!(
        build.engine_sha256 == ENGINE_SHA,
        "The HIP build manifest has an unexpected executable checksum."
    );
    for file in [
        "yue-server.exe",
        "ggml-hip.dll",
        "amdhip64_7.dll",
        "hipblas.dll",
        "rocblas.dll",
        "rocm_kpack.dll",
        "libomp140.x86_64.dll",
    ] {
        ensure!(
            root.join("bin").join(file).is_file(),
            "The HIP component is missing {file}."
        );
    }
    ensure!(
        root.join("bin/rocblas/library").is_dir(),
        "The HIP component is missing its rocBLAS device data."
    );
    Ok(root.join("bin/yue-server.exe"))
}

#[derive(Clone)]
pub(super) struct Runtime {
    downloads: Downloader,
    installing: Arc<tokio::sync::Mutex<()>>,
}

impl Runtime {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            downloads: Downloader::new(root.join("engine-runtime/hip")),
            installing: Default::default(),
        }
    }

    fn root(&self, package: &Package) -> PathBuf {
        self.downloads
            .root()
            .join(package.gfx.name())
            .join(&package.sha256[..12])
    }

    fn installed(&self, package: &Package) -> anyhow::Result<PathBuf> {
        let root = self.root(package);
        ensure!(
            crate::install_record::matches(&root.join("install.json"), package.asset.url, &root)
                == Some(true),
            "The HIP component is absent or incomplete."
        );
        executable(&root, package)
    }

    pub(super) async fn verified_executable(&self) -> Result<PathBuf, String> {
        self.selected_path(true).await
    }

    pub(super) async fn selected_path(&self, verify: bool) -> Result<PathBuf, String> {
        let runtime = self.clone();
        tokio::task::spawn_blocking(move || {
            let host = detect_host(verify)?;
            let package = package(host.gfx).ok_or_else(|| "No published HIP component is available for this device.".to_string())?;
            let dir = runtime.root(package);
            if !dir.is_dir() {
                return Err(format!("Install the YuE2 HIP component for {} in Engine Settings, or choose Vulkan.", host.gfx.name()));
            }
            let binary = runtime.installed(package).map_err(|error| error.to_string())?;
            #[cfg(windows)]
            if verify {
                music_engine::yue_server::hip_tensile_directory(&binary).map_err(|error| error.to_string())?;
            }
            if verify && !crate::pin_sha::holds(&binary, ENGINE_SHA) {
                return Err("The HIP executable checksum changed. Reinstall the component or choose Vulkan.".into());
            }
            Ok(binary)
        }).await.map_err(|error| error.to_string())?
    }

    fn unpack(&self, package: &Package) -> anyhow::Result<()> {
        let archive = self.downloads.path_of(&package.asset);
        crate::pin_sha::check(package.asset.url, &archive)?;
        let target = self.root(package);
        let parent = target.parent().context("HIP runtime directory")?;
        fs::create_dir_all(parent)?;
        let stage = tempfile::tempdir_in(parent)?;
        let files = extract_package(&archive, stage.path())?;
        let binary = executable(stage.path(), package)?;
        ensure!(
            crate::pin_sha::holds(&binary, ENGINE_SHA),
            "The HIP executable checksum does not match the build manifest."
        );
        crate::install_record::write(
            &stage.path().join("install.json"),
            package.asset.url,
            package.sha256,
            &files,
            stage.path(),
        )?;
        let backup = tempfile::tempdir_in(parent)?;
        let previous = backup.path().join("previous");
        if target.exists() {
            fs::rename(&target, &previous)?;
        }
        if let Err(error) = fs::rename(stage.path(), &target) {
            if previous.exists() {
                fs::rename(previous, target).context("Restore the previous HIP installation")?;
            }
            return Err(error.into());
        }
        Ok(())
    }
}

fn extract_package(archive: &Path, destination: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    let mut files = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let path = entry
            .enclosed_name()
            .context("Unsafe path in HIP component")?;
        ensure!(
            !path.to_string_lossy().contains(':'),
            "Unsafe path in HIP component"
        );
        if entry.is_dir() {
            continue;
        }
        let path = destination.join(path);
        fs::create_dir_all(path.parent().context("HIP component path")?)?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        files.push(path);
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(files)
}

#[derive(Serialize)]
pub(super) struct Status {
    platform_supported: bool,
    gfx: Option<Gfx>,
    driver: Option<String>,
    supported: bool,
    active: bool,
    tested_driver: Option<&'static str>,
    pub(super) installed: bool,
    pub(super) missing_bytes: u64,
    installing: bool,
    pub(super) reason: Option<String>,
    download: Option<DownloadProgress>,
}

impl Runtime {
    pub(super) async fn status(&self) -> Status {
        let host = tokio::task::spawn_blocking(|| detect_host(false))
            .await
            .unwrap_or_else(|error| Err(error.to_string()));
        let mut output = Status {
            platform_supported: cfg!(all(windows, target_arch = "x86_64")),
            gfx: None,
            driver: None,
            supported: false,
            active: false,
            tested_driver: None,
            installed: false,
            missing_bytes: 0,
            installing: self.installing.try_lock().is_err(),
            reason: None,
            download: self.downloads.active_for(SCOPE).await,
        };
        match host {
            Ok(host) => {
                output.gfx = Some(host.gfx);
                output.driver = host.driver.clone();
                output.tested_driver = (host.gfx == Gfx::Gfx1036).then_some("32.0.21034.41");
                if let Some(package) = package(host.gfx) {
                    output.supported = true;
                    output.installed = self.installed(package).is_ok();
                    if !output.installed {
                        let asset = &package.asset;
                        output.missing_bytes = if self.downloads.is_installed(asset) {
                            0
                        } else {
                            asset
                                .bytes
                                .saturating_sub(self.downloads.partial_bytes(asset))
                        };
                    }
                } else {
                    output.reason =
                        Some("No published HIP component is available for this device.".into());
                }
            }
            Err(reason) => output.reason = Some(reason),
        }
        output
    }
}

pub(super) async fn get(State(state): State<AppState>) -> Json<Status> {
    let mut status = state.engine_hip.status().await;
    let using_hip =
        *state.active_device.read().await == Some(music_engine::yue_server::ComputeBackend::Hip);
    status.active = using_hip && state.music_server.health().await;
    Json(status)
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<ApiError>)> {
    let runtime = state.engine_hip.clone();
    let host = tokio::task::spawn_blocking(|| detect_host(true))
        .await
        .unwrap_or_else(|error| Err(error.to_string()))
        .map_err(|error| api_error(StatusCode::CONFLICT, error))?;
    let package = package(host.gfx).ok_or_else(|| {
        api_error(
            StatusCode::CONFLICT,
            "No published HIP component is available for this device.".into(),
        )
    })?;
    let guard = runtime.installing.clone().try_lock_owned().map_err(|_| {
        api_error(
            StatusCode::CONFLICT,
            "The HIP installation is already running.".into(),
        )
    })?;
    runtime.downloads.clear_finished(SCOPE).await;
    tokio::spawn(async move {
        let _guard = guard;
        let result = async {
            runtime
                .downloads
                .install_all(SCOPE, &[&package.asset])
                .await?;
            let staging = runtime.clone();
            tokio::task::spawn_blocking(move || staging.unpack(package)).await?
        }
        .await;
        if let Err(error) = result {
            runtime.downloads.fail(SCOPE, format!("{error:#}")).await;
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"started": true})),
    ))
}

pub(super) async fn cancel(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({"cancelled": state.engine_hip.downloads.cancel_for(SCOPE).await}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "use distribution/components/test_hip_mirrors.py with the signed HTTPS fixture"]
    async fn pinned_hip_installs_after_the_signed_mirror_changes() {
        let fixture = PathBuf::from(std::env::var_os("YUE_HIP_MIRROR_QA").unwrap());
        let mut runtime = Runtime::new(&fixture.join("studio"));
        let port: u16 = fs::read_to_string(fixture.join("port"))
            .unwrap()
            .parse()
            .unwrap();
        let ca =
            reqwest::Certificate::from_pem(&fs::read(fixture.join("ca.pem")).unwrap()).unwrap();
        runtime.downloads = runtime.downloads.with_test_http(
            reqwest::Client::builder()
                .no_proxy()
                .resolve(
                    "github.com",
                    std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                )
                .tls_certs_only([ca])
                .build()
                .unwrap(),
        );
        let gfx1036 = package(Gfx::Gfx1036).unwrap();
        let first = runtime
            .downloads
            .install_all(SCOPE, &[&gfx1036.asset])
            .await;
        assert!(first.unwrap_err().to_string().contains("404"));
        assert!(runtime.installed(gfx1036).is_err());
        fs::write(fixture.join("first-failed"), []).unwrap();
        for _ in 0..200 {
            if fixture.join("second-published").is_file() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(fixture.join("second-published").is_file());
        runtime
            .downloads
            .install_all(SCOPE, &[&gfx1036.asset])
            .await
            .unwrap();
        runtime.unpack(gfx1036).unwrap();
        assert!(runtime.installed(gfx1036).unwrap().is_file());
        let root = runtime.root(gfx1036);
        let record = crate::install_record::read(&root.join("install.json")).unwrap();
        assert_eq!(record.url, gfx1036.asset.url);
        assert_eq!(record.sha256, gfx1036.sha256);
    }

    fn adapter(pnp: &str) -> Adapter {
        Adapter {
            pnp_device_id: Some(pnp.into()),
            driver_version: None,
        }
    }

    #[test]
    fn published_hip_pins_are_reachable_by_catalogue_and_bind_the_engine_digest() {
        let temp = tempfile::tempdir().unwrap();
        for package in PACKAGES {
            assert_eq!(
                crate::pin_sha::sha256(package.asset.url),
                Some(package.sha256)
            );
            let mirrors = crate::component_catalogue::bundled()
                .sources(package.sha256)
                .unwrap();
            assert_eq!(mirrors.first().map(String::as_str), Some(package.asset.url));
            fs::write(
                temp.path().join("BUILD.json"),
                serde_json::to_vec(&serde_json::json!({
                    "gpu_target": package.gfx.name(), "rocm": "10.1.0",
                    "engine_revision": package.revision, "engine_sha256": "0".repeat(64),
                }))
                .unwrap(),
            )
            .unwrap();
            assert!(
                executable(temp.path(), package)
                    .unwrap_err()
                    .to_string()
                    .contains("unexpected executable checksum")
            );
        }
    }

    #[test]
    fn hip_zip_preserves_rocblas_directories_and_rejects_traversal() {
        use std::io::Write;
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.zip");
        let root = temp.path().join("staged");
        let entries = |paths: &[&str]| {
            let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            for path in paths {
                writer
                    .start_file(*path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(b"fixture").unwrap();
            }
            writer.finish().unwrap();
        };
        entries(&[
            "BUILD.json",
            "bin/yue-server.exe",
            "bin/rocblas/library/target.dat",
        ]);
        assert_eq!(extract_package(&archive, &root).unwrap().len(), 3);
        assert_eq!(
            fs::read(root.join("bin/rocblas/library/target.dat")).unwrap(),
            b"fixture"
        );
        entries(&["../outside.dll"]);
        assert!(extract_package(&archive, &root).is_err());
        assert!(!temp.path().join("outside.dll").exists());
    }

    #[test]
    fn selects_known_amd_pci_and_refuses_ambiguous_ordinals() {
        let amd = r"PCI\VEN_1002&DEV_13C0&SUBSYS_00000000";
        assert_eq!(
            select_host(&[adapter(amd), adapter(r"PCI\VEN_10DE&DEV_2684")])
                .unwrap()
                .gfx,
            Gfx::Gfx1036
        );
        assert_eq!(
            select_host(&[adapter(r"PCI\VEN_1002&DEV_744C")])
                .unwrap()
                .gfx,
            Gfx::Gfx1100
        );
        assert!(
            select_host(&[adapter(amd), adapter(r"PCI\VEN_1002&DEV_0000")])
                .unwrap_err()
                .contains("Multiple AMD")
        );
        assert!(select_host(&[adapter(r"PCI\VEN_1002&DEV_0000")]).is_err());
    }
}
