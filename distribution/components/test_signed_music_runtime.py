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


def make_packages(work, adapter, native, models):
    suffix = ".exe" if sys.platform == "win32" else ""
    exe_name = f"music-adapter{suffix}"
    expected_os = {"win32": "windows", "darwin": "macos", "linux": "linux"}[sys.platform]
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}[platform.machine().lower()]
    engine_files = {
        f"ace-server{suffix}": native / f"ace-server{suffix}",
        exe_name: adapter,
        **{file.name: file for file in native.iterdir()
           if file.is_file() and (file.suffix in {".dll", ".dylib"} or ".so" in file.suffixes)
           and "cuda" not in file.name.lower() and "cublas" not in file.name.lower()},
    }
    weights = {file.name: file for file in models.glob("*.gguf")}
    assert len(weights) == 3, "Provide only the three Turbo ACE GGUF files"
    keys = {name: CryptoSigner.generate_ed25519() for name in ["root", "snapshot", "timestamp", "targets"]}
    expires = datetime.now(timezone.utc) + timedelta(days=1)
    root = Root(expires=expires)
    for role, key in keys.items():
        root.add_key(key.public_key, role)
    boot = work / "bin/component-channel"
    boot.mkdir(parents=True)
    (boot / "root.json").write_bytes(sign(root, keys["root"]))
    all_pins = file_pins(engine_files), file_pins(weights)
    data = work / "data"
    engine_dir = data / "music-engines" / ENGINE
    model_dir = data / "music-models" / MODEL

    def publish(release):
        version = f"{release}.0"
        generation = f"generation-qa{release:04d}"
        engine = {"id": ENGINE, "name": "Fixture ACE", "version": version,
                  "platform": f"{expected_os}-{arch}", "protocol": "music-http-v1",
                  "executable": exe_name, "files": all_pins[0]}
        model = {"id": MODEL, "name": "Fixture ACE Turbo", "version": version,
                 "engines": [ENGINE], "files": all_pins[1]}
        encoded = json.dumps({"schema": 1, "engines": [engine], "models": [model]}).encode()
        target = TargetFile.from_data(TARGET, encoded, ["sha256"])
        targets = sign(Targets(version=release, expires=expires, targets={TARGET: target}), keys["targets"])
        snapshot = sign(Snapshot(version=release, expires=expires,
                                 meta={"targets.json": MetaFile.from_data(release, targets, ["sha256"])}), keys["snapshot"])
        timestamp = sign(Timestamp(version=release, expires=expires,
                                   snapshot_meta=MetaFile.from_data(release, snapshot, ["sha256"])), keys["timestamp"])
        for parent, package, sources in [(engine_dir, engine, engine_files), (model_dir, model, weights)]:
            installed = parent / generation
            (installed / "proof/metadata").mkdir(parents=True)
            (installed / "proof/targets").mkdir()
            (installed / "files").mkdir()
            for name, contents in (("timestamp.json", timestamp), (f"{release}.snapshot.json", snapshot),
                                   (f"{release}.targets.json", targets)):
                (installed / "proof/metadata" / name).write_bytes(contents)
            (installed / "proof/targets" / (target.hashes["sha256"] + "." + TARGET)).write_bytes(encoded)
            write(installed / "manifest.json", package)
            for name, source in sources.items():
                hardlink_or_copy(source, installed / "files" / name)
        return generation

    first, second = publish(1), publish(2)
    catalogue = data / "component-channel/music-engines"
    shutil.copytree(engine_dir / first / "proof", catalogue / "snapshot-signed")
    (catalogue / "accepted").write_text("snapshot-signed", encoding="utf-8")
    for folder in (engine_dir, model_dir):
        write(folder / "active.json", {"active": first})
    return data, engine_dir, model_dir, first, second, exe_name, all_pins


def run(work, binary, adapter, native, models):
    data, engine_dir, model_dir, first, second, exe_name, pins = make_packages(work, adapter, native, models)
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
            catalogue = tool("music_engine_catalog_list", {})
            assert catalogue["source"] == "signed" and catalogue["engines"][0]["id"] == ENGINE
            assert catalogue["models"][0]["id"] == MODEL and ENGINE in catalogue["models"][0]["engines"]
            assert tool("music_engine_package_status", {"id": ENGINE})["generation"] == first
            assert tool("music_model_package_status", {"engine": ENGINE, "model": MODEL})["generation"] == first
            original = complete(tool("song_create", request))
            before = json.loads(call(f"/v1/library/songs/{original}"))
            identity = before["metadata"]["signed_engine"]
            assert identity == {"engine": ENGINE, "model": MODEL, "generation": first,
                                "version": "1.0", "model_generation": first, "model_version": "1.0",
                                "protocol": "music-http-v1"}
            assert before["replay_request"]["signed_engine"] == identity
            first_wav = download(original, "original")

            for folder in (engine_dir, model_dir):
                write(folder / "active.json", {"active": second, "previous": first})
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

            long_job = tool("song_create", request | {"duration_seconds": 45, "steps": 32})
            assert json.loads(call(f"/v1/music/jobs/{long_job['id']}", method="POST"))["status"] == "cancelled"
            assert json.loads(call(f"/v1/music/jobs/{long_job['id']}"))["status"] == "cancelled"

            active_exe = engine_dir / second / "files" / exe_name
            corrupted = active_exe.with_name(f"changed-{exe_name}")
            corrupted.write_bytes(b"not the signed adapter")
            corrupted.replace(active_exe)
            refused = json.loads(call("/v1/music/jobs", request, expected=503))
            assert "failed verification" in refused["message"], refused
            write(work / "result.json", {"identity": identity, "original_song": original, "replay_song": replay,
                "original": first_wav, "original_after_replay": original_after, "replay": second_wav,
                "native_files": pins[0], "weights": pins[1],
                "cancelled_job": long_job["id"], "tampered_executable": refused["message"]})
            print(work / "result.json")
        finally:
            write(work / "mcp.json", rpc)
            if server.poll() is None:
                server.terminate()
            server.wait(timeout=20)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["studio", "adapter", "ace_runtime", "weights", "output"]:
        parser.add_argument(name, type=Path)
    arguments = parser.parse_args()
    arguments.output.mkdir()
    run(arguments.output.resolve(), arguments.studio.resolve(), arguments.adapter.resolve(),
        arguments.ace_runtime.resolve(), arguments.weights.resolve())
