"""Hosted Windows QA for the settled v4.1.1 and draft v5.0.0 packages."""
import base64
from contextlib import contextmanager
import hashlib
import io
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import socket
import struct
import subprocess
import sys
import time
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener, urlopen
import wave
import zipfile

BASELINE = "v4.1.1"
BASELINE_REVISION = "5b1482ce8aac1401077ce6b09ae816119145a5c4"
BASELINE_SHA = "72da656cb407d4c8d4afd7bac4a897b47ac7ebeeff9c81300327b0421590a41f"
CANDIDATE = "v5.0.0"
RECEIPT = "windows-rc-receipt.json"
MINISIGN_ZIP = "minisign-0.12-win64.zip"
MINISIGN_SHA = "37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479"
WEIGHT = "nar_lora_joint_v9.safetensors"
WEIGHT_SHA = "585f303da1d5252d228d1e8ac6d4c4d11d970df9297406935cc8bdafa49cfa7e"
WEIGHT_BASE = ("https://huggingface.co/Mothersuperior/yue2-mothersuperior-realaudio-tokenizer-v4/"
               "resolve/e2e63d859f3af879baf1b4d4e9f22d1eeda6fde5/")
EXE = "YuE2-Studio.exe"
REQUIRED_FILES = {
    EXE, "resources/yue2-cpp/yue-server.exe", "resources/yue2-cpp/cuda12/ggml-cuda.dll",
    "resources/yue2-cpp/cuda13/ggml-cuda.dll", "resources/vst-host/vst-host.exe",
    "component-channel/root.json", "component-channel/url.txt",
}
API = "http://127.0.0.1:8791"
HTTP = build_opener(ProxyHandler({}))


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def verify_sha(path, expected):
    actual = sha(path)
    require(actual == expected.lower(), f"{path.name} SHA-256 {actual}, expected {expected}")
    return actual


def installer(tag):
    return f"YuE2.Studio_{tag[1:]}_x64-setup.exe"


def release_asset(repo, tag, name):
    return f"https://github.com/{repo}/releases/download/{tag}/{name}"


def public_download(url, path):
    with urlopen(Request(url, headers={"User-Agent": "YuE2-release-QA"}), timeout=60) as response:
        with path.open("wb") as destination:
            shutil.copyfileobj(response, destination)


def download(root, scenario):
    repo = os.environ["GITHUB_REPOSITORY"]
    shutil.copyfile(root / "draft.json", root / "evidence" / "draft.json")
    public_download(release_asset("jedisct1/minisign", "0.12", MINISIGN_ZIP), root / MINISIGN_ZIP)
    if scenario == "upgrade":
        for name in (installer(BASELINE), installer(BASELINE) + ".sig"):
            public_download(release_asset(repo, BASELINE, name), root / name)
        public_download(WEIGHT_BASE + WEIGHT, root / WEIGHT)


def validate_signature(root, tag, minisign, public_key):
    package = root / installer(tag)
    encoded = package.with_suffix(".exe.sig").read_bytes().strip()
    signature = package.with_suffix(".minisig")
    signature.write_bytes(encoded if encoded.startswith(b"untrusted comment:") else
                          base64.b64decode(encoded, validate=True))
    return subprocess.run([minisign, "-Vm", package, "-p", public_key, "-x", signature],
                          check=True, capture_output=True, text=True, timeout=60).stdout.strip()


def checked_manifest(directory, manifest):
    require(isinstance(manifest, dict), "installed_files_sha256 must be an object")
    paths = {}
    for name, expected in manifest.items():
        parts = PurePosixPath(name).parts
        require(parts and "/".join(parts) == name and all(
            re.fullmatch(r"[a-zA-Z0-9_.+ -]+", part) and part not in (".", "..") for part in parts),
            f"Unsafe installed path {name}")
        require(name.lower() not in paths and re.fullmatch(r"[a-fA-F0-9]{64}", expected),
                f"Duplicate path or malformed SHA-256 for {name}")
        path = directory.joinpath(*parts).resolve(strict=True)
        require(path.is_relative_to(directory), f"Installed file escapes installation: {name}")
        paths[name.lower()] = verify_sha(path, expected)
    require({name.lower() for name in REQUIRED_FILES} <= paths.keys(),
            f"Receipt lacks required files: {sorted(name for name in REQUIRED_FILES if name.lower() not in paths)}")
    return paths


