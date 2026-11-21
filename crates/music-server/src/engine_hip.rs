use super::*;
use crate::downloads::{Asset, DownloadProgress, Downloader};
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

#[derive(Debug)]
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

fn detect_host() -> Result<&'static Host, String> {
    static HOST: std::sync::OnceLock<Result<Host, String>> = std::sync::OnceLock::new();
    HOST.get_or_init(|| {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return Err("Native YuE2 HIP components are available on Windows x64.".into());
        }
        let mut command = std::process::Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command",
            "ConvertTo-Json -InputObject @(Get-CimInstance Win32_VideoController | Select-Object PNPDeviceID,DriverVersion) -Compress"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let adapters = command.output().ok().filter(|output| output.status.success())
            .and_then(|output| serde_json::from_slice::<Vec<Adapter>>(&output.stdout).ok())
            .ok_or_else(|| "Unable to identify AMD PCI devices on this Windows host. Choose Vulkan or Auto.".to_string())?;
        select_host(&adapters)
    }).as_ref().map_err(Clone::clone)
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

const PACKAGES: &[Package] = &[];

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
        build.engine_sha256.len() == 64
            && build
                .engine_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "The HIP build metadata has no valid engine checksum."
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
            .join(package.sha256)
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
        let runtime = self.clone();
        tokio::task::spawn_blocking(move || {
            let host = detect_host()?;
            let package = package(host.gfx).ok_or_else(|| "No published HIP component is available for this device.".to_string())?;
            let dir = runtime.root(package);
            if !dir.is_dir() {
                return Err(format!("Install the YuE2 HIP component for {} in Engine Settings, or choose Vulkan.", host.gfx.name()));
            }
            let binary = runtime.installed(package).map_err(|error| error.to_string())?;
            let build: Build = serde_json::from_slice(&fs::read(dir.join("BUILD.json")).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
            if !crate::pin_sha::holds(&binary, &build.engine_sha256) {
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
            let path = stage.path().join(path);
            fs::create_dir_all(path.parent().context("HIP component path")?)?;
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
            files.push(path);
            std::io::copy(&mut entry, &mut file)?;
        }
        let binary = executable(stage.path(), package)?;
        let build: Build = serde_json::from_slice(&fs::read(stage.path().join("BUILD.json"))?)?;
        ensure!(
            crate::pin_sha::holds(&binary, &build.engine_sha256),
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

#[derive(Serialize)]
pub(super) struct Status {
    platform_supported: bool,
    gfx: Option<Gfx>,
    driver: Option<String>,
    supported: bool,
    pub(super) installed: bool,
    pub(super) missing_bytes: u64,
    installing: bool,
    pub(super) reason: Option<String>,
    download: Option<DownloadProgress>,
}

impl Runtime {
    pub(super) async fn status(&self) -> Status {
        let host = tokio::task::spawn_blocking(detect_host)
            .await
            .unwrap_or_else(|error| Err(error.to_string()));
        let mut output = Status {
            platform_supported: cfg!(all(windows, target_arch = "x86_64")),
            gfx: None,
            driver: None,
            supported: false,
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
    Json(state.engine_hip.status().await)
}

pub(super) async fn install(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<ApiError>)> {
    let runtime = state.engine_hip.clone();
    let host = tokio::task::spawn_blocking(detect_host)
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

    fn adapter(pnp: &str) -> Adapter {
        Adapter {
            pnp_device_id: Some(pnp.into()),
            driver_version: None,
        }
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
