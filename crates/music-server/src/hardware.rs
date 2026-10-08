//! The machine the studio runs on: its GPU, how much memory it has, and the
//! model set that fits in it.

use std::{process::Command, sync::OnceLock};

use serde::Serialize;
use sysinfo::System;

/// The set a clean install selects when no card fits any set: the lightest,
/// so the model manager still names a concrete local target.
const FALLBACK_PROFILE: &str = "light";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub gpu_name: Option<String>,
    pub total_vram_gb: f64,
    pub total_ram_gb: f64,
    /// An NVIDIA card: the engine runs on CUDA there, which needs cuBLAS.
    /// Every other card runs on Vulkan and needs nothing downloaded.
    pub nvidia: bool,
    /// The CUDA build of the engine this card and its driver run, none when
    /// neither does: such a card computes on Vulkan.
    pub cuda: Option<CudaBuild>,
    /// The NVIDIA card's compute capability, `[7, 5]` for Turing.
    pub compute_capability: Option<(u32, u32)>,
    /// The model set id that fits this card, none when no set does.
    pub recommended: Option<&'static str>,
}

/// The engine ships one CUDA backend per toolkit. CUDA 13 targets Turing and
/// newer and needs a driver from its own release on; CUDA 12 carries the
/// Maxwell, Pascal and Volta cards CUDA 13 dropped, and every newer card whose
/// driver predates CUDA 13. Both hold device code for each architecture, so no
/// driver ever compiles PTX.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CudaBuild {
    Cuda12,
    Cuda13,
}

impl CudaBuild {
    /// The folder beside yue-server.exe that holds this build's ggml-cuda.dll.
    pub fn folder(self) -> &'static str {
        match self {
            CudaBuild::Cuda12 => "cuda12",
            CudaBuild::Cuda13 => "cuda13",
        }
    }
}

/// The first driver of each CUDA major line on Windows, from NVIDIA's CUDA
/// compatibility tables: minor version compatibility runs a whole major line
/// on it, and device code needs nothing newer.
pub const CUDA13_DRIVER: u32 = 580;
const CUDA12_DRIVER: u32 = 525;

/// The oldest architecture the CUDA 12 build has device code for: 5.2, the
/// Maxwell of the GTX 900 series and the Tesla M40.
const CUDA12_OLDEST: (u32, u32) = (5, 2);
/// Turing, the oldest architecture CUDA 13 still targets.
const CUDA13_OLDEST: (u32, u32) = (7, 5);

/// Picks the build from the card's compute capability and the driver's major
/// version, the way Ollama chooses between its CUDA runners.
pub fn cuda_build(compute: (u32, u32), driver_major: u32) -> Option<CudaBuild> {
    if compute >= CUDA13_OLDEST && driver_major >= CUDA13_DRIVER {
        Some(CudaBuild::Cuda13)
    } else if compute >= CUDA12_OLDEST && driver_major >= CUDA12_DRIVER {
        Some(CudaBuild::Cuda12)
    } else {
        None
    }
}

/// Tensor cores before Ampere accumulate in FP16, where the V projection can
/// overflow to infinity and poison every later attention; the engine's clamp
/// keeps it in range and changes nothing on a card that never overflows.
pub fn accumulates_in_fp16() -> bool {
    probe().compute_capability.is_some_and(|compute| compute < (8, 0))
}

/// `nvidia-smi` costs tens of milliseconds and the setup screen polls status
/// once per second while a download runs. The machine's GPU does not change
/// inside one process lifetime, so probe it once.
fn probe() -> &'static Hardware {
    static HARDWARE: OnceLock<Hardware> = OnceLock::new();
    HARDWARE.get_or_init(|| {
        let mut system = System::new();
        system.refresh_memory();
        let total_ram_gb = system.total_memory() as f64 / 1_000_000_000.0;
        let (gpu_name, total_vram_gb, nvidia) = match nvidia_smi() {
            Some((name, vram)) => (Some(name), vram, true),
            None => match display_adapter() {
                Some((name, vram)) => (Some(name), vram, false),
                None => (None, 0.0, false),
            },
        };
        let device = if nvidia { nvidia_cuda_device() } else { None };
        let cuda = device.and_then(|(compute, driver)| cuda_build(compute, driver));
        let compute_capability = device.map(|(compute, _)| compute);
        Hardware {
            gpu_name,
            total_vram_gb,
            total_ram_gb,
            nvidia,
            cuda,
            compute_capability,
            recommended: profile_for_vram(total_vram_gb),
        }
    })
}

pub fn hardware() -> Hardware {
    probe().clone()
}