def install(package, directory, update=False):
    # NSIS consumes the final /D= parameter verbatim. Updater mode instead locates the old install.
    options = "/S /UPDATE" if update else f"/S /D={directory}"
    command = subprocess.list2cmdline([str(package)]) + " " + options
    print(f"Running {package.name} {options}", flush=True)
    subprocess.run(command, check=True, timeout=600)
    require((directory / EXE).is_file(), f"Installed executable is missing from {directory}")


def request(route, method="GET", body=None, binary=False, timeout=15):
    data = None if body is None else json.dumps(body).encode()
    try:
        with HTTP.open(Request(API + route, data, {"Content-Type": "application/json"}, method=method),
                       timeout=timeout) as response:
            return response.read() if binary else json.load(response)
    except HTTPError as error:
        raise RuntimeError(f"{method} {route}: HTTP {error.code} {error.read(1000)!r}") from error


@contextmanager
def running(directory, evidence, phase, shared):
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 8791))
    exe = directory / EXE
    env = dict(os.environ, TIMONCOOL_SHARED_DIR=str(shared), DO_NOT_TRACK="1")
    with (evidence / f"{phase}.log").open("wb") as log:
        process = subprocess.Popen([exe, "--headless"], cwd=directory, env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 100
            while True:
                require(time.monotonic() < deadline and process.poll() is None,
                        f"{phase}: installed shell did not become healthy (exit={process.poll()})")
                try:
                    health = request("/health", timeout=3)
                    break
                except OSError:
                    time.sleep(0.5)
            require(health["status"] == "ok" and health["runtime"] == "native"
                    and health["engine_bundle_present"] and Path(health["service_executable"]).samefile(exe),
                    f"{phase}: unexpected installed service health {health}")
            yield health
        finally:
            if process.poll() is None:
                subprocess.run([exe, "--stop"], cwd=directory, env=env, stdout=log, stderr=log, timeout=25)
                try:
                    process.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=15)
                    raise


def install_fixture(data, weight):
    models = data / "models" / "yue2-cpp"
    models.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(weight, models / WEIGHT)
    require(request("/v1/library/songs") == [], "Baseline library is not empty")
    require(request("/v1/library/file-tags", "PUT", {"settings": False})["settings"] is False,
            "Published baseline did not save the non-default preference")
    (data / "media").mkdir(exist_ok=True)
    ids = []
    for number, frequency in enumerate((330, 440), start=1):
        name = f"release-acceptance-{number}.wav"
        title = f"Acceptance tone {number}"
        with wave.open(str(data / "media" / name), "wb") as wav:
            wav.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            wav.writeframes(b"".join(struct.pack("<h", round(4000 * math.sin(2 * math.pi * frequency * i / 16000)))
                                     for i in range(16000)))
        song = request("/v1/library/songs", "POST", {"title": title, "audio_path": name, "engine_id": "yue2-cpp"})
        ids.append(song["id"])
    playlist = request("/v1/library/playlists", "POST", {"name": "Upgrade order", "song_ids": ids[::-1]})
    return ids, playlist["id"]


def library_snapshot(data, fixture):
    ids, playlist_id = fixture
    songs = {row["id"]: row["title"] for row in request("/v1/library/songs")}
    require(songs == {song: f"Acceptance tone {i}" for i, song in enumerate(ids, start=1)},
            f"Unexpected library rows {songs}")
    playlist = request("/v1/library/playlists/" + playlist_id)
    require(playlist["name"] == "Upgrade order" and playlist["song_ids"] == ids[::-1],
            f"Unexpected playlist or order {playlist}")
    audio = {}
    pcm = {}
    for index, song in enumerate(ids, start=1):
        body = request("/v1/library/media/" + song, binary=True)
        require(body[:4] == b"RIFF" and body[8:12] == b"WAVE", f"Media {song} is not WAV")
        audio[song] = verify_sha(data / "media" / f"release-acceptance-{index}.wav",
                                 hashlib.sha256(body).hexdigest())
        with wave.open(io.BytesIO(body), "rb") as recording:
            require(recording.getparams()[:4] == (1, 2, 16000, 16000), f"Unexpected PCM parameters for {song}")
            pcm[song] = hashlib.sha256(recording.readframes(16000)).hexdigest()
    require(request("/v1/library/file-tags")["settings"] is False, "Saved file-tag preference was lost")
    return {"songs": songs, "playlist_id": playlist_id, "playlist_order": playlist["song_ids"],
            "served_wav_sha256": audio, "served_pcm_sha256": pcm, "file_tags": False,
            "settings_sha256": sha(data / "studio-settings.json"),
            "existing_weight_sha256": verify_sha(data / "models" / "yue2-cpp" / WEIGHT, WEIGHT_SHA)}


