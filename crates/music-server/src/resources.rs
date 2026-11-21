//! Live machine resources for the studio's resource monitor.
//!
//! ACE Studio had a resource readout and it was worth keeping: local music
//! generation is the most VRAM-hungry thing on the machine, and a user who
//! cannot see VRAM pressure has no way to tell "still loading the DiT" from
//! "about to run out of memory". Every number here is measured — nothing is
//! estimated, and a metric that cannot be read is reported as absent rather
//! than as zero.

use std::{sync::Mutex, time::Duration};

use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

#[derive(Debug, Clone, Serialize)]
pub struct GpuSnapshot {
    pub name: String,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    /// Percent of the sampling period during which the GPU was busy.
    pub utilization_percent: Option<u8>,
    pub temperature_c: Option<u8>,
    pub power_draw_w: Option<f32>,
    pub power_limit_w: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessSnapshot {
    pub name: String,
    pub memory_mb: u64,
    pub cpu_percent: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceSnapshot {
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpus: Vec<GpuSnapshot>,
    /// The native engine process, when it is running on this machine.
    pub engine_process: Option<ProcessSnapshot>,
    /// Resident memory of the studio server itself.
    pub studio_process_mb: u64,
}

/// `sysinfo` reports CPU load as the delta between two refreshes, so the
/// sampler has to be kept alive between requests. A first call therefore
/// reports 0% CPU; every later call reports the load since the previous one.
static SAMPLER: Mutex<Option<System>> = Mutex::new(None);

pub fn snapshot() -> ResourceSnapshot {
    let mut guard = SAMPLER.lock().unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
        // The very first CPU reading has no previous sample to diff against and
        // would report a meaningless 100%. Prime it before the caller sees it.
        let mut primed = System::new().expect("sysinfo has Windows, Linux and macOS");
        primed.refresh_cpu_usage();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        *guard = Some(primed);
    }
    let system = guard.as_mut().expect("sampler was primed above");
    system.refresh_memory();
    system.refresh_cpu_usage();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let engine_process = system
        .processes()
        .values()
        .find(|process| {
            let name = process.name().to_string_lossy().to_ascii_lowercase();
            name == "yue-server" || name == "yue-server.exe"
        })
        .map(|process| ProcessSnapshot {
            name: process.name().to_string_lossy().into_owned(),
            memory_mb: process.memory() / 1_048_576,
            cpu_percent: process.cpu_usage(),
        });

    let studio_process_mb = sysinfo::get_current_pid()
        .ok()
        .and_then(|pid| system.process(pid))
        .map(|process| process.memory() / 1_048_576)
        .unwrap_or_default();

    ResourceSnapshot {
        cpu_percent: system.global_cpu_usage(),
        ram_used_mb: system.used_memory() / 1_048_576,
        ram_total_mb: system.total_memory() / 1_048_576,
        gpus: machine_gpus(system.used_memory() / 1_048_576, system.total_memory() / 1_048_576),
        engine_process,
        studio_process_mb,
    }
}

/// The card's video memory in use, as the kernel counts it per DRM client:
/// `drm-resident-vramN` less `drm-shared-vramN`, once per client and summed over
/// the clients - the figure `nvtop` and the other monitors show. A process has
/// several file descriptors open on the card, and every one of them reports the
/// same client's totals: counted per descriptor, a desktop shell alone looked
/// like 2.8 GiB. A buffer two clients hold is counted by both as shared, so
/// subtracting shared keeps it from being counted twice.
///
/// `None` when nothing under /proc could be read (no /proc, or hidepid): the
/// monitor then shows no figure for this card rather than an invented one.
#[cfg(not(any(windows, target_os = "macos")))]
fn drm_usage() -> Option<std::collections::HashMap<String, u64>> {
    type Sample = (std::time::Instant, Option<std::collections::HashMap<String, u64>>);
    static LAST: Mutex<Option<Sample>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((at, usage)) = &*last && at.elapsed() < Duration::from_secs(2) {
        return usage.clone();
    }
    let usage = scan_drm_usage();
    *last = Some((std::time::Instant::now(), usage.clone()));
    usage
}

#[cfg(not(any(windows, target_os = "macos")))]
fn scan_drm_usage() -> Option<std::collections::HashMap<String, u64>> {
    use std::collections::HashMap;

    // (card, client id) -> (resident, shared): the same client is written by
    // every descriptor its process holds, and each writes the same totals.
    let mut clients: HashMap<(String, String), (u64, u64)> = HashMap::new();
    for process in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = process.file_name();
        let name = name.to_string_lossy();
        if !name.chars().all(|character| character.is_ascii_digit()) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(process.path().join("fdinfo")) else { continue };
        for file in files.flatten() {
            let Ok(text) = std::fs::read_to_string(file.path()) else { continue };
            if !text.contains("drm-client-id") {
                continue;
            }
            if let Some((pdev, client, resident, shared)) = drm_client_of_fdinfo(&text)
                && !pdev.is_empty()
            {
                clients.insert((pdev, client), (resident, shared));
            }
        }
    }
    Some(vram_of_clients(&clients))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn vram_of_clients(clients: &std::collections::HashMap<(String, String), (u64, u64)>) -> std::collections::HashMap<String,u64> {
    let mut usage = std::collections::HashMap::new();
    for ((card, _), (resident, shared)) in clients {
        *usage.entry(card.clone()).or_default() += resident.saturating_sub(*shared);
    }
    usage
}
/// One `fdinfo` block as the kernel writes it for a DRM client: the card it is
/// on, the client id, and its resident and shared video memory in bytes
/// (`drm-resident-vram0: 1234 KiB`). Every region a card keeps is counted.
#[cfg(not(any(windows, target_os = "macos")))]
fn drm_client_of_fdinfo(text: &str) -> Option<(String, String, u64, u64)> {
    let mut card = String::new();
    let mut client = None;
    let mut resident = None;
    let mut shared = 0u64;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match key {
            "drm-pdev" => card = value.to_owned(),
            "drm-client-id" => client = Some(value.to_owned()),
            _ if key.starts_with("drm-resident-vram") => {
                if let Some(kib) = value.strip_suffix("KiB").and_then(|value| value.trim().parse::<u64>().ok()) {
                    *resident.get_or_insert(0u64) += kib * 1024;
                }
            }
            _ if key.starts_with("drm-shared-vram") => {
                if let Some(kib) = value.strip_suffix("KiB").and_then(|value| value.trim().parse::<u64>().ok()) {
                    shared += kib * 1024;
                }
            }
            _ => {}
        }
    }
    Some((card, client?, resident?, shared))
}

