import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tarfile
import urllib.request
import zipfile

release = "https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/"
platforms = {
    "Windows": {
        "1.31.1": (
            release + "onnxruntime-win-x64-1.31.1.zip",
            "5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c",
            ("onnxruntime.dll", "onnxruntime_providers_shared.dll"),
        ),
        "1.32.0": (
            "https://files.pythonhosted.org/packages/a9/3e/27039badc39a1b01628b6b40b1ff1bf4a886e6dfb2f311c483606b7c86f7/onnxruntime-1.32.0-cp312-cp312-win_amd64.whl",
            "efe7456952c243a05a53d5b8732953625144642c2f3989094277f6217f0c0c40",
            ("onnxruntime.dll", "onnxruntime_providers_shared.dll"),
        ),
    },
    "Linux": {
        "1.31.1": (
            release + "onnxruntime-linux-x64-1.31.1.tgz",
            "c849e36a1ef3e22d4ff217b9d1ce4a3cf5b9467b975610913ae959ad5b16bdc9",
            ("libonnxruntime.so.1.31.1", "libonnxruntime_providers_shared.so"),
        ),
        "1.32.0": (
            "https://files.pythonhosted.org/packages/a3/81/55845186c20cba59f614991f4065dea148e48eeb9749893768c85ce64327/onnxruntime-1.32.0-cp312-cp312-manylinux_2_28_x86_64.whl",
            "99e1074c4b2197accd5073052a465361be17b89c093b8ffb6e6bd8f986c52df8",
            ("libonnxruntime.so.1.32.0", "libonnxruntime_providers_shared.so"),
        ),
    },
}

native_archives = {
    "Windows": (
        [
            "https://dl.photoprism.app/onnx/runtime/v1.32.0/onnxruntime-win-x64-1.32.0.zip",
            "https://registry.npmmirror.com/-/binary/onnxruntime/v1.32.0/onnxruntime-win-x64-1.32.0.zip",
        ],
        "aaa1d04fcf86bd31e84b90bc0cbde80e632bd0720dcc5d8ba05c4fe45c09d127", 96945131,
        "348c3d29d6995eee37e3451825eaddb7ea7400be3afda6ca2286b12bc775f042",
    ),
    "Linux": (
        [
            "https://dl.photoprism.app/onnx/runtime/v1.32.0/onnxruntime-linux-x64-1.32.0.tgz",
            "https://registry.npmmirror.com/-/binary/onnxruntime/v1.32.0/onnxruntime-linux-x64-1.32.0.tgz",
        ],
        "bb169b7e374731730d2ae406fb39de7be4b315f413a9132efe896261c31bb503", 13425244,
        "4213d6d8c42ac4754865849cb139d31fc39127db6a326babf90bbc81a6eb52fc",
    ),
}

def sha256(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()

def download(url, path, expected):
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=180) as response, path.open("wb") as file:
                shutil.copyfileobj(response, file)
            break
        except OSError:
            if attempt == 2:
                raise
    actual = sha256(path)
    if actual != expected:
        raise RuntimeError(f"{path}: SHA-256 mismatch {actual} != {expected}")

def install(archive, output, names):
    output.mkdir()
    if archive.suffix in (".zip", ".whl"):
        with zipfile.ZipFile(archive) as src:
            found = {Path(member).name: member for member in src.namelist()}
            for name in names:
                with src.open(found[name]) as src_file, (output / name).open("wb") as dst:
                    shutil.copyfileobj(src_file, dst)
    else:
        with tarfile.open(archive) as src:
            found = {Path(member.name).name: member for member in src.getmembers() if member.isfile()}
            for name in names:
                with src.extractfile(found[name]) as src_file, (output / name).open("wb") as dst:
                    shutil.copyfileobj(src_file, dst)

def reported_ort_version(path):
    get_api = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_uint32)
    get_version = ctypes.CFUNCTYPE(ctypes.c_char_p)

    class OrtApiBase(ctypes.Structure):
        _fields_ = [("get_api", get_api), ("get_version", get_version)]

    library = ctypes.CDLL(str(path))
    library.OrtGetApiBase.restype = ctypes.POINTER(OrtApiBase)
    api = library.OrtGetApiBase().contents
    if not api.get_api(31):
        raise RuntimeError(f"API 31 unavailable in {path}")
    return api.get_version().decode("ascii")

