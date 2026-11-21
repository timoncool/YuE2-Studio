//! The video memory a Vulkan card reports, for the cards whose kernel driver
//! does not publish it.
//!
//! `/sys/class/drm/card*/device/mem_info_vram_total` is AMD's own file. Intel's
//! `xe` driver keeps the size to itself, so `hardware::linux_card` read 0 GB for
//! every Intel card, and the studio went on to treat a card that works as a
//! machine that computes on the processor: it recommended the lightest set,
//! said nothing about the memory the card has, and decided every memory question
//! against a zero. The engine has always run on that card through Vulkan, so the
//! size is asked of the same API the engine uses.
//!
//! The query is the smallest one that answers: create an instance, ask the
//! physical devices for their properties and their memory heaps, keep the card
//! the kernel named, and read its device-local heaps - the memory a discrete
//! card keeps to itself. No device is created and nothing is allocated.

use std::ffi::{c_char, c_void};
use std::ptr;

type VkInstance = *mut c_void;
type VkPhysicalDevice = *mut c_void;

type CreateInstance = unsafe extern "C" fn(*const InstanceCreateInfo, *const c_void, *mut VkInstance) -> i32;
type DestroyInstance = unsafe extern "C" fn(VkInstance, *const c_void);
type EnumeratePhysicalDevices = unsafe extern "C" fn(VkInstance, *mut u32, *mut VkPhysicalDevice) -> i32;
type GetPhysicalDeviceProperties = unsafe extern "C" fn(VkPhysicalDevice, *mut c_void);
type GetPhysicalDeviceMemoryProperties = unsafe extern "C" fn(VkPhysicalDevice, *mut MemoryProperties);

const VK_SUCCESS: i32 = 0;
const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: i32 = 1;
const VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU: i32 = 2;
const VK_MEMORY_HEAP_DEVICE_LOCAL_BIT: u32 = 1;
const VK_MAX_MEMORY_TYPES: usize = 32;
const VK_MAX_MEMORY_HEAPS: usize = 16;
/// `sizeof(VkPhysicalDeviceProperties)` is 824; the buffer is larger so that no
/// driver, whatever it appends to the struct, can write past it.
const PROPERTIES_BYTES: usize = 1024;

const GIB: f64 = (1u64 << 30) as f64;

#[repr(C)]
struct InstanceCreateInfo {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    application_info: *const c_void,
    enabled_layer_count: u32,
    enabled_layer_names: *const *const c_char,
    enabled_extension_count: u32,
    enabled_extension_names: *const *const c_char,
}

