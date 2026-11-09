//! The dual-mono writers hold a piece of the track, not the track: the memory
//! they take beyond the track itself does not grow with its length.
//! `AUDIO_POST_SCRATCH` - a folder for the files written.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The bytes taken above what was held when `run` started, at its highest.
fn extra(run: impl FnOnce()) -> usize {
    let base = NOW.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    run();
    PEAK.load(Ordering::Relaxed) - base
}

#[test]
#[ignore]
fn the_memory_of_a_dual_mono_file_does_not_grow_with_the_track() {
    let dir = std::path::PathBuf::from(std::env::var("AUDIO_POST_SCRATCH").expect("AUDIO_POST_SCRATCH"));
    let mut taken = Vec::new();
    for seconds in [30usize, 300] {
        let x: Vec<f32> = (0..seconds * 24_000).map(|i| 0.4 * (i as f32 * 0.05).sin()).collect();
        let mp3 = extra(|| {
            let mut file = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{seconds}.mp3"))).unwrap());
            audio_post::encode::mp3_dual_mono(&x, 24_000, 48_000, 192, &mut file).unwrap();
        });
        let flac = extra(|| {
            let mut file = std::fs::File::options().read(true).write(true).create(true).truncate(true).open(dir.join(format!("{seconds}.flac"))).unwrap();
            audio_post::encode::flac_dual_mono(&x, 24_000, 24_000, &mut file).unwrap();
        });
        eprintln!("{seconds} s ({} MB of track): MP3 took {} KB more, FLAC {} KB more", x.len() * 4 / 1_000_000, mp3 / 1000, flac / 1000);
        taken.push((mp3, flac));
    }
    let (short, long) = (taken[0], taken[1]);
    assert!(long.0 < short.0 * 2 && long.0 < 8_000_000, "MP3: {short:?} -> {long:?}");
    assert!(long.1 < short.1 * 2 && long.1 < 8_000_000, "FLAC: {short:?} -> {long:?}");
}
