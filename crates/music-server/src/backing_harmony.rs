//! Relative pitch-class energy over time, followed by conservative triad
//! matching. Silence and ambiguous frames are gaps in the result.
use realfft::RealFftPlanner;
use serde::Serialize;

const RATE: usize = 22_050;
const FFT: usize = 8192;
const HOP: usize = FFT / 2;
const ROOTS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
type Chroma = [f64; 12];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Triad {
    root: usize,
    minor: bool,
}

impl Triad {
    fn label(self) -> String {
        format!("{}{}", ROOTS[self.root], if self.minor { "m" } else { "" })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct Chord {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub label: String,
}
#[derive(Debug, Serialize)]
pub(super) struct Section {
    start_seconds: f64,
    end_seconds: f64,
    label: String,
}
#[derive(Debug, Serialize)]
pub(super) struct Analysis {
    pub key: Option<String>,
    pub chords: Vec<Chord>,
    pub sections: Vec<Section>,
}

fn triad(chroma: Chroma) -> Option<Triad> {
    let total = chroma.iter().sum::<f64>();
    if total <= 0.0 {
        return None;
    }
    let mut best = (0.0, None);
    let mut second: f64 = 0.0;
    for root in 0..12 {
        for minor in [false, true] {
            let third = chroma[(root + if minor { 3 } else { 4 }) % 12];
            let fifth = chroma[(root + 7) % 12];
            let power = [chroma[root], third, fifth];
            if power.iter().copied().fold(f64::INFINITY, f64::min) < 0.11 * total {
                continue;
            }
            let coverage = power.iter().sum::<f64>() / total;
            if coverage < 0.68 {
                continue;
            }
            if coverage > best.0 {
                second = best.0;
                best = (coverage, Some(Triad { root, minor }));
            } else {
                second = second.max(coverage);
            }
        }
    }
    (best.0 - second >= 0.08).then_some(best.1).flatten()
}

fn pitch_energy(magnitudes: &[f64]) -> Chroma {
    let mut chroma = [0.0; 12];
    let peak = magnitudes.iter().copied().fold(0.0, f64::max);
    let hz_per_bin = RATE as f64 / FFT as f64;
    for bin in 18..=560 {
        let [left, power, right] = magnitudes[bin - 1..=bin + 1] else {
            unreachable!()
        };
        if power <= 0.04 * peak || power <= left || power < right {
            continue;
        }
        // Quadratic peak interpolation is less sensitive to a note falling
        // between two FFT bins. Reject peaks far from a tempered pitch.
        let offset = 0.5 * (left - right) / (left - 2.0 * power + right);
        let note = 69.0 + 12.0 * (((bin as f64 + offset) * hz_per_bin) / 440.0).log2();
        if (note - note.round()).abs() > 0.38 {
            continue;
        }
        let pitch = (note.round() as i32).rem_euclid(12) as usize;
        chroma[pitch] += power / (1.0 + (bin as f64 * hz_per_bin / 440.0).powi(2));
    }
    chroma
}

fn key(chroma: Chroma, roots: [bool; 12], held_seconds: [f64; 12]) -> Option<String> {
    if roots.into_iter().filter(|&present| present).count() < 3 {
        return None;
    }
    const MAJOR: Chroma = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    const MINOR: Chroma = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let centered = |profile: Chroma| {
        let mean = profile.iter().sum::<f64>() / 12.0;
        profile.map(|value| value - mean)
    };
    let norm = |profile: Chroma| {
        profile
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt()
    };
    let input = centered(chroma);
    let input_norm = norm(input);
    if input_norm <= f64::EPSILON {
        return None;
    }
    let mut guesses = Vec::with_capacity(24);
    for (minor, profile) in [(false, MAJOR), (true, MINOR)] {
        let pattern = centered(profile);
        let normalization = input_norm * norm(pattern);
        for root in 0..12 {
            let dot = (0..12)
                .map(|pc| input[(pc + root) % 12] * pattern[pc])
                .sum::<f64>();
            guesses.push((dot / normalization, Triad { root, minor }));
        }
    }
    guesses.sort_by(|a, b| b.0.total_cmp(&a.0));
    let [(best, winner), (runner_up, _), ..] = guesses.as_slice() else {
        unreachable!()
    };
    const MINOR_KEYS: [&str; 12] = [
        "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm",
    ];
    let tonic_duration = held_seconds[winner.root];
    let competing_duration = held_seconds
        .iter()
        .enumerate()
        .filter(|&(root, _)| root != winner.root)
        .map(|(_, &seconds)| seconds)
        .fold(0.0, f64::max);
    // The key profile and the longest stable chord root must agree.
    (*best >= 0.68 && best - runner_up >= 0.075 && tonic_duration > competing_duration).then(|| {
        if winner.minor {
            MINOR_KEYS[winner.root].to_owned()
        } else {
            winner.label()
        }
    })
}

pub(super) fn analyze(mono: &[f32]) -> anyhow::Result<Analysis> {
    anyhow::ensure!(
        mono.len() >= RATE * 3,
        "The backing is too short to analyze."
    );
    anyhow::ensure!(
        mono.len() <= RATE * 600,
        "Choose a backing up to ten minutes long."
    );
    let mut plan = RealFftPlanner::<f64>::new();
    let fft = plan.plan_fft_forward(FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let mut magnitudes = vec![0.0; spectrum.len()];
    let window: Vec<_> = (0..FFT)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FFT as f64).cos())
        .collect();
    let mut frames = Vec::new();
    let mut aggregate = [0.0; 12];
    let mut roots = [false; 12];
    for start in (0..=mono.len() - FFT).step_by(HOP) {
        let samples = &mono[start..start + FFT];
        let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>() / FFT as f32).sqrt();
        if !rms.is_finite() || rms < 0.002 {
            frames.push(None);
            continue;
        }
        for ((target, &sample), taper) in input.iter_mut().zip(samples).zip(&window) {
            *target = f64::from(sample) * taper;
        }
        fft.process_with_scratch(&mut input, &mut spectrum, &mut scratch)?;
        for (magnitude, bin) in magnitudes.iter_mut().zip(&spectrum) {
            *magnitude = bin.norm();
        }
        let chroma = pitch_energy(&magnitudes);
        let guess = triad(chroma);
        if let Some(chord) = guess {
            roots[chord.root] = true;
            let total = chroma.iter().sum::<f64>();
            for (acc, energy) in aggregate.iter_mut().zip(chroma) {
                *acc += energy / total;
            }
        }
        frames.push(guess);
    }
    let mut chords = Vec::new();
    let mut held_seconds = [0.0; 12];
    let mut index = 0;
    let boundary = |frame| (frame * HOP + HOP / 2) as f64 / RATE as f64;
    while index < frames.len() {
        let next = index
            + frames[index..]
                .iter()
                .take_while(|&&frame| frame == frames[index])
                .count();
        if let Some(chord) = frames[index].filter(|_| next - index >= 3) {
            let span = Chord {
                start_seconds: if index == 0 { 0.0 } else { boundary(index) },
                end_seconds: if next == frames.len() {
                    mono.len() as f64 / RATE as f64
                } else {
                    boundary(next)
                },
                label: chord.label(),
            };
            held_seconds[chord.root] += span.end_seconds - span.start_seconds;
            chords.push(span);
        }
        index = next;
    }
    let marked = chords
        .iter()
        .map(|span| span.end_seconds - span.start_seconds)
        .sum::<f64>();
    anyhow::ensure!(
        marked >= 2.0 && marked >= 0.35 * mono.len() as f64 / RATE as f64,
        "Too little stable harmony was confirmed. Keep the score or enter chords manually."
    );
    let sections = repeated_sections(&chords);
    Ok(Analysis {
        key: key(aggregate, roots, held_seconds),
        chords,
        sections,
    })
}

