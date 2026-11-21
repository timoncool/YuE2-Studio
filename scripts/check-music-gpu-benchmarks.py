"""Validate the README and six Pages tables against the music GPU measurements."""

import argparse
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
from statistics import median


ROOT = Path(__file__).resolve().parents[1]
GIB = 2**30
LOCALES = ("index", "ru", "ja", "ko", "zh", "pt")
SHA256 = re.compile(r"[0-9a-f]{64}")


class TableRows(HTMLParser):
    def __init__(self):
        super().__init__()
        self.rows = []
        self.cell = None

    def handle_starttag(self, tag, attrs):
        if tag == "tr":
            self.rows.append([])
        if tag == "td":
            self.cell = ""

    def handle_endtag(self, tag):
        if tag == "td":
            self.rows[-1].append(self.cell)
            self.cell = None

    def handle_data(self, data):
        if self.cell is not None:
            self.cell += data


def section(path, name="music-gpu-matrix"):
    content = path.read_text(encoding="utf-8")
    start = f"<!-- {name}:start -->"
    end = f"<!-- {name}:end -->"
    assert content.count(start) == content.count(end) == 1, path
    return content.split(start, 1)[1].split(end, 1)[0]


def readme_rows(path, name="music-gpu-matrix"):
    return [
        [cell.strip().replace("`", "") for cell in line.strip("|").split("|")]
        for line in section(path, name).splitlines()
        if line.startswith("| ")
    ][2:]


def site_rows(path, name="music-gpu-matrix"):
    parser = TableRows()
    parser.feed(section(path, name))
    return parser.rows[1:]


def expected_cells(row, resident, decimal_comma):
    seconds = row["runs"][0]["audio_seconds"]
    peak = row["cold_vram_peak_bytes"]
    delta = row["cold_vram_increase_bytes"]
    label = " · ".join((
        row["family"], f"{row['engine']} {row['revision']}", row["weight_profile"], row["workload"],
    ))
    label = resident["label"] if resident else label
    numbers = [
        f"{seconds:.3f}",
        f"{row['cold_seconds']:.3f}",
        f"{row['warm_median_seconds']:.3f}",
        f"{row['warm_audio_seconds_per_second']:.2f}×",
        f"{peak / GIB:.2f} / {delta / GIB:+.2f}",
        f"{row['warm_pre_request_vram_bytes'] / GIB:.2f}",
        f"{row['cold_rss_peak_bytes'] / GIB:.2f}",
        resident_cell(resident),
    ]
    if decimal_comma:
        numbers = [value.replace(".", ",") for value in numbers]
    return [label, row["backend"], *numbers]


def resident_cell(resident):
    if resident is None:
        return "—"
    return f"{resident['dedicated_gpu_bytes'] / GIB:.2f} / {resident['rss_bytes'] / GIB:.2f}"


def validate_table(path, observed, rows, residency, decimal_comma=False):
    with_timing = {entry["timing_summary_id"]: entry for entry in residency if "timing_summary_id" in entry}
    expected_rows = [
        expected_cells(row, with_timing.get(row["summary_id"]), decimal_comma)
        for row in rows
    ]
    for entry in residency:
        if "timing_summary_id" not in entry:
            numbers = [f"{entry['audio_seconds']:.3f}", *(["—"] * 6), resident_cell(entry)]
            if decimal_comma:
                numbers = [value.replace(".", ",") for value in numbers]
            expected_rows.append([entry["label"], entry["backend"], *numbers])
    assert len(observed) == len(expected_rows), f"{path}: {len(observed)} rows, expected {len(expected_rows)}"
    for index, (actual, expected) in enumerate(zip(observed, expected_rows), 1):
        assert actual == expected, f"{path}: row {index}\nexpected {expected!r}\nfound    {actual!r}"


