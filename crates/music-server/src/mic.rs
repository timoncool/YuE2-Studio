//! Recording from the microphone, Speech Studio's recorder (cpal + hound, a mono 16-bit WAV): the person records a voice
//! sample, a sung melody or a take, and the recording becomes a library track every tool takes. The microphone is the
//! person's: the routes refuse the agent.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

struct RecState {
    stop_tx: Sender<()>,
    path: std::path::PathBuf,
    since: std::time::Instant,
}

fn recorder() -> &'static Mutex<Option<RecState>> {
    static R: OnceLock<Mutex<Option<RecState>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}

fn rate_cell() -> &'static AtomicU32 {
    static R: OnceLock<AtomicU32> = OnceLock::new();
    R.get_or_init(|| AtomicU32::new(0))
}

fn peak_cell() -> &'static AtomicU32 {
    static P: OnceLock<AtomicU32> = OnceLock::new();
    P.get_or_init(|| AtomicU32::new(0))
}

/// The loudest sound of the last twentieth of a second, 0..1, for the level meter.
pub fn level() -> f32 {
    f32::from_bits(peak_cell().load(Ordering::Relaxed))
}

fn device_name(d: &cpal::Device) -> Option<String> {
    use cpal::traits::DeviceTrait;
    d.description().ok().map(|desc| desc.name().to_string())
}

/// The microphones, the system's default first.
pub fn input_devices() -> Vec<String> {
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    let def = host.default_input_device().and_then(|d| device_name(&d));
    let mut names = Vec::new();
    if let Some(n) = &def {
        names.push(n.clone());
    }
    if let Ok(devs) = host.input_devices() {
        for d in devs {
            if let Some(n) = device_name(&d) {
                if Some(&n) != def.as_ref() {
                    names.push(n);
                }
            }
        }
    }
    names
}

/// How long the microphone has been recording, while it records.
pub fn in_use() -> Option<std::time::Duration> {
    recorder().lock().ok()?.as_ref().map(|st| st.since.elapsed())
}

