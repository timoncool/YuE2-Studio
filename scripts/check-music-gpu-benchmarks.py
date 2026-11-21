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


def section(path):
    content = path.read_text(encoding="utf-8")
    start = "<!-- music-gpu-matrix:start -->"
    end = "<!-- music-gpu-matrix:end -->"
    assert content.count(start) == content.count(end) == 1, path
    return content.split(start, 1)[1].split(end, 1)[0]


def readme_rows(path):
    return [
        [cell.strip().replace("`", "") for cell in line.strip("|").split("|")]
        for line in section(path).splitlines()
        if line.startswith("| ")
    ][2:]


def site_rows(path):
    parser = TableRows()
    parser.feed(section(path))
    return parser.rows[1:]


def expected_cells(row, decimal_comma):
    seconds = row["runs"][0]["audio_seconds"]
    peak = row["cold_vram_peak_bytes"]
    delta = row["cold_vram_increase_bytes"]
    label = " · ".join((
        row["family"], f"{row['engine']} {row['revision']}", row["weight_profile"], row["workload"],
    ))
    numbers = [
        f"{seconds:.3f}",
        f"{row['cold_seconds']:.3f}",
        f"{row['warm_median_seconds']:.3f}",
        f"{row['warm_audio_seconds_per_second']:.2f}×",
        f"{peak / GIB:.2f} / {delta / GIB:+.2f}",
        f"{row['cold_rss_peak_bytes'] / GIB:.2f}",
    ]
    if decimal_comma:
        numbers = [value.replace(".", ",") for value in numbers]
    return [label, row["backend"], *numbers]


def validate_table(path, observed, rows, decimal_comma=False):
    assert len(observed) == len(rows), f"{path}: {len(observed)} rows, expected {len(rows)}"
    for index, (actual, row) in enumerate(zip(observed, rows), 1):
        expected = expected_cells(row, decimal_comma)
        assert actual == expected, f"{path}: row {index}\nexpected {expected!r}\nfound    {actual!r}"


def verify_wav(evidence_dir, row):
    summary = evidence_dir / row["summary_id"]
    assert summary.is_file(), summary
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence-dir", type=Path, help="Also verify cold WAV hashes against the original files.")
    args = parser.parse_args()
    payload = (ROOT / "docs/music-gpu-benchmarks.json").read_text(encoding="utf-8")
    assert not re.search(r"[A-Za-z]:[/\\]|GPU-[a-f0-9-]{20}", payload), "Local paths or GPU UUID in public data"
    rows = json.loads(payload)["rows"]
    validate_data(rows, args.evidence_dir)
    readme = ROOT / "README.md"
    validate_table(readme, readme_rows(readme), rows)
    for locale in LOCALES:
        page = ROOT / "docs" / f"{locale}.html"
        validate_table(page, site_rows(page), rows, decimal_comma=locale in {"ru", "pt"})
    print(f"{len(rows)} music GPU rows match the README and {len(LOCALES)} site pages.")


if __name__ == "__main__":
    main()