/// VRAM tiers follow the strict-eviction peak of each set: the larger backbone
/// half plus the KV cache and the compute buffers. The cache is sized to the
/// song, about 1.5 GB for 130 s under guidance, so the tiers keep headroom for
/// the longest songs.
fn profile_for_vram(total_vram_gb: f64) -> Option<&'static str> {
    if total_vram_gb >= 12.0 {
        Some("native")
    } else if total_vram_gb >= 8.0 {
        Some("quality-q8")
    } else if total_vram_gb >= 7.0 {
        Some("balanced")
    } else if total_vram_gb >= 5.5 {
        Some("light")
    } else {
        None
    }
}

/// Chooses the complete local set on a clean install. This only records a
/// selection; downloading any component remains a separate user action.
pub fn recommended_local_profile() -> &'static str {
    probe().recommended.unwrap_or(FALLBACK_PROFILE)
}

fn quiet(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A GUI process spawning a console tool flashes a window without this.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The card the studio was pointed at (`apply_saved_gpu`), as nvidia-smi's
/// `-i` takes it: the same PCI order CUDA_DEVICE_ORDER=PCI_BUS_ID gives CUDA.
pub(crate) fn chosen_card() -> Vec<String> {
    match std::env::var("CUDA_VISIBLE_DEVICES").ok().and_then(|value| value.trim().parse::<u32>().ok()) {
        Some(index) => vec!["-i".into(), index.to_string()],
        None => Vec::new(),
    }
}

/// Every NVIDIA card: its nvidia-smi index, name and memory in GB.
pub fn nvidia_cards() -> Vec<(u32, String, f64)> {
    let Ok(output) = quiet("nvidia-smi").args(["--query-gpu=index,name,memory.total", "--format=csv,noheader,nounits"]).output() else { return Vec::new() };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            let index = fields.next()?.parse().ok()?;
            let name = fields.next()?.to_string();
            let memory = fields.next()?.parse::<f64>().ok()? / 1024.0;
            Some((index, name, memory))
        })
        .collect()
}

fn nvidia_smi() -> Option<(String, f64)> {
    let output = quiet("nvidia-smi").args(chosen_card()).args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).lines().next()?.trim().to_owned();
    let (name, memory) = line.rsplit_once(',')?;
    Some((name.trim().into(), memory.trim().parse::<f64>().ok()? / 1024.0))
}