os_name = platform.system()
root = Path(os.environ["RUNNER_TEMP"]) / "ort-beat-this"
root.mkdir()
models = root / "models"
models.mkdir()
model = models / "beat_this.onnx"
model_sha = "c5c1466e08abdb03fdeb50668a06f244b787d564c212490482231a9cfbe9ccbd"
download("https://raw.githubusercontent.com/mosynthkey/beat_this_cpp/07ab790a9ec2eda8093d52d249e3ec4f0510ee72/onnx/beat_this.onnx", model, model_sha)
song = Path("docs/samples/babushka-na-diskoteke.mp3").resolve()
song_sha = "e4afe83c474c2d86158c420227158de4e47cb013ca021cc9ba02e6e45a1291a7"
if sha256(song) != song_sha:
    raise RuntimeError("Published song fixture differs")
results = []
native_only = os.environ.get("QA_ORT_NATIVE_ONLY") == "1"
packages = platforms[os_name]
if native_only:
    urls, archive_sha, archive_bytes, native_library_sha = native_archives[os_name]
    packages = {"1.32.0": (urls[0], archive_sha, packages["1.32.0"][2])}

for version, (url, expected, names) in packages.items():
    archive = root / url.rsplit("/", 1)[1]
    if native_only:
        for source in urls:
            try:
                download(source, archive, expected)
                if archive.stat().st_size != archive_bytes:
                    raise RuntimeError("Native archive size differs")
                url = source
                break
            except (OSError, RuntimeError) as error:
                print(f"Mirror {source}: {error}", flush=True)
        else:
            raise RuntimeError("Native package unavailable from checked mirrors")
    else:
        download(url, archive, expected)
    libs = root / version
    install(archive, libs, names)
    library = libs / names[0]
    if native_only and sha256(library) != native_library_sha:
        raise RuntimeError("Unpacked native ORT library differs from the checked archive")
    directory = os.add_dll_directory(str(libs)) if os_name == "Windows" else None
    try:
        actual = reported_ort_version(library)
    finally:
        if directory:
            directory.close()
    if actual != version:
        raise RuntimeError(f"Expected ORT {version}, loaded {actual}")
    env = os.environ.copy()
    env.update(ORT_DYLIB_PATH=str(library), AUDIO_FACTS_MODELS=str(models), AUDIO_FACTS_SONGS=str(song))
    search_path = "PATH" if os_name == "Windows" else "LD_LIBRARY_PATH"
    env[search_path] = str(libs) + os.pathsep + env.get(search_path, "")
    run = subprocess.run(
        ["cargo", "test", "--locked", "-p", "music-server", "--lib", "audio_facts::tests::measures_real_songs",
         "--", "--exact", "--ignored", "--nocapture"],
        env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, encoding="utf-8", errors="replace")
    print(f"ORT {actual}, API 31, {library}", flush=True)
    print(run.stdout, flush=True)
    (root / f"{version}.log").write_text(run.stdout, encoding="utf-8")
    bpm_match = re.search(r"babushka-na-diskoteke\.mp3: ([0-9]+(?:\.[0-9]+)?) BPM \(([0-9.]+) s\)", run.stdout)
    if run.returncode or not bpm_match or "1 passed; 0 failed" not in run.stdout:
        raise RuntimeError(f"Beat This fixture failed on {os_name} with ORT {version}")
    results.append({
        "version": actual, "api": 31, "bpm": float(bpm_match[1]), "measure_seconds": float(bpm_match[2]),
        "archive": archive.name, "archive_url": url, "archive_size": archive.stat().st_size, "archive_sha256": expected, "library_sha256": sha256(library),
    })
report = {"os": os_name, "release": platform.release(), "architecture": platform.machine(),
          "song_sha256": song_sha, "beat_this_sha256": model_sha, "results": results}
(root / "results.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print("Beat This comparison:", json.dumps(report), flush=True)
