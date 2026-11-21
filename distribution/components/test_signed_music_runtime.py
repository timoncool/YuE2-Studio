"""Isolated signed-runtime acceptance test. Python creates the QA catalogue and
calls Studio; the adapter and audio generation are both native processes.

Run with the built service and example adapter, a native ACE runtime directory
and an installed directory containing the three Turbo ACE GGUFs.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timedelta, timezone
from urllib.error import HTTPError, URLError
from urllib.request import ProxyHandler, Request, build_opener

from securesystemslib.signer import CryptoSigner
from tuf.api.metadata import Metadata, MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp

ENGINE = "fixture-ace-http"
MODEL = "fixture-ace-turbo"
TARGET = "music-engine-catalog.json"


def sign(role, key):
    metadata = Metadata(role)
    metadata.sign(key)
    return metadata.to_bytes()


def digest(file):
    value = hashlib.sha256()
    with file.open("rb") as stream:
        for chunk in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data), encoding="utf-8")


def hardlink_or_copy(source, target):
    try:
        os.link(source, target)
    except OSError:
        shutil.copy2(source, target)


def file_pins(files):
    return [
        {"name": name, "bytes": file.stat().st_size, "sha256": digest(file),
         "url": f"https://offline-fixture.example/{name}"}
        for name, file in sorted(files.items())
    ]


def make_packages(work, adapter, native, models, updated_native, updated_vae):
    suffix = ".exe" if sys.platform == "win32" else ""
    exe_name = f"music-adapter{suffix}"
    expected_os = {"win32": "windows", "darwin": "macos", "linux": "linux"}[sys.platform]
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}[platform.machine().lower()]
    def native_files(directory):
        return {
            f"ace-server{suffix}": directory / f"ace-server{suffix}",
            exe_name: adapter,
            **{file.name: file for file in directory.iterdir()
               if file.is_file() and (file.suffix in {".dll", ".dylib"} or ".so" in file.suffixes)
               and "cuda" not in file.name.lower() and "cublas" not in file.name.lower()},
        }
    weights = {file.name: file for file in models.glob("*.gguf")}
    assert len(weights) == 3 and "vae-BF16.gguf" in weights, "Provide only the three Turbo ACE GGUF files"
    engines = {1: native_files(native), 2: native_files(updated_native)}
    models_by_version = {1: weights, 2: weights | {"vae-BF16.gguf": updated_vae}}
    engine_pins = {version: file_pins(files) for version, files in engines.items()}
    model_pins = {version: file_pins(files) for version, files in models_by_version.items()}
    for name, versions in ((f"ace-server{suffix}", engine_pins), ("vae-BF16.gguf", model_pins)):
        assert len({pin["sha256"] for pins in versions.values() for pin in pins if pin["name"] == name}) == 2, name
    keys = {name: CryptoSigner.generate_ed25519() for name in ["root", "snapshot", "timestamp", "targets"]}
    expires = datetime.now(timezone.utc) + timedelta(days=1)
    root = Root(expires=expires)
    for role, key in keys.items():
        root.add_key(key.public_key, role)
    boot = work / "bin/component-channel"
    boot.mkdir(parents=True)
    (boot / "root.json").write_bytes(sign(root, keys["root"]))
    data = work / "data"
    engine_dir = data / "music-engines" / ENGINE
    model_dir = data / "music-models" / MODEL

    def publish(release):
        engine_version = 1 if release == 1 else 2
        model_version = 1 if release < 3 else 2
        engine = {"id": ENGINE, "name": "Fixture ACE", "version": f"{engine_version}.0",
                  "platform": f"{expected_os}-{arch}", "protocol": "music-http-v1",
                  "executable": exe_name, "files": engine_pins[engine_version]}
        model = {"id": MODEL, "name": "Fixture ACE Turbo", "version": f"{model_version}.0",
                 "engines": [ENGINE], "files": model_pins[model_version]}
        encoded = json.dumps({"schema": 1, "engines": [engine], "models": [model]}).encode()
        target = TargetFile.from_data(TARGET, encoded, ["sha256"])
        targets = sign(Targets(version=release, expires=expires, targets={TARGET: target}), keys["targets"])
        snapshot_metadata = sign(Snapshot(version=release, expires=expires,
                                 meta={"targets.json": MetaFile.from_data(release, targets, ["sha256"])}), keys["snapshot"])
        timestamp = sign(Timestamp(version=release, expires=expires,
                                   snapshot_meta=MetaFile.from_data(release, snapshot_metadata, ["sha256"])), keys["timestamp"])
        catalogue = data / "component-channel/music-engines"
        snapshot_name = f"snapshot-{release:06d}"
        snapshot = catalogue / snapshot_name
        (snapshot / "metadata").mkdir(parents=True)
        (snapshot / "targets").mkdir()
        for name, contents in (("timestamp.json", timestamp), (f"{release}.snapshot.json", snapshot_metadata),
                               (f"{release}.targets.json", targets)):
            (snapshot / "metadata" / name).write_bytes(contents)
        (snapshot / "targets" / (target.hashes["sha256"] + "." + TARGET)).write_bytes(encoded)
        changed = [(engine_dir, engine, engines[engine_version], engine_version),
                   (model_dir, model, models_by_version[model_version], model_version)]
        if release == 2:
            changed = changed[:1]
        elif release == 3:
            changed = changed[1:]
        for parent, package, sources, version in changed:
            installed = parent / f"generation-qa{version:04d}"
            (installed / "files").mkdir(parents=True)
            shutil.copytree(snapshot, installed / "proof")
            write(installed / "manifest.json", package)
            for name, source in sources.items():
                if parent == model_dir:
                    os.link(source, installed / "files" / name)
                else:
                    hardlink_or_copy(source, installed / "files" / name)
        return snapshot_name

    first, second = "generation-qa0001", "generation-qa0002"
    accepted = data / "component-channel/music-engines/accepted"
    snapshots = {release: publish(release) for release in (1, 2, 3)}
    accepted.write_text(snapshots[1], encoding="utf-8")
    for folder in (engine_dir, model_dir):
        write(folder / "active.json", {"active": first})
    return (data, engine_dir, model_dir, first, second, exe_name, engine_pins, model_pins, accepted,
            snapshots, engines, models_by_version)


def run(work, binary, adapter, native, models, updated_native, updated_vae):
    (data, engine_dir, model_dir, first, second, exe_name, engine_pins, model_pins, accepted,
     snapshots, engines, models_by_version) = make_packages(work, adapter, native, models, updated_native, updated_vae)
    binary = Path(shutil.copy2(binary, work / "bin" / binary.name))
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(data), YUE_STUDIO_PORT=str(port),
               YUE_ENGINE_BIN=str(work / "absent-yue"), DO_NOT_TRACK="1", TIMONCOOL_SHARED_DIR=str(work / "shared"))
    client = build_opener(ProxyHandler({}))
    rpc = []

    def call(route, data=None, expected=200, method=None):
        request = Request(f"http://127.0.0.1:{port}{route}", data=None if data is None else json.dumps(data).encode(),
                          headers={"Content-Type": "application/json", "Accept": "application/json, text/event-stream"},
                          method=method)
        try:
            with client.open(request, timeout=180) as response:
                assert response.status == expected, (route, response.status, expected)
                return response.read()
        except HTTPError as error:
            content = error.read()
            assert error.code == expected, (route, error.code, content)
            return content

    def rpc_call(method, params):
        request = {"jsonrpc": "2.0", "id": len(rpc) + 1, "method": method, "params": params}
        response = json.loads(call("/mcp", request))
        rpc.append({"request": request, "response": response})
        return response["result"]

    def tool(name, arguments):
        result = rpc_call("tools/call", {"name": name, "arguments": arguments})
        assert not result.get("isError"), result
        return result["structuredContent"]

    def complete(job):
        end = time.monotonic() + 180
        while time.monotonic() < end:
            run = tool("song_job_get", {"job_id": job["id"], "response_format": "detailed"})
            if run["status"] == "completed":
                return run["song"]["id"]
            assert run["status"] not in {"failed", "cancelled"}, run
            time.sleep(.25)
        raise TimeoutError(job)

    def download(song, name):
        audio = call(f"/v1/library/media/{song}")
        output = work / f"{name}.wav"
        output.write_bytes(audio)
        probe = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "a:0",
            "-show_entries", "stream=sample_rate,channels,codec_name", "-of", "json", output]))
        stream, = probe["streams"]
        assert stream == {"channels": 2, "sample_rate": "48000", "codec_name": "pcm_f32le"}, stream
        frames = subprocess.check_output(["ffmpeg", "-nostdin", "-v", "error", "-xerror", "-i", output,
                                         "-f", "f32le", "pipe:1"])
        duration = len(frames) / (2 * 4 * 48000)
        assert 3.9 <= duration <= 4.1 and any(abs(sample) > .001 for sample, in struct.iter_unpack("<f", frames))
        return {"duration": duration, "sha256": digest(output), "pcm_sha256": hashlib.sha256(frames).hexdigest()}

    def processes():
        if sys.platform == "win32":
            script = """Get-CimInstance Win32_Process -Filter "Name = 'music-adapter.exe' OR Name = 'ace-server.exe'" |
                ForEach-Object { "$($_.ProcessId) $($_.ParentProcessId) $($_.Name)" }"""
            command = ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script]
        else:
            command = ["ps", "-eo", "pid=,ppid=,comm="]
        return {int(pid): (int(parent), Path(name).stem.lower())
                for pid, parent, name in (line.strip().split(maxsplit=2) for line in subprocess.check_output(
                    command, text=True, timeout=10).splitlines() if line.strip())}

    def activate(release, folder):
        for path, value in ((accepted, snapshots[release]), (folder / "active.json", json.dumps(
                {"active": second, "previous": first}))):
            replacement = path.with_suffix(".next")
            replacement.write_text(value, encoding="utf-8")
            replacement.replace(path)

    def assert_pair(engine_version, model_version):
        catalogue = tool("music_engine_catalog_list", {})
        assert catalogue["source"] == "signed" and catalogue["engines"][0]["id"] == ENGINE
        assert catalogue["models"][0]["id"] == MODEL and ENGINE in catalogue["models"][0]["engines"]
        assert catalogue["engines"][0]["version"] == f"{engine_version}.0"
        assert catalogue["models"][0]["version"] == f"{model_version}.0"
        engine_status = tool("music_engine_package_status", {"id": ENGINE})
        model_status = tool("music_model_package_status", {"engine": ENGINE, "model": MODEL})
        assert engine_status["generation"] == f"generation-qa{engine_version:04d}"
        assert model_status["generation"] == f"generation-qa{model_version:04d}"

    source = {"kind": "signed", "engine": ENGINE, "model": MODEL}
    request = {"music_source": source, "style": "Instrumental funk groove, clean electric bass and dry drums",
               "lyrics": "[Instrumental]", "title": "Signed ACE control", "duration_seconds": 4,
               "steps": 8, "seed": 42, "output_format": "wav32"}
    with (work / "studio.log").open("w", encoding="utf-8") as log:
        server = subprocess.Popen([binary], cwd=binary.parent, env=env, stdout=log, stderr=log)
        try:
            until = time.monotonic() + 40
            while time.monotonic() < until:
                if server.poll() is not None:
                    raise RuntimeError(f"Studio exited {server.returncode}")
                try:
                    call("/health")
                    break
                except URLError:
                    time.sleep(.15)
            else:
                raise TimeoutError("Studio did not start")

            names = {entry["name"] for entry in rpc_call("tools/list", {})["tools"]}
            assert {"song_create", "song_replay", "music_engine_catalog_list",
                    "music_engine_package_status", "music_model_package_status"} <= names
            assert_pair(1, 1)
            original = complete(tool("song_create", request))
            before = json.loads(call(f"/v1/library/songs/{original}"))
            identity = before["metadata"]["signed_engine"]
            assert identity == {"engine": ENGINE, "model": MODEL, "generation": first,
                                "version": "1.0", "model_generation": first, "model_version": "1.0",
                                "protocol": "music-http-v1"}
            assert before["replay_request"]["signed_engine"] == identity
            first_wav = download(original, "original")

            publications = []
            for release, folder, engine_version, model_version in (
                    (2, engine_dir, 2, 1), (3, model_dir, 2, 2)):
                activate(release, folder)
                assert_pair(engine_version, model_version)
                song_id = complete(tool("song_create", request | {"seed": 42 + release}))
                song = json.loads(call(f"/v1/library/songs/{song_id}"))
                selected = song["metadata"]["signed_engine"]
                assert selected == identity | {
                    "generation": f"generation-qa{engine_version:04d}", "version": f"{engine_version}.0",
                    "model_generation": f"generation-qa{model_version:04d}", "model_version": f"{model_version}.0"}
                assert song["replay_request"]["signed_engine"] == selected and server.poll() is None
                publications.append({"release": release, "song": song_id, "identity": selected,
                                     "audio": download(song_id, f"publication-{release}")})
            replay = complete(tool("song_replay", {"song_id": original, "seed": 43}))
            saved = json.loads(call(f"/v1/library/songs/{replay}"))
            assert replay != original and saved["metadata"]["signed_engine"] == identity
            assert saved["metadata"]["derived"]["from"] == original
            assert saved["replay_request"]["payload"]["seed"] == 43
            second_wav = download(replay, "replay")
            assert second_wav["pcm_sha256"] != first_wav["pcm_sha256"]
            original_after = download(original, "original-after-replay")
            assert original_after["pcm_sha256"] == first_wav["pcm_sha256"]
            after = json.loads(call(f"/v1/library/songs/{original}"))
            assert after["replay_request"] == before["replay_request"] and after["metadata"]["signed_engine"] == identity
            for publication in publications:
                source_id = publication["song"]
                release = publication["release"]
                replay_id = complete(tool("song_replay", {"song_id": source_id, "seed": 50 + release}))
                take = json.loads(call(f"/v1/library/songs/{replay_id}"))
                assert replay_id != source_id and take["metadata"]["derived"]["from"] == source_id
                assert take["replay_request"]["signed_engine"] == publication["identity"]
                assert take["replay_request"]["payload"]["seed"] == 50 + release
                replay_audio = download(replay_id, f"publication-{release}-replay")
                assert replay_audio["pcm_sha256"] != publication["audio"]["pcm_sha256"]
                publication["replay"] = {"song": replay_id, "audio": replay_audio}

            long_job = tool("song_create", request | {"duration_seconds": 45, "steps": 32})
            workers = processes()
            adapter_pid, = [pid for pid, (parent, name) in workers.items()
                            if parent == server.pid and name == "music-adapter"]
            native_pid, = [pid for pid, (parent, name) in workers.items()
                           if parent == adapter_pid and name == "ace-server"]
            assert json.loads(call(f"/v1/music/jobs/{long_job['id']}", method="POST"))["status"] == "cancelled"
            assert json.loads(call(f"/v1/music/jobs/{long_job['id']}"))["status"] == "cancelled"
            assert not {adapter_pid, native_pid}.intersection(processes())

            active_exe = engine_dir / second / "files" / exe_name
            corrupted = active_exe.with_name(f"changed-{exe_name}")
            corrupted.write_bytes(b"not the signed adapter")
            corrupted.replace(active_exe)
            refused = json.loads(call("/v1/music/jobs", request, expected=503))
            assert "failed verification" in refused["message"], refused
            assert {version: file_pins(files) for version, files in engines.items()} == engine_pins
            assert {version: file_pins(files) for version, files in models_by_version.items()} == model_pins
            write(work / "result.json", {"identity": identity, "original_song": original, "replay_song": replay,
                "original": first_wav, "original_after_replay": original_after, "replay": second_wav,
                "publications": publications, "studio_pid": server.pid,
                "native_files": engine_pins, "weights": model_pins,
                "cancelled_job": long_job["id"], "stopped_pids": [adapter_pid, native_pid],
                "tampered_executable": refused["message"]})
            print(work / "result.json")
        finally:
            write(work / "mcp.json", rpc)
            if server.poll() is None:
                server.terminate()
            server.wait(timeout=20)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["studio", "adapter", "ace_runtime", "weights", "output", "updated_ace_runtime", "updated_vae"]:
        parser.add_argument(name, type=Path)
    arguments = parser.parse_args()
    arguments.output.mkdir()
    run(arguments.output.resolve(), arguments.studio.resolve(), arguments.adapter.resolve(),
        arguments.ace_runtime.resolve(), arguments.weights.resolve(), arguments.updated_ace_runtime.resolve(),
        arguments.updated_vae.resolve())