def verify(root, scenario):
    evidence = root / "evidence"
    directory = root / "installed"
    require(not directory.exists(), "Expected a fresh installation directory")
    manifest = json.loads((root / RECEIPT).read_text(encoding="utf-8-sig"))
    require(re.fullmatch(r"[a-fA-F0-9]{40}", manifest["revision"]), "Receipt lacks a full source revision")
    report = {"scenario": scenario, "source_revision": manifest["revision"], "baseline_tag": BASELINE,
              "updater_key_source_revision": BASELINE_REVISION, "phases": {}}
    report_path = evidence / "acceptance.json"
    try:
        verify_sha(root / MINISIGN_ZIP, MINISIGN_SHA)
        minisign = root / "minisign.exe"
        with zipfile.ZipFile(root / MINISIGN_ZIP) as archive:
            minisign.write_bytes(archive.read("minisign-win64/x86_64/minisign.exe"))
        config = json.loads(subprocess.check_output(
            ["git", "show", f"{BASELINE_REVISION}:desktop/src-tauri/tauri.conf.json"], text=True))
        public_key = root / "4.1.1-minisign.pub"
        public_key.write_bytes(base64.b64decode(config["plugins"]["updater"]["pubkey"], validate=True))
        candidate = root / installer(CANDIDATE)
        report["candidate_installer_sha256"] = verify_sha(candidate, manifest["installer_sha256"])
        report["candidate_signature_verification"] = validate_signature(root, CANDIDATE, minisign, public_key)
        data = directory / "data"
        if scenario == "upgrade":
            baseline = root / installer(BASELINE)
            report["baseline_installer_sha256"] = verify_sha(baseline, BASELINE_SHA)
            report["baseline_signature_verification"] = validate_signature(root, BASELINE, minisign, public_key)
            verify_sha(root / WEIGHT, WEIGHT_SHA)
            install(baseline, directory)
            report["baseline_installed_executable_sha256"] = sha(directory / EXE)
            with running(directory, evidence, "baseline", root / "shared") as health:
                before = report["phases"]["baseline"] = {"health": health}
                fixture = install_fixture(data, root / WEIGHT)
                before["library"] = library_snapshot(data, fixture)
        install(candidate, directory, update=scenario == "upgrade")
        report["installed_files_sha256"] = checked_manifest(directory.resolve(), manifest["installed_files_sha256"])
        require(not (directory / "portable.flag").exists(), "Installer unexpectedly deployed a portable marker")
        if scenario == "upgrade":
            report["settings_sha256_after_installer"] = verify_sha(
                data / "studio-settings.json", before["library"]["settings_sha256"])
            for index, song in enumerate(fixture[0], start=1):
                verify_sha(data / "media" / f"release-acceptance-{index}.wav", before["library"]["served_wav_sha256"][song])
        with running(directory, evidence, "candidate", root / "shared") as health:
            after = report["phases"]["candidate"] = {"health": health}
            if scenario == "upgrade":
                after["library"] = library_snapshot(data, fixture)
                for key in ("songs", "playlist_id", "playlist_order", "served_pcm_sha256", "file_tags",
                            "existing_weight_sha256"):
                    require(after["library"][key] == before["library"][key], f"Startup after upgrade changed {key}")
            else:
                require(request("/v1/library/songs") == [] and request("/v1/library/playlists") == [],
                        "Clean installation did not start with an empty library")
                require((data / "library.sqlite").is_file(), "Installed shell did not initialize its library on disk")
                after["library"] = {"songs": 0, "playlists": 0}
        report["passed"] = True
    finally:
        report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")


if __name__ == "__main__":
    action, scenario = sys.argv[1:]
    require(scenario in {"clean", "upgrade"}, f"Unknown scenario {scenario}")
    root = Path(os.environ["QA_DIR"]).resolve()
    (root / "evidence").mkdir(parents=True, exist_ok=True)
    {"download": download, "verify": verify}[action](root, scenario)