/// Call a boundary only when two distinct consecutive motifs each repeat.
/// Simple chord loops and one-off changes carry no evidence of a boundary.
fn repeated_sections(chords: &[Chord]) -> Vec<Section> {
    if chords
        .windows(2)
        .any(|pair| pair[1].start_seconds - pair[0].end_seconds > 0.5)
    {
        return Vec::new();
    }
    for bars in [4, 3] {
        let labels = |from| {
            chords[from..from + bars]
                .iter()
                .map(|chord| chord.label.as_str())
        };
        for from in 0..=chords.len().saturating_sub(4 * bars) {
            if from + 4 * bars > chords.len() {
                break;
            }
            let middle = from + 2 * bars;
            if labels(from).eq(labels(from + bars))
                && labels(middle).eq(labels(middle + bars))
                && labels(from)
                    .zip(labels(middle))
                    .filter(|(a, b)| a != b)
                    .count()
                    >= 2
            {
                return [(from, middle, "A"), (middle, from + 4 * bars, "B")]
                    .into_iter()
                    .map(|(start, end, label)| Section {
                        start_seconds: chords[start].start_seconds,
                        end_seconds: chords[end - 1].end_seconds,
                        label: label.to_owned(),
                    })
                    .collect();
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_the_chords_and_repeated_sections_of_a_known_backing() {
        let a = [[48, 52, 55], [55, 59, 62], [57, 60, 64], [53, 57, 60]];
        let b = [[50, 53, 57], [55, 59, 62], [48, 52, 55], [52, 55, 59]];
        let audio = [a, a, b, b]
            .into_iter()
            .flatten()
            .flat_map(|notes| {
                (0..2 * RATE).map(move |sample| {
                    let time = sample as f64 / RATE as f64;
                    notes
                        .into_iter()
                        .map(|midi| {
                            let hz = 440.0 * 2.0_f64.powf(f64::from(midi - 69) / 12.0);
                            (std::f64::consts::TAU * hz * time).sin() as f32 * 0.12
                        })
                        .sum::<f32>()
                })
            })
            .collect::<Vec<_>>();
        let result = analyze(&audio).unwrap();
        let names = result
            .chords
            .iter()
            .map(|chord| chord.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "C", "G", "Am", "F", "C", "G", "Am", "F", "Dm", "G", "C", "Em", "Dm", "G", "C",
                "Em"
            ]
        );
        assert_eq!(result.sections.len(), 2);
        assert!((result.sections[1].start_seconds - 16.0).abs() <= 0.4);
    }

    #[test]
    fn requires_a_distinct_key_profile_and_multiple_chord_roots() {
        let c_major = [8.0, 0.0, 2.0, 0.0, 5.0, 3.0, 0.0, 6.0, 0.0, 2.0, 0.0, 1.0];
        let mut roots = [false; 12];
        roots[0] = true;
        assert!(key(c_major, roots, c_major).is_none());
        roots[5] = true;
        roots[7] = true;
        assert_eq!(key(c_major, roots, c_major).as_deref(), Some("C"));
        assert!(key([1.0; 12], roots, c_major).is_none());
        let mut conflicting_roots = c_major;
        conflicting_roots[7] = 12.0;
        assert!(key(c_major, roots, conflicting_roots).is_none());
    }

    #[test]
    fn rejects_silence_and_a_single_pitch() {
        assert!(analyze(&vec![0.0; RATE * 3]).is_err());
        let tone = (0..RATE * 4)
            .map(|frame| (std::f32::consts::TAU * 220.0 * frame as f32 / RATE as f32).sin() * 0.25)
            .collect::<Vec<_>>();
        assert!(analyze(&tone).is_err());
    }
}
