//! The computer's own memory the graphics driver lends a process once the card's memory is full: Windows counts it
//! per process as `\GPU Process Memory(pid_<pid>_...)\Shared Usage`. Work kept there runs many times slower.

/// More of an engine's work than this in the computer's memory is a song that crawls.
const SHARED_ALARM_BYTES: u64 = 1024 * 1024 * 1024;
/// The share of the card's memory taken from which it counts as full.
const CARD_FULL: f64 = 0.97;

/// Whether the driver lends a song's engine the computer's memory: the card's own is nearly all taken and more than
/// `SHARED_ALARM_BYTES` of the engine's work is kept outside it.
pub fn spilling(shared_bytes: u64, card_used_mb: u64, card_total_mb: u64) -> bool {
    card_total_mb > 0 && card_used_mb as f64 / card_total_mb as f64 >= CARD_FULL && shared_bytes > SHARED_ALARM_BYTES
}

/// The bytes of the computer's memory the driver lends process `pid` for its work on the card; `None` when the
/// counters cannot be read.
#[cfg(windows)]
pub fn shared_usage(pid: u32) -> Option<u64> {
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetRawCounterArrayW, PdhOpenQueryW, PDH_CSTATUS_VALID_DATA, PDH_HQUERY,
        PDH_MORE_DATA, PDH_RAW_COUNTER_ITEM_W,
    };

    struct Query(PDH_HQUERY);
    impl Drop for Query {
        fn drop(&mut self) {
            unsafe { PdhCloseQuery(self.0) };
        }
    }

    let mut handle = std::ptr::null_mut();
    if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut handle) } != 0 {
        return None;
    }
    let query = Query(handle);
    let path: Vec<u16> = "\\GPU Process Memory(*)\\Shared Usage".encode_utf16().chain(Some(0)).collect();
    let mut counter = std::ptr::null_mut();
    if unsafe { PdhAddEnglishCounterW(query.0, path.as_ptr(), 0, &mut counter) } != 0 {
        return None;
    }
    if unsafe { PdhCollectQueryData(query.0) } != 0 {
        return None;
    }
    let (mut size, mut count) = (0u32, 0u32);
    if unsafe { PdhGetRawCounterArrayW(counter, &mut size, &mut count, std::ptr::null_mut()) } != PDH_MORE_DATA {
        return None;
    }
    // u64 words keep the items' pointers and 64-bit values aligned; the names follow the items in the same buffer
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let items = buffer.as_mut_ptr().cast::<PDH_RAW_COUNTER_ITEM_W>();
    if unsafe { PdhGetRawCounterArrayW(counter, &mut size, &mut count, items) } != 0 {
        return None;
    }
    let prefix = format!("pid_{pid}_");
    let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
    let total = items
        .iter()
        .filter(|item| item.RawValue.CStatus == PDH_CSTATUS_VALID_DATA && !item.szName.is_null())
        .filter(|item| {
            let name = unsafe {
                let mut length = 0;
                while *item.szName.add(length) != 0 {
                    length += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(item.szName, length))
            };
            name.starts_with(&prefix)
        })
        .map(|item| item.RawValue.FirstValue.max(0) as u64)
        .sum();
    Some(total)
}

#[cfg(not(windows))]
pub fn shared_usage(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_card_with_the_engine_in_the_computers_memory_is_a_song_that_crawls() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert!(spilling(3 * GIB, 23_629, 24_564));
        assert!(!spilling(3 * GIB, 18_000, 24_564), "the card has room: what is lent is not the song's");
        assert!(!spilling(GIB / 2, 24_000, 24_564), "a little lent memory is the driver's usual");
        assert!(!spilling(3 * GIB, 0, 0));
    }
}