/// The card the engine computes on when nvidia-smi does not know it: a Vulkan
/// card on Linux, with the memory the kernel reports in use. `None` when there
/// is no such card, or when the kernel publishes nothing for it: a number is
/// never invented for the gauge.
fn machine_gpus(used: u64, total: u64) -> Vec<GpuSnapshot> {
    let nvidia = nvidia_gpus();
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let has_nvidia_readings = nvidia.as_ref().is_some_and(|cards| !cards.is_empty());
        let mut gpus = nvidia.unwrap_or_default();
        let cards = crate::hardware::linux_cards();
        let usage = std::cell::OnceCell::new();
        for card in cards {
            if card.vram_gb <= 0.0 || (card.nvidia && has_nvidia_readings) { continue; }
            let Some(pci) = card.pci.as_ref() else { continue };
            let bytes = std::fs::read_to_string(&card.used_path).ok().and_then(|s| s.trim().parse::<u64>().ok())
                .or_else(|| usage.get_or_init(drm_usage).as_ref()?.get(pci).copied());
            let Some(bytes) = bytes else { continue };
            gpus.push(GpuSnapshot {
                name: if cards.len() > 1 { format!("{} ({pci})", card.name) } else { card.name.clone() },
                vram_used_mb: bytes / 1_048_576, vram_total_mb: (card.vram_gb * 1024.0).round() as u64,
                utilization_percent: None, temperature_c: None, power_draw_w: None, power_limit_w: None,
            });
        }
        if !gpus.is_empty() { return gpus; }
    }
    #[cfg(any(windows, target_os = "macos"))]
    if let Some(gpus) = nvidia { return gpus; }
    unified_memory_gpu(used, total)
}
/// Apple Silicon has one GPU that shares the machine's memory, so its memory
/// figures are the system's; it reports no load, temperature or power.
fn unified_memory_gpu(used_mb: u64, total_mb: u64) -> Vec<GpuSnapshot> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    crate::hardware::hardware()
        .gpu_name
        .map(|name| GpuSnapshot { name, vram_used_mb: used_mb, vram_total_mb: total_mb, utilization_percent: None, temperature_c: None, power_draw_w: None, power_limit_w: None })
        .into_iter()
        .collect()
}

pub(crate) fn nvidia_gpus() -> Option<Vec<GpuSnapshot>> {
    // the card the studio runs on, when one was chosen: that is the one to watch
    let output = dub_core::proc::host_command("nvidia-smi")
        .args(crate::hardware::chosen_card())
        .args([
            "--query-gpu=name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw,power.limit",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_gpu_line)
            .collect(),
    )
}

