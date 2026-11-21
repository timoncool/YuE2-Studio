"""Repack audited Windows HOT-Step assets with CPU modules built at the old GGML pin."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

VARIANTS = ("alderlake", "cannonlake", "cascadelake", "haswell", "icelake", "sandybridge", "skylakex", "sse42", "x64")
CPU_NAMES = {f"ggml-cpu-{name}.dll" for name in VARIANTS}
OLD = {
    "hotstep-tools-cuda-windows-x64.zip": ("hotstep-tools-xcr0-cuda-windows-x64.zip",
        "f423135405e51ed861b51ad1fa8dc108d891914951915eb54e18c03b8271331a",
        ("music-train.exe", "music-midi.exe", "ace-caption.exe")),
    "ace-caption-windows-x64.zip": ("ace-caption-xcr0-windows-x64.zip",
        "254af78ec1254ad2d141afb12f5dc20014dcbbb2387eed1b63a975d5ee46a52a",
        ("ace-caption.exe",)),
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def scores(root):
    with os.add_dll_directory(str(root)):
        libraries = [ctypes.CDLL(str(root / name)) for name in ("ggml-base.dll", "ggml.dll")]
        results = {}
        for name in VARIANTS:
            library = ctypes.CDLL(str(root / f"ggml-cpu-{name}.dll"))
            libraries.append(library)
            entry = library.ggml_backend_score
            entry.argtypes = []
            entry.restype = ctypes.c_int
            results[name] = entry()
        assert results["x64"] > 0
        return results


def inspect(archive, programs):
    with tempfile.TemporaryDirectory(prefix="hotstep-xcr0-") as directory:
        root = Path(directory)
        with zipfile.ZipFile(archive) as bundle:
            assert bundle.testzip() is None
            assert all(Path(name).name == name for name in bundle.namelist())
            bundle.extractall(root)
        smoke = {}
        for name in programs:
            result = subprocess.run([root / name, "--help"], cwd=root, text=True, errors="replace",
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
            assert result.returncode in (0, 1, 2) and "usage" in result.stdout.lower(), (name, result.returncode, result.stdout)
            smoke[name] = {"exit": result.returncode, "first_line": result.stdout.splitlines()[0]}
        module_scores = json.loads(subprocess.check_output([sys.executable, __file__, "--scores", root], text=True))
        return {"programs": smoke, "cpu_scores": module_scores}


def main(old_dir, build, output, patch):
    output.mkdir()
    changed = {}
    for name in sorted(CPU_NAMES):
        source, = build.rglob(name)
        changed[name] = source.read_bytes()
    packages = []
    for old_name, (new_name, old_digest, programs) in OLD.items():
        original, candidate = old_dir / old_name, output / new_name
        assert digest(original.read_bytes()) == old_digest
        with zipfile.ZipFile(original) as before, zipfile.ZipFile(candidate, 'x') as after:
            names = before.namelist()
            assert CPU_NAMES <= set(names) and len(names) == len(set(names))
            old_files = {name: digest(before.read(name)) for name in names}
            for info in before.infolist():
                old = before.read(info)
                data = changed.get(info.filename, old)
                after.writestr(info, data)
        with zipfile.ZipFile(candidate) as verify:
            new_files = {name: digest(verify.read(name)) for name in verify.namelist()}
        assert new_files.keys() == old_files.keys()
        assert {name for name in old_files if old_files[name] != new_files[name]} == CPU_NAMES
        packages.append({"archive": new_name, "archive_bytes": candidate.stat().st_size,
            "archive_sha256": digest(candidate.read_bytes()), "old_archive_sha256": old_digest,
            "old": inspect(original, programs), "new": inspect(candidate, programs),
            "entry_before": old_files, "entry_after": new_files})
    receipt = {"ggml_commit": "9a3939d487be7fb9a868537ebc8c970d461fa8f6",
               "patch_sha256": digest(patch.read_bytes()), "packages": packages}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")


if __name__ == "__main__":
    if sys.argv[1] == "--scores":
        print(json.dumps(scores(Path(sys.argv[2]))))
    else:
        main(*(Path(value).resolve() for value in sys.argv[1:5]))