/// Starts recording into a WAV at `path`; `device` is a microphone's name, none for the system's.
pub fn start(path: &std::path::Path, device: Option<String>) -> Result<(), String> {
    let mut guard = recorder().lock().map_err(|_| "the recorder is poisoned".to_string())?;
    if guard.is_some() {
        return Err("The microphone is already recording.".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let path_buf = path.to_path_buf();
    let thread_path = path_buf.clone();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    if let Ok(file) = std::env::var("YUE2_STUDIO_MIC_FILE") {
        // a file in the microphone's place, for tests on a machine without one
        let stereo = crate::audio_pcm::decode_stereo(std::path::Path::new(&file)).map_err(|e| format!("{file}: {e:#}"))?;
        let sr = stereo.rate;
        let samples: Vec<f32> = stereo.left.iter().zip(&stereo.right).map(|(l, r)| (l + r) / 2.0).collect();
        let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::create(&thread_path, spec).map_err(|e| format!("{}: {e}", thread_path.display()))?;
        rate_cell().store(sr, Ordering::Relaxed);
        std::thread::spawn(move || {
            let step = (sr / 10).max(1) as usize;
            let silence = vec![0.0f32; step];
            let mut at = 0;
            while stop_rx.try_recv().is_err() {
                let chunk = if at < samples.len() { &samples[at..(at + step).min(samples.len())] } else { &silence[..] };
                at += step;
                for &v in chunk {
                    let _ = writer.write_sample((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                }
                peak_cell().store(chunk.iter().fold(0f32, |m, v| m.max(v.abs())).min(1.0).to_bits(), Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let _ = writer.finalize();
        });
        *guard = Some(RecState { stop_tx, path: path_buf, since: std::time::Instant::now() });
        return Ok(());
    }

    std::thread::spawn(move || {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let wanted = device;
        let device = match &wanted {
            Some(name) => host.input_devices().ok().and_then(|mut it| it.find(|d| device_name(d).is_some_and(|n| &n == name))),
            None => host.default_input_device(),
        };
        let Some(device) = device else {
            let missing = match &wanted {
                Some(name) => format!("The microphone {name} is not connected."),
                None => "No microphone was found on this computer.".to_string(),
            };
            let _ = ready_tx.send(Err(missing));
            return;
        };
        let supported = match device.default_input_config() {
            Ok(c) => c,
            Err(e) => { let _ = ready_tx.send(Err(format!("The microphone's settings could not be read: {e}"))); return; }
        };
        let fmt = supported.sample_format();
        let ch = (supported.channels() as usize).max(1);
        let sr = supported.sample_rate();
        let config: cpal::StreamConfig = supported.into();
        let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let writer = match hound::WavWriter::create(&thread_path, spec) {
            Ok(w) => Arc::new(Mutex::new(Some(w))),
            Err(e) => { let _ = ready_tx.send(Err(format!("{}: {e}", thread_path.display()))); return; }
        };
        let emit_every = (sr / 20).max(1) as usize;
        rate_cell().store(sr, Ordering::Relaxed);

        macro_rules! cb {
            ($ty:ty, $to_f:expr) => {{
                let w = writer.clone();
                let mut peak = 0f32;
                let mut frames = 0usize;
                move |data: &[$ty], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut g) = w.lock() {
                        if let Some(wr) = g.as_mut() {
                            for frame in data.chunks(ch) {
                                let mono = frame.iter().map(|&s| $to_f(s)).sum::<f32>() / ch as f32;
                                peak = peak.max(mono.abs());
                                let _ = wr.write_sample((mono.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                            }
                        }
                    }
                    frames += data.len() / ch;
                    if frames >= emit_every {
                        peak_cell().store(peak.min(1.0).to_bits(), Ordering::Relaxed);
                        peak = 0.0;
                        frames = 0;
                    }
                }
            }};
        }
        let err_fn = |e| eprintln!("[ERROR] microphone stream: {e}");
        let built = match fmt {
            cpal::SampleFormat::F32 => device.build_input_stream(config, cb!(f32, |s: f32| s), err_fn, None),
            cpal::SampleFormat::I16 => device.build_input_stream(config, cb!(i16, |s: i16| s as f32 / i16::MAX as f32), err_fn, None),
            cpal::SampleFormat::U16 => device.build_input_stream(config, cb!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0), err_fn, None),
            other => { let _ = ready_tx.send(Err(format!("The microphone's sample format {other:?} is not supported."))); return; }
        };
        let stream = match built {
            Ok(s) => s,
            Err(e) => { let _ = ready_tx.send(Err(format!("The microphone could not be opened: {e}"))); return; }
        };
        if let Err(e) = stream.play() {
            let _ = ready_tx.send(Err(format!("The microphone did not start: {e}")));
            return;
        }
        let _ = ready_tx.send(Ok(()));
        let _ = stop_rx.recv();
        drop(stream);
        if let Some(w) = writer.lock().ok().and_then(|mut g| g.take()) {
            let _ = w.finalize();
        }
    });

    match ready_rx.recv().map_err(|e| e.to_string())? {
        Ok(()) => { *guard = Some(RecState { stop_tx, path: path_buf, since: std::time::Instant::now() }); Ok(()) }
        Err(e) => Err(e),
    }
}

/// Stops recording: the finished WAV.
pub fn stop() -> Result<std::path::PathBuf, String> {
    let mut guard = recorder().lock().map_err(|_| "the recorder is poisoned".to_string())?;
    let st = guard.take().ok_or_else(|| "The microphone is not recording.".to_string())?;
    let _ = st.stop_tx.send(());
    std::thread::sleep(std::time::Duration::from_millis(200));
    peak_cell().store(0, Ordering::Relaxed);
    Ok(st.path)
}

/// Below −60 dBFS over the whole take: a microphone the system keeps closed or a muted one writes zeros, not an error.
pub const SILENT_PEAK: f32 = 0.001;

/// The loudest sample of a recorded WAV, 0..1, and its length in seconds.
pub fn measure(path: &std::path::Path) -> Result<(f32, f64), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = r.spec();
    let (mut peak, mut count) = (0f32, 0u64);
    for sample in r.samples::<i16>().filter_map(Result::ok) {
        peak = peak.max((sample as f32 / i16::MAX as f32).abs());
        count += 1;
    }
    Ok((peak, count as f64 / (f64::from(spec.sample_rate) * f64::from(spec.channels.max(1)))))
}
