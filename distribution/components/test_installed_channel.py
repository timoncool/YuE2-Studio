"""Linux package QA. Run in a disposable network-isolated container with loopback.

Arguments: built music-server, pinned Linux x64 ONNX 1.32 archive, TLS certificate
for localhost (trusted inside this container only), matching private TLS key.
TUF keys are generated for this run. No installed library is used.
"""
import ctypes
import functools
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timedelta, timezone
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener

from securesystemslib.signer import CryptoSigner
from tuf.api.metadata import Metadata, MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp


DIGEST = "bb169b7e374731730d2ae406fb39de7be4b315f413a9132efe896261c31bb503"


def sign(role, key):
    metadata = Metadata(role)
    metadata.sign(key)
    return metadata.to_bytes()


def run(work, binary, archive, certificate, tls_key):
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == DIGEST
    install = work / "package/usr/bin"
    install.mkdir(parents=True)
    origin = work / "origin"
    channel = work / "package/usr/lib/YuE2 Studio/component-channel"
    channel.mkdir(parents=True)
    (origin / "targets").mkdir(parents=True)
    exe = Path(shutil.copy2(binary, install))
    shutil.copyfile(archive, origin / "current.tgz")
    binary_hash = hashlib.sha256(exe.read_bytes()).hexdigest()
    requested = []
    interrupted = []

    class Handler(SimpleHTTPRequestHandler):
        def do_GET(self):
            if self.path != "/timestamp.json" or not cut_timestamp:
                return super().do_GET()
            content = (origin / "timestamp.json").read_bytes()
            self.send_response(200)
            self.send_header("Content-Length", str(len(content)))
            self.end_headers()
            self.wfile.write(content[:len(content) // 2])
            self.wfile.flush()
            self.close_connection = True
            interrupted.append(self.path)

        def log_request(self, code="-", size="-"):
            requested.append((self.path, int(code)))

    cut_timestamp = False
    keys = {role: CryptoSigner.generate_ed25519() for role in ("root", "snapshot", "timestamp", "targets")}
    expires = datetime.now(timezone.utc) + timedelta(days=1)
    root = Root(expires=expires)
    for name, key in keys.items():
        root.add_key(key.public_key, name)
    (channel / "root.json").write_bytes(sign(root, keys["root"]))

    with ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=origin)) as server:
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(certificate, tls_key)
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        base = f"https://localhost:{server.server_port}/"
        (channel / "url.txt").write_text(base)
        threading.Thread(target=server.serve_forever, daemon=True).start()

        def publish(revision, filename, extra=None):
            name = "mirrors.json"
            archives = {DIGEST: {"sources": [base + filename]}, **(extra or {})}
            data = json.dumps({"schema": 1, "revision": revision, "archives": archives}).encode()
            target = TargetFile.from_data(name, data, ["sha256"])
            (origin / "targets" / (target.hashes["sha256"] + "." + name)).write_bytes(data)
            targets = sign(Targets(version=revision, expires=expires, targets={name: target}), keys["targets"])
            (origin / f"{revision}.targets.json").write_bytes(targets)
            snap = sign(Snapshot(version=revision, expires=expires, meta={"targets.json": MetaFile.from_data(revision, targets, ["sha256"])}), keys["snapshot"])
            (origin / f"{revision}.snapshot.json").write_bytes(snap)
            (origin / "timestamp.json").write_bytes(sign(Timestamp(version=revision, expires=expires, snapshot_meta=MetaFile.from_data(revision, snap, ["sha256"])), keys["timestamp"]))

        publish(1, "removed.tgz")
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        url = f"http://127.0.0.1:{port}"
        http = build_opener(ProxyHandler({}))
        data = work / "studio"
        env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(data), YUE_STUDIO_PORT=str(port), HOME=str(work))
        proc = subprocess.Popen([exe], cwd=install, env=env)
        try:
            for _ in range(120):
                if proc.poll() is not None:
                    raise RuntimeError(f"service exited {proc.returncode}")
                try:
                    with http.open(url + "/health", timeout=2):
                        break
                except URLError:
                    time.sleep(0.25)
            else:
                raise TimeoutError("service startup")

            def install_runtime(asset_id="onnxruntime"):
                req = Request(url + "/v1/karaoke/install", json.dumps({"asset_id": asset_id}).encode(), {"Content-Type": "application/json"})
                with http.open(req, timeout=180) as response:
                    json.load(response)
                deadline = time.monotonic() + 180
                while time.monotonic() < deadline:
                    with http.open(url + "/v1/karaoke/status", timeout=10) as response:
                        job = json.load(response).get("active_download")
                    if job and job["asset_id"] == asset_id and job["done"]:
                        return job
                    time.sleep(0.25)
                raise TimeoutError(f"install job {asset_id}")

            first = install_runtime()
            assert first["error"] and ("/removed.tgz", 404) in requested, (first, requested)
            timestamp = origin / "timestamp.json"
            old_timestamp = timestamp.read_bytes()
            publish(2, "also-removed.tgz")
            missing = install_runtime()
            assert missing["error"] and ("/also-removed.tgz", 404) in requested, (missing, requested)

            # Replay the original, correctly signed timestamp after accepting version 2.
            timestamp.write_bytes(old_timestamp)
            requested.clear()
            replay = install_runtime()
            assert replay["error"] and ("/timestamp.json", 200) in requested, (replay, requested)
            assert not any(path == "/removed.tgz" for path, _ in requested), requested

            publish(3, "unsigned.tgz")
            targets = json.loads((origin / "3.targets.json").read_bytes())
            digest = targets["signed"]["targets"]["mirrors.json"]["hashes"]["sha256"]
            manifest = origin / "targets" / (digest + ".mirrors.json")
            manifest.write_bytes(manifest.read_bytes().replace(b"unsigned.tgz", b"tampered.tgz"))
            requested.clear()
            corrupt = install_runtime()
            assert corrupt["error"] and ("/targets/" + manifest.name, 200) in requested, (corrupt, requested)
            assert not any(path == "/tampered.tgz" for path, _ in requested), requested

            publish(4, "current.tgz")
            cut_timestamp = True
            requested.clear()
            dropped = install_runtime()
            assert dropped["error"] and interrupted, (dropped, requested)
            assert not any(path == "/current.tgz" for path, _ in requested), requested
            assert not any(data.rglob("libonnxruntime.so*"))
            cut_timestamp = False
            (origin / "current.tgz").write_bytes(b"incorrect archive")
            requested.clear()
            bad_archive = install_runtime()
            assert bad_archive["error"] and ("/current.tgz", 200) in requested, (bad_archive, requested)
            assert not any(data.rglob("libonnxruntime.so*"))
            shutil.copyfile(archive, origin / "current.tgz")
            requested.clear()
            second = install_runtime()
            assert not second["error"], second
            assert ("/current.tgz", 200) in requested, requested
            assert any(p.is_file() for p in data.rglob("libonnxruntime.so*"))
            print("PASS same service PID", proc.pid, "old URL 404 -> signed new URL -> installed ONNX", DIGEST)
            # This component has a shipped content pin, but no shipped mirror catalogue entry.
            config = archive.parent / "crisper-config.json"
            config_sha = "9d823d0eacddfc5ee920fac73897abfd29fbecac7d9e49b5d8e7f0bd63a56144"
            assert hashlib.sha256(config.read_bytes()).hexdigest() == config_sha
            shutil.copyfile(config, origin / "relocated-config.json")
            publish(5, "current.tgz", {config_sha: {"sources": [base + "relocated-config.json"]}})
            requested.clear()
            result = install_runtime("crisperwhisper-turbo-config")
            assert not result["error"], result
            assert ("/relocated-config.json", 200) in requested, requested
            stored, = data.rglob("crisperwhisper2-turbo/config.json")
            assert hashlib.sha256(stored.read_bytes()).hexdigest() == config_sha
            print("PASS same service PID", proc.pid, "signed mirror for a previously unlisted pinned config", config_sha)
            requested.clear()
            reused = install_runtime()
            assert not reused["error"] and not requested, (reused, requested)
            # Cut off the complete mirror host after a successful installation.
            server.shutdown()
            server.server_close()
            requested.clear()
            offline = install_runtime()
            assert not offline["error"] and not requested, (offline, requested)
            assert hashlib.sha256(stored.read_bytes()).hexdigest() == config_sha
            library = next(path for path in data.rglob("libonnxruntime.so*") if path.is_file())
            runtime = ctypes.CDLL(str(library))
            runtime.OrtGetApiBase.restype = ctypes.c_void_p
            assert runtime.OrtGetApiBase(), library
            print("PASS incorrect archive rejected, installed runtime reopened offline", library.name)
            assert proc.poll() is None and hashlib.sha256(exe.read_bytes()).hexdigest() == binary_hash
            print("PASS interrupted timestamp rejected; retry recovered in unchanged binary SHA-256", binary_hash)
        finally:
            proc.terminate()
            proc.wait(timeout=20)
            server.shutdown()


