//! Where each studio tool computes: one choice per tool — Auto, the card or the processor — kept in the studio's
//! settings, and the one audio.cpp build that choice becomes for the model the tool runs.

use std::collections::BTreeMap;

use acpp_server::asset::Build;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    #[default]
    Auto,
    /// The card, whatever its make; the window's device picker names it `cuda`, as the stems' settings always have.
    #[serde(rename = "cuda")]
    Card,
    Cpu,
}

/// Every tool with a device of its own. Stems keep theirs in the separation settings (`runtime`), shown here too.
pub const TOOLS: [&str; 6] = ["stems", "midi", "restore", "effects", "foley", "voice"];

/// The choices made in the window, by tool; a tool not in it has never been set.
pub type ToolDevices = BTreeMap<String, Device>;

impl Device {
    fn choice(self) -> &'static str {
        match self {
            Device::Auto => "auto",
            Device::Card => "gpu",
            Device::Cpu => "cpu",
        }
    }

    pub fn from_runtime(runtime: crate::lyrics_sync::OnnxFlavour) -> Self {
        match runtime {
            crate::lyrics_sync::OnnxFlavour::Auto => Device::Auto,
            crate::lyrics_sync::OnnxFlavour::Cuda => Device::Card,
            crate::lyrics_sync::OnnxFlavour::Cpu => Device::Cpu,
        }
    }

    pub fn runtime(self) -> crate::lyrics_sync::OnnxFlavour {
        match self {
            Device::Auto => crate::lyrics_sync::OnnxFlavour::Auto,
            Device::Card => crate::lyrics_sync::OnnxFlavour::Cuda,
            Device::Cpu => crate::lyrics_sync::OnnxFlavour::Cpu,
        }
    }
}

/// Whether the tool's model runs on audio.cpp's Vulkan build. MuScriptor was checked note for note on CUDA only, so
/// on a card of another make it transcribes on the processor.
pub fn runs_on_vulkan(tool: &str) -> bool {
    tool != "midi"
}

/// The audio.cpp build a tool computes on for a device.
pub fn build(tool: &str, device: Device) -> anyhow::Result<Build> {
    let build = crate::acpp::build_for(device.choice())?;
    Ok(if build == Build::Vulkan && !runs_on_vulkan(tool) { Build::Cpu } else { build })
}

/// The name of a build as the window shows it.
pub fn build_name(build: Build) -> &'static str {
    match build {
        Build::Cuda13 => "CUDA 13",
        Build::Cuda12 => "CUDA 12",
        Build::Vulkan => "Vulkan",
        Build::Metal => "Metal",
        Build::Cpu => "cpu",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_and_the_runtime_it_was_kept_as_are_the_same_choice() {
        for device in [Device::Auto, Device::Card, Device::Cpu] {
            assert_eq!(Device::from_runtime(device.runtime()), device);
        }
    }

    #[test]
    fn only_the_transcriber_leaves_vulkan_for_the_processor() {
        assert!(!runs_on_vulkan("midi"));
        assert!(TOOLS.iter().filter(|tool| **tool != "midi").all(|tool| runs_on_vulkan(tool)));
    }

    #[test]
    fn the_processor_is_the_processor_for_every_tool() {
        if Build::for_this_mac().is_some() {
            return;
        }
        for tool in TOOLS {
            assert_eq!(build(tool, Device::Cpu).unwrap(), Build::Cpu);
        }
    }
}