def verify_residency(residency, rows, evidence_dir):
    timings = {row["summary_id"]: row for row in rows}
    for revision, entry in residency.items():
        assert revision in entry["label"] and entry["backend"] == "CUDA 13"
        assert entry["dedicated_gpu_bytes"] > 0 and entry["rss_bytes"] > 0 and entry["audio_seconds"] > 0
        for name in ("binary_sha256", "cuda_dll_sha256", "wav_sha256"):
            assert SHA256.fullmatch(entry[name]), entry["label"]
        if "timing_summary_id" in entry:
            timing = timings[entry["timing_summary_id"]]
            assert timing["binary_sha256"] == entry["binary_sha256"]
            assert timing["backend"] == entry["backend"]
            assert timing["revision"] == revision
        if evidence_dir is not None:
            receipt = evidence_dir / entry["receipt"]
            raw = json.loads(receipt.read_bytes())
            assert raw["published_engine"] == revision and raw["keep_loaded"] and raw["warmup_song_count"] == 1
            assert raw["binary_sha256"] == entry["binary_sha256"]
            assert raw["cuda_modules"]["ggml-cuda.dll"] == entry["cuda_dll_sha256"]
            assert raw["warmup_audio_seconds"] == entry["audio_seconds"]
            wav = receipt.with_name(receipt.name.replace("-resident.json", ".wav"))
            assert hashlib.sha256(wav.read_bytes()).hexdigest() == entry["wav_sha256"]
            samples = raw["samples"]
            assert len(samples) == 3
            assert entry["dedicated_gpu_bytes"] == median(sample["wddm_dedicated_gpu_bytes"] for sample in samples)
            assert entry["rss_bytes"] == median(sample["rss_bytes"] for sample in samples)


def verify_wav(evidence_dir, row):
    summary = evidence_dir / row["summary_id"]
    assert summary.is_file(), summary
    raw = json.loads(summary.read_bytes())
    runs = raw.get("rows") or raw["runs"]
    baselines = [(run.get("memory") or run)["gpus"][0]["device_baseline_bytes"] for run in runs[1:]]
    assert len(baselines) == 3 and median(baselines) == row["warm_pre_request_vram_bytes"], summary
    if summary.name == "summary.json":
        wav = summary.with_name("take-0.wav")
    else:
        suffix = "-cold.wav" if row["engine"] == "audio.cpp" else ".wav"
        wav = summary.with_name(summary.name.replace("-summary.json", suffix))
    digest = hashlib.sha256(wav.read_bytes()).hexdigest()
    assert digest == row["runs"][0]["wav_sha256"], wav


def validate_data(rows, evidence_dir):
    assert rows, "No benchmarks were recorded"
    assert len({row["summary_id"] for row in rows}) == len(rows), "Repeated measurement"
    for row in rows:
        source = row["summary_id"]
        cold, *warm = row["runs"]
        assert len(warm) == 3 and row["cold_seconds"] == cold["wall_seconds"], source
        assert row["warm_median_seconds"] == median(run["wall_seconds"] for run in warm), source
        assert row["warm_audio_seconds_per_second"] == median(
            run["audio_seconds"] / run["wall_seconds"] for run in warm
        ), source
        assert all(run["audio_seconds"] > 0 and run["wall_seconds"] > 0 for run in row["runs"]), source
        assert row["cold_vram_peak_bytes"] - row["cold_vram_baseline_bytes"] == row["cold_vram_increase_bytes"], source
        assert 0 < row["warm_pre_request_vram_bytes"] <= row["warm_vram_peak_bytes"], source
        assert row["audio_levels"]["rms_dbfs"] > -60 and row["weight_sha256"], source
        hashes = [
            row["binary_sha256"], row["request_sha256"], *(run["wav_sha256"] for run in row["runs"]),
            *row["weight_sha256"].values(), *row["module_sha256"].values(),
        ]
        if row["archive_sha256"] is not None:
            hashes.append(row["archive_sha256"])
        assert all(SHA256.fullmatch(digest) for digest in hashes), source
        if evidence_dir is not None:
            verify_wav(evidence_dir, row)