fn parse_gpu_line(line: &str) -> Option<GpuSnapshot> {
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() < 3 || fields[0].is_empty() {
        return None;
    }
    Some(GpuSnapshot {
        name: fields[0].to_owned(),
        vram_used_mb: fields[1].parse().ok()?,
        vram_total_mb: fields[2].parse().ok()?,
        // `[N/A]` appears on cards that do not report these counters.
        utilization_percent: fields.get(3).and_then(|value| value.parse().ok()),
        temperature_c: fields.get(4).and_then(|value| value.parse().ok()),
        power_draw_w: fields.get(5).and_then(|value| value.parse().ok()),
        power_limit_w: fields.get(6).and_then(|value| value.parse().ok()),
    })
}

/// Sampling interval the UI should poll at. One second matches the resolution
/// `nvidia-smi` itself reports and keeps the process list cheap.
pub const SUGGESTED_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_nvidia_smi_row() {
        let gpu = parse_gpu_line("NVIDIA GeForce RTX 4090, 8244, 24564, 37, 51, 142.35, 450.00").unwrap();
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 4090");
        assert_eq!(gpu.vram_used_mb, 8244);
        assert_eq!(gpu.vram_total_mb, 24564);
        assert_eq!(gpu.utilization_percent, Some(37));
        assert_eq!(gpu.temperature_c, Some(51));
        assert_eq!(gpu.power_draw_w, Some(142.35));
        assert_eq!(gpu.power_limit_w, Some(450.0));
    }

    /// Unsupported counters must be absent, never silently reported as zero.
    #[test]
    fn missing_counters_are_absent_rather_than_zero() {
        let gpu = parse_gpu_line("NVIDIA T400, 100, 2048, [N/A], [N/A], [N/A], [N/A]").unwrap();
        assert_eq!(gpu.vram_total_mb, 2048);
        assert_eq!(gpu.utilization_percent, None);
        assert_eq!(gpu.temperature_c, None);
        assert_eq!(gpu.power_draw_w, None);
    }

    #[test]
    fn rejects_truncated_rows() {
        assert!(parse_gpu_line("").is_none());
        assert!(parse_gpu_line("NVIDIA, 100").is_none());
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn the_card_memory_in_fdinfo_is_resident_less_shared_over_every_region() {
        let block = "pos:\t0\nflags:\t02100002\nmnt_id:\t1240\nino:\t123\ndrm-driver:\txe\ndrm-pdev:\t0000:04:00.0\ndrm-client-id:\t7\ndrm-total-vram0:\t8388608 KiB\ndrm-shared-vram0:\t2097152 KiB\ndrm-resident-vram0:\t6291456 KiB\ndrm-active-vram0:\t1024 KiB\ndrm-total-vram1:\t4096 KiB\ndrm-shared-vram1:\t0 KiB\ndrm-resident-vram1:\t2048 KiB\n";
        let (card, client, resident, shared) = super::drm_client_of_fdinfo(block).unwrap();
        assert_eq!(card, "0000:04:00.0");
        assert_eq!(client, "7");
        assert_eq!(resident, (6_291_456 + 2048) * 1024, "every region is the card's");
        assert_eq!(shared, 2_097_152 * 1024);
        assert_eq!(super::drm_client_of_fdinfo("pos:\t0\n"), None, "a block that is not a card's says nothing");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn missing_residency_is_unknown_and_measured_zero_is_valid() {
        let base = "drm-pdev: 0000:04:00.0\ndrm-client-id: 30\n";
        assert!(super::drm_client_of_fdinfo(base).is_none());
        assert_eq!(super::drm_client_of_fdinfo(&format!("{base}drm-resident-vram0: 0 KiB")).unwrap().2, 0);
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn a_client_with_three_descriptors_is_counted_once() {
        // Every descriptor a process holds writes the same client's totals: the
        // desktop shell alone looked like 2.8 GiB counted per descriptor.
        let mut clients = std::collections::HashMap::new();
        let card = "0000:04:00.0".to_string();
        for _ in 0..3 {
            clients.insert((card.clone(), "30".to_string()), (2_000u64, 500u64));
        }
        clients.insert((card.clone(), "70".to_string()), (1_000, 0));
        clients.insert(("0000:00:02.0".into(), "30".into()), (3_000, 200));
        let totals = super::vram_of_clients(&clients);
        assert_eq!(totals[&card], 2_500);
        assert_eq!(totals["0000:00:02.0"], 2_800);
    }

    #[test]
    fn a_snapshot_reports_real_totals() {
        let snapshot = snapshot();
        assert!(snapshot.ram_total_mb > 0);
        assert!(snapshot.ram_used_mb <= snapshot.ram_total_mb);
    }
}