/// Asked apart from the name and memory: a driver too old to know
/// `compute_cap` fails the whole query, and such a driver runs neither build.
fn nvidia_cuda_device() -> Option<((u32, u32), u32)> {
    let output = quiet("nvidia-smi").args(chosen_card()).args(["--query-gpu=compute_cap,driver_version", "--format=csv,noheader"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).lines().next()?.trim().to_owned();
    parse_cuda_query(&line)
}

/// `7.5, 581.29` into the compute capability and the driver's major version.
fn parse_cuda_query(line: &str) -> Option<((u32, u32), u32)> {
    let (compute, driver) = line.split_once(',')?;
    let (major, minor) = compute.trim().split_once('.')?;
    let driver_major = driver.trim().split('.').next()?.parse().ok()?;
    Some(((major.parse().ok()?, minor.parse().ok()?), driver_major))
}

/// The display adapter with the most dedicated memory, from the driver's own
/// registry entry: `HardwareInformation.qwMemorySize` is the 64-bit size the
/// driver reports, where WMI's `AdapterRAM` wraps at 4 GB.
#[cfg(windows)]
fn display_adapter() -> Option<(String, f64)> {
    const CLASS: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let query = |value: &str| -> Option<String> {
        let output = quiet("reg").args(["query", CLASS, "/s", "/v", value]).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    best_adapter(&query("DriverDesc")?, &query("HardwareInformation.qwMemorySize")?)
}

#[cfg(target_os = "macos")]
fn display_adapter() -> Option<(String, f64)> {
    // Apple Silicon has one GPU per machine and no separate VRAM: its memory
    // is the system's (unified), so the whole RAM is what a model set has to
    // share with the rest of the machine. The chip name is the only identity
    // macOS reports for it.
    let output = quiet("system_profiler").args(["-json", "SPDisplaysDataType"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let chip = apple_chip(&text)?;
    let ram = quiet("sysctl").args(["-n", "hw.memsize"]).output().ok().and_then(|output| {
        std::str::from_utf8(&output.stdout).ok().and_then(|value| value.trim().parse::<u64>().ok())
    })?;
    Some((chip, ram as f64 / 1_000_000_000.0))
}

/// The chip name from `system_profiler -json SPDisplaysDataType`: the value of
/// `"sppci_model" : "Apple M2 Max"`.
#[cfg(any(target_os = "macos", test))]
fn apple_chip(profile: &str) -> Option<String> {
    let after_key = profile.split("\"sppci_model\"").nth(1)?;
    let name = after_key.split('"').nth(1)?;
    name.starts_with("Apple ").then(|| name.to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn display_adapter() -> Option<(String, f64)> {
    None
}

/// Joins `reg query /s` listings of the adapter names and memory sizes by
/// their subkey and keeps the adapter with the most memory.
fn best_adapter(names: &str, sizes: &str) -> Option<(String, f64)> {
    fn values(listing: &str) -> Vec<(String, String)> {
        let mut key = String::new();
        let mut out = Vec::new();
        for line in listing.lines() {
            if line.starts_with("HKEY_") {
                key = line.trim().to_owned();
            } else if let Some((_, value)) = line.trim().split_once("    REG_") {
                if let Some((_, data)) = value.split_once("    ") {
                    out.push((key.clone(), data.trim().to_owned()));
                }
            }
        }
        out
    }
    let names = values(names);
    values(sizes)
        .into_iter()
        .filter_map(|(key, size)| {
            let bytes = u64::from_str_radix(size.trim_start_matches("0x"), 16).ok()?;
            let name = names.iter().find(|(name_key, _)| *name_key == key)?.1.clone();
            (!name.starts_with("Microsoft")).then_some((name, bytes as f64 / 1024.0 / 1024.0 / 1024.0))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_apple_chip_is_read_from_the_system_profiler_json() {
        let profile = r#"{ "SPDisplaysDataType" : [ { "_name" : "Apple M2 Max", "sppci_cores" : "30", "sppci_model" : "Apple M2 Max" } ] }"#;
        assert_eq!(super::apple_chip(profile).as_deref(), Some("Apple M2 Max"));
        assert_eq!(super::apple_chip(r#"{ "sppci_model" : "AMD Radeon Pro" }"#), None);
        assert_eq!(super::apple_chip("{}"), None);
    }

    use super::*;

    #[test]
    fn recommendation_follows_yue2_vram_tiers() {
        assert_eq!(profile_for_vram(24.0), Some("native"));
        assert_eq!(profile_for_vram(11.9), Some("quality-q8"));
        assert_eq!(profile_for_vram(8.0), Some("quality-q8"));
        assert_eq!(profile_for_vram(7.6), Some("balanced"));
        assert_eq!(profile_for_vram(6.0), Some("light"));
        assert_eq!(profile_for_vram(4.0), None);
        assert_eq!(profile_for_vram(0.0), None);
    }

    #[test]
    fn every_recommendation_is_a_declared_profile() {
        for vram in [6.0, 7.5, 10.0, 16.0, 24.0] {
            assert!(crate::model_manager::profile_exists(profile_for_vram(vram).unwrap()));
        }
        assert!(crate::model_manager::profile_exists(FALLBACK_PROFILE));
    }

    #[test]
    fn the_cuda_build_follows_the_architecture_and_the_driver() {
        // GTX 1660 Super on a current driver: device code from CUDA 13.
        assert_eq!(cuda_build((7, 5), 581), Some(CudaBuild::Cuda13));
        assert_eq!(cuda_build((12, 0), 590), Some(CudaBuild::Cuda13));
        // Pascal and Maxwell, which CUDA 13 dropped, on any driver.
        assert_eq!(cuda_build((6, 1), 581), Some(CudaBuild::Cuda12));
        assert_eq!(cuda_build((5, 2), 560), Some(CudaBuild::Cuda12));
        assert_eq!(cuda_build((7, 0), 552), Some(CudaBuild::Cuda12));
        // A new card on a driver from before CUDA 13.
        assert_eq!(cuda_build((8, 9), 566), Some(CudaBuild::Cuda12));
        // Kepler, the first Maxwell and drivers older than CUDA 12: Vulkan.
        assert_eq!(cuda_build((3, 5), 581), None);
        assert_eq!(cuda_build((5, 0), 581), None);
        assert_eq!(cuda_build((8, 6), 511), None);
    }

    #[test]
    fn the_cuda_query_reads_the_capability_and_the_driver_major() {
        assert_eq!(parse_cuda_query("7.5, 581.29"), Some(((7, 5), 581)));
        assert_eq!(parse_cuda_query("12.0, 591.44"), Some(((12, 0), 591)));
        assert_eq!(parse_cuda_query("[N/A], 581.29"), None);
    }

    #[test]
    fn the_adapter_with_the_most_memory_wins_and_basic_display_never_does() {
        let names = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    DriverDesc    REG_SZ    AMD Radeon RX 7800 XT\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0001\r\n    DriverDesc    REG_SZ    Intel(R) UHD Graphics 770\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0002\r\n    DriverDesc    REG_SZ    Microsoft Basic Display Adapter\r\n";
        let sizes = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x400000000\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0001\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x80000000\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0002\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x800000000\r\n";
        let (name, vram) = best_adapter(names, sizes).unwrap();
        assert_eq!(name, "AMD Radeon RX 7800 XT");
        assert_eq!(vram, 16.0);
    }
}