def verify_radeon(pair, evidence_dir):
    assert SHA256.fullmatch(pair["input_sha256"])
    assert pair["wddm_sample_interval_s"] == 1 and "gfx1036" in pair["card"]
    assert pair["request"]["request"]["duration_seconds"] == 4
    rows = []
    if evidence_dir is not None:
        raw = json.loads((evidence_dir / "yue2-242-radeon-paired.json").read_bytes())
        assert raw["source_sha"] == pair["source_sha"] and raw["input_sha256"] == pair["input_sha256"]
    for result in pair["results"]:
        cold, *warm = result["runs"]
        assert len(warm) == 3 and all(run["seconds"] == 4 and SHA256.fullmatch(run["sha256"]) for run in result["runs"])
        assert cold["wall_seconds"] == result["cold_seconds"]
        assert median(run["wall_seconds"] for run in warm) == result["warm_median_seconds"]
        peaks = result["wddm_peak_sampled_bytes"]
        if evidence_dir is not None:
            source = raw["results"][result["backend"].lower()]
            assert peaks == source["peak_sampled_bytes"] and result["samples"] == source["samples"]
            assert [run["sha256"] for run in result["runs"]] == [run["sha256"] for run in source["runs"]]
        rows.append([result["backend"], f"{result['cold_seconds']:.3f}", f"{result['warm_median_seconds']:.3f}",
                     f"{peaks['dedicated'] / GIB:.2f}", f"{peaks['shared'] / GIB:.2f}"])
    assert [row[0] for row in rows] == ["HIP", "Vulkan"]
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence-dir", type=Path, help="Also verify cold WAV hashes against the original files.")
    args = parser.parse_args()
    payload = (ROOT / "docs/music-gpu-benchmarks.json").read_text(encoding="utf-8")
    assert not re.search(r"[A-Za-z]:[/\\]|GPU-[a-f0-9-]{20}", payload), "Local paths or GPU UUID in public data"
    data = json.loads(payload)
    rows = data["rows"]
    published = data["published_residency"]
    candidates = data.get("candidate_residency", {})
    assert len(published) == 3 and published.keys().isdisjoint(candidates)
    residency = {**published, **candidates}
    verify_residency(residency, rows, args.evidence_dir)
    validate_data(rows, args.evidence_dir)
    radeon = verify_radeon(data["radeon_pair"], args.evidence_dir)
    native = []
    receipt = None
    if args.evidence_dir:
        receipt = json.loads((args.evidence_dir / "yue2-242-native-pair.json").read_bytes())
    for result in data["native_radeon_pair"]["results"]:
        cold, *warm = result["runs"]
        assert len(warm) == 3 and all(SHA256.fullmatch(run["sha256"]) and run["rms_pcm16"] > 1
                                     for run in result["runs"])
        assert result["process_start_to_first_wav_s"] >= cold["request_to_wav_s"]
        assert median(run["request_to_wav_s"] for run in warm) == result["warm_median_s"]
        peak = result["wddm"]
        assert peak["samples"] > 0
        if receipt is not None:
            source = receipt[{"HIP": "ROCm0", "Vulkan": "Vulkan1"}[result["backend"]]]
            assert source["runs"] == result["runs"] and source["revision"] == result["revision"]
            assert source["process_start_to_first_wav_s"] == result["process_start_to_first_wav_s"]
            assert peak == next(p for n, p in source["wddm_sampled_peaks_by_adapter"].items()
                                if n.endswith(receipt["verified_adapter_luid"]))
        native.append([result["backend"] + " " + result["revision"][:8],
                       f'{result["process_start_to_first_wav_s"]:.3f}', f'{result["warm_median_s"]:.3f}',
                       f'{peak["dedicated_bytes"]/GIB:.2f}', f'{peak["shared_bytes"]/GIB:.2f}'])
    radeon_section = "radeon-gpu-pair"
    readme = ROOT / "README.md"
    validate_table(readme, readme_rows(readme), rows, residency.values())
    assert readme_rows(readme, radeon_section) == radeon
    assert len(native) == 2 and readme_rows(readme, "native-radeon-pair") == native
    for locale in LOCALES:
        page = ROOT / "docs" / f"{locale}.html"
        validate_table(page, site_rows(page), rows, residency.values(), decimal_comma=locale in {"ru", "pt"})
        matrix = [[value.replace(".", ",") for value in row] for row in radeon] if locale in {"ru", "pt"} else radeon
        assert site_rows(page, radeon_section) == matrix, page
        native_rows = [[row[0], *[v.replace(".", ",") for v in row[1:]]] for row in native] if locale in {"ru", "pt"} else native
        assert site_rows(page, "native-radeon-pair") == native_rows, page
        assert data["radeon_pair"]["source_sha"][:8] in section(page, radeon_section), page
    print(f"{len(rows)} timing results, {len(residency)} residency results and both Radeon comparisons match the README and {len(LOCALES)} site pages.")


if __name__ == "__main__":
    main()