/// The leading fields of `VkPhysicalDeviceProperties`: the identity of the
/// card, before the name and the limits a struct this size is not worth
/// spelling out.
#[repr(C)]
struct PropertiesHead {
    api_version: u32,
    driver_version: u32,
    vendor_id: u32,
    device_id: u32,
    device_type: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryType {
    property_flags: u32,
    heap_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryHeap {
    size: u64,
    flags: u32,
}

#[repr(C)]
struct MemoryProperties {
    memory_type_count: u32,
    memory_types: [MemoryType; VK_MAX_MEMORY_TYPES],
    memory_heap_count: u32,
    memory_heaps: [MemoryHeap; VK_MAX_MEMORY_HEAPS],
}

/// The video memory of the card with these PCI ids, in GiB: none when Vulkan is
/// not installed, no driver answers, or the card found is not a discrete one
/// (integrated graphics share the system's memory, which the kernel reports).
/// `vendor_id` and `device_id` come from sysfs, `0x8086` and `0xe20b` alike.
pub(crate) fn device_memory_gb(vendor_id: &str, device_id: &str) -> Option<f64> {
    let vendor = parse_pci_id(vendor_id)?;
    let device = parse_pci_id(device_id).unwrap_or(0);
    probe(vendor, device).map(to_gb)
}

fn parse_pci_id(text: &str) -> Option<u32> {
    let text = text.trim();
    let text = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u16::from_str_radix(text, 16).ok().map(u32::from)
}

fn to_gb(bytes: u64) -> f64 {
    bytes as f64 / GIB
}

/// The device-local heaps a card declares: its own memory, never the memory it
/// shares with the processor.
fn device_local_bytes(memory: &MemoryProperties) -> u64 {
    memory
        .memory_heaps
        .iter()
        .take(memory.memory_heap_count as usize)
        .filter(|heap| heap.flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT != 0)
        .map(|heap| heap.size)
        .sum()
}

/// One card Vulkan lists, as the kernel's PCI ids and the size of its own memory.
struct Candidate {
    vendor: u32,
    device: u32,
    bytes: u64,
}

/// The card the kernel named, by its place in the list Vulkan gave. A card that
/// is not among the devices Vulkan lists is not another card's size: the answer
/// is none, so a machine whose first DRM card is built-in graphics never borrows
/// the size of a discrete card beside it.
fn pick(candidates: &[Candidate], vendor: u32, device: u32) -> Option<usize> {
    let mut matching = candidates.iter().enumerate().filter(|(_, c)| c.vendor == vendor && c.device == device);
    let (index, first) = matching.next()?;
    matching.all(|(_, other)| other.bytes == first.bytes).then_some(index)
}

fn probe(vendor: u32, device: u32) -> Option<u64> {
    // Safety: every call below is a Vulkan entry point resolved from the
    // installed loader, given the structures the specification defines for it,
    // and the instance it creates is destroyed before this function returns.
    unsafe {
        let library = libloading::Library::new("libvulkan.so.1").ok()?;
        let create_instance = *library.get::<CreateInstance>(b"vkCreateInstance").ok()?;
        let destroy_instance = *library.get::<DestroyInstance>(b"vkDestroyInstance").ok()?;
        let enumerate = *library.get::<EnumeratePhysicalDevices>(b"vkEnumeratePhysicalDevices").ok()?;
        let properties = *library.get::<GetPhysicalDeviceProperties>(b"vkGetPhysicalDeviceProperties").ok()?;
        let memory_properties = *library.get::<GetPhysicalDeviceMemoryProperties>(b"vkGetPhysicalDeviceMemoryProperties").ok()?;

        // A null `pApplicationInfo` is allowed, and nothing here needs a name.
        let info = InstanceCreateInfo {
            s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            application_info: ptr::null(),
            enabled_layer_count: 0,
            enabled_layer_names: ptr::null(),
            enabled_extension_count: 0,
            enabled_extension_names: ptr::null(),
        };
        let mut instance: VkInstance = ptr::null_mut();
        if create_instance(&info, ptr::null(), &mut instance) != VK_SUCCESS || instance.is_null() {
            return None;
        }
        let instance = Instance { handle: instance, destroy: destroy_instance };

        let mut count = 0u32;
        if enumerate(instance.handle, &mut count, ptr::null_mut()) != VK_SUCCESS || count == 0 {
            return None;
        }
        let mut devices: Vec<VkPhysicalDevice> = vec![ptr::null_mut(); count as usize];
        if enumerate(instance.handle, &mut count, devices.as_mut_ptr()) != VK_SUCCESS {
            return None;
        }
        devices.truncate(count as usize);

        let mut candidates: Vec<Candidate> = Vec::new();
        for device_handle in devices {
            let mut buffer = [0u64; PROPERTIES_BYTES.div_ceil(8)];
            properties(device_handle, buffer.as_mut_ptr().cast());
            let head: PropertiesHead = ptr::read_unaligned(buffer.as_ptr().cast());
            // Integrated graphics report the system's memory as their own: the
            // kernel's own figures are the ones to use for them.
            if head.device_type != VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU {
                continue;
            }
            let mut memory: MemoryProperties = std::mem::zeroed();
            memory_properties(device_handle, &mut memory);
            let bytes = device_local_bytes(&memory);
            if bytes > 0 {
                candidates.push(Candidate { vendor: head.vendor_id, device: head.device_id, bytes });
            }
        }
        pick(&candidates, vendor, device).map(|index| candidates[index].bytes)
    }
}

/// The instance a query made, destroyed however the query ends.
struct Instance {
    handle: VkInstance,
    destroy: DestroyInstance,
}

impl Drop for Instance {
    fn drop(&mut self) {
        // Safety: the handle was created by `vkCreateInstance` and is destroyed
        // once, here.
        unsafe { (self.destroy)(self.handle, ptr::null()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(heaps: &[(u64, u32)]) -> MemoryProperties {
        let mut properties: MemoryProperties = unsafe { std::mem::zeroed() };
        properties.memory_heap_count = heaps.len() as u32;
        for (heap, (size, flags)) in properties.memory_heaps.iter_mut().zip(heaps) {
            heap.size = *size;
            heap.flags = *flags;
        }
        properties
    }

    #[test]
    fn the_cards_own_heaps_are_its_video_memory() {
        let card = memory(&[(12 * (1u64 << 30), VK_MEMORY_HEAP_DEVICE_LOCAL_BIT), (32 * (1u64 << 30), 0)]);
        assert_eq!(device_local_bytes(&card), 12 * (1u64 << 30), "the host heap is not the card's");
        assert!((to_gb(device_local_bytes(&card)) - 12.0).abs() < 1e-9);
    }

    #[test]
    fn heaps_past_the_count_the_driver_declares_are_ignored() {
        let card = memory(&[(8 * (1u64 << 30), VK_MEMORY_HEAP_DEVICE_LOCAL_BIT), (12 * (1u64 << 30), VK_MEMORY_HEAP_DEVICE_LOCAL_BIT)]);
        let mut only_one = card;
        only_one.memory_heap_count = 1;
        assert_eq!(device_local_bytes(&only_one), 8 * (1u64 << 30));
    }

    #[test]
    fn the_card_the_kernel_names_is_the_one_read_and_no_other() {
        let candidates = [
            Candidate { vendor: 0x1002, device: 0x73bf, bytes: 16 << 30 },
            Candidate { vendor: 0x8086, device: 0xe20b, bytes: 12 << 30 },
        ];
        assert_eq!(pick(&candidates, 0x8086, 0xe20b), Some(1));
        assert_eq!(pick(&candidates, 0x1002, 0x73bf), Some(0));
        assert_eq!(pick(&candidates, 0x8086, 0x1234), None, "a card Vulkan does not list is not another card's memory");
    }

    #[test]
    fn pci_ids_parse_with_and_without_the_hex_prefix() {
        assert_eq!(parse_pci_id("0x8086"), Some(0x8086));
        assert_eq!(parse_pci_id("8086"), Some(0x8086));
        assert_eq!(parse_pci_id("0xe20b"), Some(0xe20b));
        assert_eq!(parse_pci_id(""), None);
        assert_eq!(parse_pci_id("not a number"), None);
    }

    #[test]
    fn a_video_memory_size_is_counted_in_gib_as_the_rest_of_the_studio_counts_it() {
        assert!((to_gb(12_884_901_888) - 12.0).abs() < 1e-9);
        assert!((to_gb(0) - 0.0).abs() < 1e-9);
    }
}