def run_rvc(work, binary, certificate, tls_key, install_voice=False):
    install = work / "install"
    channel = install / "component-channel"
    origin = work / "origin"
    channel.mkdir(parents=True)
    (origin / "targets").mkdir(parents=True)
    exe = Path(shutil.copy2(binary, install))
    source = Path(__file__).resolve().parents[2] / "config" / "rvc-voice-catalog.json"
    bundled = json.loads(source.read_text(encoding="utf-8"))
    voice = next(v for v in bundled["voices"] if v["id"] == "rvc-486ca595c7aeb194e1c5")
    assert voice.get("index") is None and voice.get("archive") is None
    keys = {role: CryptoSigner.generate_ed25519() for role in ("root", "snapshot", "timestamp", "targets")}
    expires = datetime.now(timezone.utc) + timedelta(days=1)
    root = Root(expires=expires)
    for name, key in keys.items():
        root.add_key(key.public_key, name)
    root_metadata = sign(root, keys["root"])
    (channel / "root.json").write_bytes(root_metadata)
    (origin / "1.root.json").write_bytes(root_metadata)
    request_paths = []
    hold_library = threading.Event()
    library_requested = threading.Event()
    release_library = threading.Event()

    class Handler(SimpleHTTPRequestHandler):
        def do_GET(self):
            if self.path == "/music.dll" and hold_library.is_set():
                self.send_response(200)
                self.send_header("Content-Length", str((origin / "music.dll").stat().st_size))
                self.end_headers()
                library_requested.set()
                release_library.wait(20)
                return
            super().do_GET()

        def log_request(self, code="-", size="-"):
            request_paths.append(self.path)

    def publish(version, manifest, name="rvc-voice-catalog.json"):
        raw = json.dumps(manifest).encode()
        target = TargetFile.from_data(name, raw, ["sha256"])
        path = origin / "targets" / f"{target.hashes['sha256']}.{name}"
        path.write_bytes(raw)
        targets = sign(Targets(version=version, expires=expires, targets={name: target}), keys["targets"])
        (origin / f"{version}.targets.json").write_bytes(targets)
        snap = sign(Snapshot(version=version, expires=expires, meta={
            "targets.json": MetaFile.from_data(version, targets, ["sha256"]),
        }), keys["snapshot"])
        (origin / f"{version}.snapshot.json").write_bytes(snap)
        (origin / "timestamp.json").write_bytes(sign(Timestamp(
            version=version, expires=expires, snapshot_meta=MetaFile.from_data(version, snap, ["sha256"]),
        ), keys["timestamp"]))
        return path

    def start(studio):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(studio), YUE_STUDIO_PORT=str(port), HOME=str(work))
        proc = subprocess.Popen([exe], cwd=install, env=env)
        for _ in range(120):
            if proc.poll() is not None:
                raise RuntimeError(f"service exited {proc.returncode}")
            try:
                with http.open(base + "/health", timeout=2):
                    return proc, base
            except URLError:
                time.sleep(0.25)
        proc.terminate()
        proc.wait(timeout=20)
        raise TimeoutError("service startup")

    def tool(name, arguments=None, expected_error=False):
        request = Request(base + "/mcp", json.dumps({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": arguments or {}},
        }).encode(), {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"})
        with http.open(request, timeout=150) as response:
            result = json.load(response)["result"]
        assert bool(result.get("isError")) == expected_error, result
        return result if expected_error else result["structuredContent"]

    def engine_catalogue(refresh=False):
        path = "/v1/components/music-engines" + ("/refresh" if refresh else "")
        request = Request(base + path, method="POST" if refresh else "GET")
        with http.open(request, timeout=60) as response:
            return json.load(response)

    http = build_opener(ProxyHandler({}))
    studio = work / "studio"
    with ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=origin)) as server:
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(certificate, tls_key)
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        (channel / "url.txt").write_text(f"https://localhost:{server.server_port}/")
        threading.Thread(target=server.serve_forever, daemon=True).start()
        proc, base = start(studio)
        try:
            assert tool("rvc_catalog_list")["total"] == len(bundled["voices"])
            assert not request_paths
            publish(1, {"schema_version": 1, "voices": [voice]})
            initial = (origin / "timestamp.json").read_bytes()
            receipt = tool("rvc_catalog_refresh")
            assert receipt["voices"] == 1 and receipt["source"] == "signed", receipt
            signed_voice = dict(voice, name="Signed RVC catalogue test voice")
            publish(2, {"schema_version": 1, "voices": [signed_voice]})
            tool("rvc_catalog_refresh")
            current = tool("rvc_catalog_list", {"has_index": False})
            assert current["total"] == 1 and current["voices"][0]["name"] == signed_voice["name"], current

            if install_voice:
                result = tool("rvc_catalog_install", {"id": voice["id"]})
                assert result["index_name"] is None, result
                paths = list((studio / "rvc" / "voices").rglob("voice.pth"))
                assert len(paths) == 1 and hashlib.sha256(paths[0].read_bytes()).hexdigest() == voice["weights"]["sha256"]
                assert not list((studio / "rvc" / "voices").rglob("*.index"))
                print("PASS published checkpoint installed through MCP without index", voice["id"], voice["weights"]["sha256"])

            (origin / "timestamp.json").write_bytes(initial)
            tool("rvc_catalog_refresh", expected_error=True)
            assert tool("rvc_catalog_list", {"has_index": False})["voices"][0]["name"] == signed_voice["name"]
            tampered = publish(3, {"schema_version": 1, "voices": [voice]})
            tampered.write_bytes(tampered.read_bytes() + b" ")
            tool("rvc_catalog_refresh", expected_error=True)
            publish(4, {"schema_version": 2, "voices": [voice]})
            tool("rvc_catalog_refresh", expected_error=True)
            assert engine_catalogue()["engines"] == []
            url = f"https://localhost:{server.server_port}/"
            binary_bytes = b"fixture executable for package verification"
            library_bytes = b"revision one"
            (origin / "worker").write_bytes(binary_bytes)
            (origin / "music.dll").write_bytes(library_bytes)
            def pinned(name, contents):
                return {"name": name, "url": url+name, "bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}
            engine = {"id": "fixture-engine", "name": "Fixture", "protocol": "music-http-v1", "version": "1",
                      "platform": "windows-x86_64" if os.name == "nt" else "linux-x86_64", "executable": "worker",
                      "files": [pinned("worker", binary_bytes), pinned("music.dll", library_bytes)]}
            publish(5, {"schema": 1, "engines": [engine]}, "music-engine-catalog.json")
            assert engine_catalogue(refresh=True)["engines"] == [engine]
            args = {"id": engine["id"]}
            first = tool("music_engine_package_install", args)
            assert first["downloaded_bytes"] == len(binary_bytes) + len(library_bytes), first
            assert tool("music_engine_package_status", args)["generation"] == first["generation"]
            request_paths.clear()
            assert tool("music_engine_package_install", args)["generation"] == first["generation"]
            assert not request_paths, request_paths
            publish(6, {"schema": 9, "engines": []}, "music-engine-catalog.json")
            try:
                engine_catalogue(refresh=True)
            except URLError as error:
                assert getattr(error, "code", None) == 502, error
            else:
                raise AssertionError("incompatible catalogue was accepted")
            assert engine_catalogue()["engines"] == [engine]
            engine["version"] = "2"
            new_library = b"revision two"
            engine["files"][1] = pinned("music.dll", new_library)
            publish(7, {"schema": 1, "engines": [engine]}, "music-engine-catalog.json")
            engine_catalogue(refresh=True)
            tool("music_engine_package_install", args, expected_error=True)
            assert tool("music_engine_package_status", args)["generation"] == first["generation"]
            (origin / "music.dll").write_bytes(new_library)
            from concurrent.futures import ThreadPoolExecutor
            package_key = {"kind": "music-engines", "id": engine["id"]}
            hold_library.set()
            try:
                with ThreadPoolExecutor(max_workers=1) as pool:
                    pending = pool.submit(tool, "music_engine_package_install", args, True)
                    assert library_requested.wait(10)
                    progress = tool("music_package_progress", package_key)["active"]
                    assert progress["stage"] == "downloading" and progress["file"] == "music.dll", progress
                    assert tool("music_package_cancel", package_key)["cancel_requested"]
                    pending.result(timeout=10)
                    assert tool("music_package_progress", package_key)["active"] is None
                    assert tool("music_engine_package_status", args)["generation"] == first["generation"]
            finally:
                hold_library.clear()
                release_library.set()
            request_paths.clear()
            second = tool("music_engine_package_install", args)
            assert second["downloaded_bytes"] == len(new_library) and second["reused_bytes"] == len(binary_bytes), second
            assert "/worker" not in request_paths and "/music.dll" in request_paths, request_paths
            assert second["generation"] != first["generation"]
            assert tool("music_engine_package_status", args)["version"] == "2"
            weight = b"shared model weights"
            (origin / "weights.bin").write_bytes(weight)
            model = {"id": "new-model", "name": "New model", "version": "1", "engines": [engine["id"]],
                     "files": [pinned("weights.bin", weight)]}
            sibling = dict(model, id="new-model-alt", name="Same weights, another model")
            publish(8, {"schema": 1, "engines": [engine], "models": [model, sibling]}, "music-engine-catalog.json")
            refreshed = engine_catalogue(refresh=True)
            assert refreshed["models"] == [model, sibling] and refreshed["platform"] == engine["platform"]
            request_paths.clear()
            first_model = tool("music_model_package_install", {"id": model["id"]})
            assert first_model["downloaded_bytes"] == len(weight) and "/worker" not in request_paths, first_model
            request_paths.clear()
            other = tool("music_model_package_install", {"id": sibling["id"]})
            assert other["downloaded_bytes"] == 0 and other["reused_bytes"] == len(weight) and not request_paths
            model_args = {"engine": engine["id"], "model": model["id"]}
            assert tool("music_model_package_status", model_args)["generation"] == first_model["generation"]
            tool("music_model_package_status", dict(model_args, engine="incompatible-engine"), expected_error=True)
            print("PASS new model delivered without engine update; second model reused pinned weights offline")
            print("PASS independent library update, unchanged executable reused, failed update preserved prior generation")
            assert tool("rvc_catalog_list", {"has_index": False})["voices"][0]["name"] == signed_voice["name"]
        finally:
            proc.terminate()
            proc.wait(timeout=20)
            server.shutdown()

    proc, base = start(studio)
    try:
        assert tool("rvc_catalog_list", {"has_index": False})["voices"][0]["name"] == signed_voice["name"]
        tool("rvc_catalog_refresh", expected_error=True)
        assert tool("rvc_catalog_list", {"has_index": False})["voices"][0]["name"] == signed_voice["name"]
        assert engine_catalogue()["engines"] == [engine]
        assert tool("music_engine_package_status", args)["generation"] == second["generation"]
        assert tool("music_model_package_status", model_args)["generation"] == first_model["generation"]
        active_files, = [p.parent for p in studio.rglob("music.dll") if p.parent.parent.name == second["generation"]]
        rogue = active_files / "unexpected.dll"
        rogue.write_bytes(b"unlisted")
        tool("music_engine_package_status", args, expected_error=True)
        rogue.unlink()
        assert tool("music_engine_package_status", args)["version"] == "2"
        print("PASS signed engine discovery, incompatible schema rejected, offline restart, unexpected library rejected")
        cache = studio / "component-channel" / "rvc"
        snapshot = cache / (cache / "accepted").read_text()
        path, = (snapshot / "targets").iterdir()
        path.write_bytes(path.read_bytes() + b" ")
        assert tool("rvc_catalog_list")["total"] == len(bundled["voices"])
        print("PASS signed RVC update, rollback and corruption rejected, offline restart, bundled fallback", len(bundled["voices"]))
    finally:
        proc.terminate()
        proc.wait(timeout=20)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        if sys.argv[1] == "--rvc":
            binary, certificate, tls_key = (Path(arg).resolve(strict=True) for arg in sys.argv[2:5])
            run_rvc(work, binary, certificate, tls_key, "--install-voice" in sys.argv[5:])
        else:
            binary, archive, certificate, tls_key = (Path(arg).resolve(strict=True) for arg in sys.argv[1:])
            run(work / "components", binary, archive, certificate, tls_key)
            run_rvc(work / "rvc", binary, certificate, tls_key)
