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
    install = work / "install"
    origin = work / "origin"
    channel = install / "component-channel"
    channel.mkdir(parents=True)
    (origin / "targets").mkdir(parents=True)
    exe = Path(shutil.copy2(binary, install))
    shutil.copyfile(archive, origin / "current.tgz")
    requested = []

    class Handler(SimpleHTTPRequestHandler):
        def log_request(self, code="-", size="-"):
            requested.append((self.path, int(code)))

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

        def publish(revision, filename):
            name = "mirrors.json"
            data = json.dumps({"schema": 1, "revision": revision, "archives": {DIGEST: {"sources": [base + filename]}}}).encode()
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

            def install_runtime():
                req = Request(url + "/v1/karaoke/install", b'{"asset_id":"onnxruntime"}', {"Content-Type": "application/json"})
                with http.open(req, timeout=180) as response:
                    json.load(response)
                deadline = time.monotonic() + 180
                while time.monotonic() < deadline:
                    with http.open(url + "/v1/karaoke/status", timeout=10) as response:
                        job = json.load(response).get("active_download")
                    if job and job["asset_id"] == "onnxruntime" and job["done"]:
                        return job
                    time.sleep(0.25)
                raise TimeoutError("ONNX install job")

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
            second = install_runtime()
            assert not second["error"], second
            assert ("/current.tgz", 200) in requested, requested
            assert any(p.is_file() for p in data.rglob("libonnxruntime.so*"))
            print("PASS same service PID", proc.pid, "old URL 404 -> signed new URL -> installed ONNX", DIGEST)
        finally:
            proc.terminate()
            proc.wait(timeout=20)
            server.shutdown()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as temp:
        run(Path(temp), *(Path(arg).resolve(strict=True) for arg in sys.argv[1:]))
