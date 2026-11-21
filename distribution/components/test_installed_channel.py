"""Linux package QA. Run in a disposable network-isolated container with loopback.

Arguments: built music-server, pinned Linux x64 ONNX 1.32 archive, TLS certificate
for localhost (trusted inside this container only), matching private TLS key.
TUF keys are generated for this run. No installed library is used.
"""
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

    class Handler(SimpleHTTPRequestHandler):
        def log_request(self, code="-", size="-"):
            request_paths.append(self.path)

    def publish(version, manifest):
        name = "rvc-voice-catalog.json"
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
