"""Controlled TLS mirrors on an exact Windows NSIS installation."""
import argparse
from contextlib import ExitStack, contextmanager
import ctypes
from cryptography import x509
from cryptography.hazmat.primitives import hashes
from datetime import datetime, timedelta, timezone
from functools import partial
import hashlib
from http.server import BaseHTTPRequestHandler, SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import shutil
import os
from pathlib import Path
import socket
import ssl
import subprocess
import threading
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener

from securesystemslib.signer import CryptoSigner
from tuf.api.metadata import MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp
from test_hip_mirrors import tls_files
from test_installed_channel import sign

ARCHIVE_SHA = "aaa1d04fcf86bd31e84b90bc0cbde80e632bd0720dcc5d8ba05c4fe45c09d127"
HTTP = build_opener(ProxyHandler({}))


def sha(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class FixtureServer(ThreadingHTTPServer):
    stopped = False

    def stop(self):
        if not self.stopped:
            self.shutdown()
            self.server_close()
            self.stopped = True


@contextmanager
def hosted_trust(ca):
    # Windows' platform verifier does not honor SSL_CERT_FILE.
    assert os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"
    cert = x509.load_pem_x509_certificate(ca.read_bytes())
    thumbprint = cert.fingerprint(hashes.SHA1()).hex()
    subprocess.run(["certutil.exe", "-user", "-addstore", "Root", str(ca)], check=True)
    try:
        yield
    finally:
        subprocess.run(["certutil.exe", "-user", "-delstore", "Root", thumbprint], check=True)


def check(work, installed, archive, receipt, revision, installer_sha, executable_sha):
    assert os.name == "nt"
    assert sha(archive) == ARCHIVE_SHA
    work.mkdir(parents=True)
    data = work / "profile"
    isolated = work / "installation"
    shutil.copytree(installed, isolated, ignore=shutil.ignore_patterns("data"))
    executable = isolated / "YuE2-Studio.exe"
    original = installed / executable.name
    assert sha(original) == sha(executable) == executable_sha
    channel = isolated / "component-channel"
    bootstrap = {name: sha(channel / name) for name in ("root.json", "url.txt")}
    assert json.loads((channel / "root.json").read_bytes()), "missing production bootstrap"
    binary_sha = executable_sha
    assert sha(channel / "root.json") == "6115060a91903955485aefaf4fbacfee0ec1292053d819be2ed98cdf4cce2a83"
    tls_files(work)
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain(work / "tls.pem", work / "tls.key")

    channel_origin = work / "channel-origin"
    (channel_origin / "targets").mkdir(parents=True)
    mirror_origin = work / "mirror-origin"
    mirror_origin.mkdir()
    shutil.copyfile(archive, mirror_origin / "current.zip")
    corrupt = mirror_origin / "wrong-sha.zip"
    body = bytearray(archive.read_bytes())
    body[-1] ^= 1
    corrupt.write_bytes(body)
    assert corrupt.stat().st_size == archive.stat().st_size and sha(corrupt) != ARCHIVE_SHA
    report = {
        "installer_sha256": installer_sha, "source_revision": revision,
        "installed_executable_sha256_before": binary_sha,
        "original_packaged_bootstrap_sha256": bootstrap,
        "test_ca_sha256": sha(work / "ca.pem"), "expected_archive_sha256": ARCHIVE_SHA,
        "corrupt_same_size_archive_sha256": sha(corrupt), "archive_bytes": archive.stat().st_size,
        "external_network_isolation": "per-profile deny proxy, loopback fixtures only",
        "cases": {},
    }
    requested = []
    cut_timestamp = False

    class DenyProxy(BaseHTTPRequestHandler):
        def do_CONNECT(self):
            requested.append({"server": "proxy", "method": "CONNECT", "path": self.path, "status": 403})
            self.send_error(403, "QA blocks all external traffic")

        def log_message(self, *_args):
            pass

    class Handler(SimpleHTTPRequestHandler):
        def log_request(self, code="-", size="-"):
            requested.append({"server": self.server.label, "method": self.command, "path": self.path,
                              "status": int(code), "range": self.headers.get("Range")})

        def do_GET(self):
            if self.server.label != "channel" or self.path != "/timestamp.json" or not cut_timestamp:
                return super().do_GET()
            content = (channel_origin / "timestamp.json").read_bytes()
            self.send_response(200)
            self.send_header("Content-Length", str(len(content)))
            self.end_headers()
            self.wfile.write(content[:len(content) // 2])
            self.wfile.flush()
            self.close_connection = True

    keys = {role: CryptoSigner.generate_ed25519() for role in ("root", "targets", "snapshot", "timestamp")}
    expires = datetime.now(timezone.utc) + timedelta(hours=1)
    root = Root(expires=expires)
    for role, key in keys.items():
        root.add_key(key.public_key, role)
    (channel / "root.json").write_bytes(sign(root, keys["root"]))
    report["temporary_tuf_root_sha256"] = sha(channel / "root.json")

    def start_server(stack, label, folder):
        server = FixtureServer(("127.0.0.1", 0), partial(Handler, directory=folder))
        server.label = label
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        stack.callback(server.stop)
        return server, f"https://localhost:{server.server_port}/"

    def publish(revision, source):
        name = "mirrors.json"
        raw = json.dumps({"schema": 1, "revision": revision,
                          "archives": {ARCHIVE_SHA: {"sources": [source]}}}).encode()
        target = TargetFile.from_data(name, raw, ["sha256"])
        (channel_origin / "targets" / f"{target.hashes['sha256']}.{name}").write_bytes(raw)
        targets = sign(Targets(version=revision, expires=expires, targets={name: target}), keys["targets"])
        (channel_origin / f"{revision}.targets.json").write_bytes(targets)
        snapshot = sign(Snapshot(version=revision, expires=expires, meta={
            "targets.json": MetaFile.from_data(revision, targets, ["sha256"]),
        }), keys["snapshot"])
        (channel_origin / f"{revision}.snapshot.json").write_bytes(snapshot)
        (channel_origin / "timestamp.json").write_bytes(sign(Timestamp(
            version=revision, expires=expires, snapshot_meta=MetaFile.from_data(revision, snapshot, ["sha256"]),
        ), keys["timestamp"]))

    data.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.lower().endswith("_proxy")}
    for name in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TIMONCOOL_SHARED_DIR", "WEBVIEW2_USER_DATA_FOLDER"):
        path = work / name.lower()
        path.mkdir(mode=0o700)
        env[name] = str(path)
    studio_port = 8791
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", studio_port))
    env.update(YUE_ENGINE_PORT=str(port()),
               YUE_STUDIO_DATA_ROOT=str(data), YUE_STUDIO_SETTINGS_PATH=str(work / "settings.json"),
               DO_NOT_TRACK="1")
    api = f"http://127.0.0.1:{studio_port}"

    def call(route, body=None, method=None):
        request = Request(api + route, body, method=method, headers={"Content-Type": "application/json"})
        with HTTP.open(request, timeout=10) as answer:
            return json.load(answer)

    @contextmanager
    def running(phase):
        with (receipt.parent / f"controlled-channel-{phase}.log").open("wb") as log:
            process = subprocess.Popen([executable, "--headless"], env=env, cwd=work,
                                       stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 45
                while True:
                    assert time.monotonic() < deadline and process.poll() is None, (
                        phase, process.poll(), (receipt.parent / f"controlled-channel-{phase}.log").read_text(errors="replace")[-4000:])
                    try:
                        health = call("/health")
                        break
                    except URLError:
                        time.sleep(0.2)
                assert health["status"] == "ok" and health["runtime"] == "native"
                assert Path(health["service_executable"]).samefile(executable), health
                assert sha(Path(health["service_executable"])) == binary_sha
                report[phase] = {"health": health, "pid": process.pid, "running_binary_sha256": binary_sha}
                yield
                assert process.poll() is None and sha(executable) == binary_sha
            finally:
                process.terminate()
                process.wait(timeout=15)

    def runtime():
        return next((path for root in (data, Path(env["TIMONCOOL_SHARED_DIR"]))
                     for path in root.rglob("onnxruntime.dll") if path.is_file()), None)

    def install(label, error, expected_path=None, expected_status=None):
        start = len(requested)
        initial = call("/v1/karaoke/install", b'{"asset_id":"onnxruntime"}')
        deadline = time.monotonic() + 60
        while True:
            result = call("/v1/karaoke/status").get("active_download")
            if result and result["asset_id"] == "onnxruntime" and result["done"]:
                break
            assert time.monotonic() < deadline, (label, initial, result, requested[start:])
            time.sleep(0.1)
        hits = requested[start:]
        report["cases"][label] = {"result": result, "http_requests": hits}
        assert bool(result["error"]) == error, (label, result, hits)
        if expected_path:
            assert any(hit["path"] == expected_path and hit["status"] == expected_status for hit in hits), (label, hits)
        assert bool(runtime()) != error, (label, runtime())
        print(label, result["error"] or "ok", len(hits), flush=True)
        return result, hits

    try:
        with ExitStack() as stack:
            stack.enter_context(hosted_trust(work / "ca.pem"))
            proxy = FixtureServer(("127.0.0.1", 0), DenyProxy)
            threading.Thread(target=proxy.serve_forever, daemon=True).start()
            stack.callback(proxy.stop)
            proxy_url = f"http://127.0.0.1:{proxy.server_port}"
            env["HTTPS_PROXY"] = env["HTTP_PROXY"] = proxy_url
            env["NO_PROXY"] = "localhost,127.0.0.1,127.0.0.2"
            channel_server, base = start_server(stack, "channel", channel_origin)
            mirror, mirror_url = start_server(stack, "mirror", mirror_origin)
            (channel / "url.txt").write_text(base)
            publish(1, mirror_url + "retired.zip")
            with running("online"):
                configured = {"mode": "custom", "address": proxy_url, "kind": "http", "huggingface_mirror": False}
                call("/v1/network/proxy", json.dumps(configured).encode(), method="PUT")
                assert call("/v1/network/proxy")["address"] == proxy_url
                _, hits = install("retired_mirror_404", True, "/retired.zip", 404)
                assert any(hit["server"] == "proxy" for hit in hits), hits
                disabled = f"https://127.0.0.2:{port()}/blocked.zip"
                publish(2, disabled)
                result, hits = install("unreachable_mirror_host", True, "/2.targets.json", 200)
                assert not any(hit["server"] == "mirror" for hit in hits), hits
                report["cases"]["unreachable_mirror_host"]["signed_url"] = disabled
                publish(3, mirror_url + "wrong-sha.zip")
                result, _ = install("wrong_sha_same_size", True, "/wrong-sha.zip", 200)
                assert any(term in result["error"].lower() for term in ("sha", "pinned", "checksum")), result
                publish(4, mirror_url + "current.zip")
                cut_timestamp = True
                _, hits = install("truncated_timestamp", True, "/timestamp.json", 200)
                assert not any(hit["server"] == "mirror" for hit in hits), hits
                cut_timestamp = False
                install("signed_replacement_install", False, "/current.zip", 200)
                final_library_sha = sha(runtime())
                result, hits = install("installed_repeat_zero_http", False)
                assert not any(hit["server"] != "proxy" for hit in hits) and result["downloaded_bytes"] == 0, (hits, result)
                mirror.stop()
                channel_server.stop()
                proxy.stop()
            with running("offline_restart"):
                assert call("/v1/network/proxy")["address"] == proxy_url
                result, hits = install("offline_after_restart", False)
                assert not hits and result["downloaded_bytes"] == 0 and sha(runtime()) == final_library_sha, (hits, result)
                lib = ctypes.CDLL(str(runtime()))
                lib.OrtGetApiBase.restype = ctypes.c_void_p
                assert lib.OrtGetApiBase()
                report["offline_runtime_library_sha256"] = final_library_sha
                report["offline_runtime_ort_get_api_base"] = True
        assert sha(original) == sha(executable) == binary_sha
        assert {name: sha(installed / "component-channel" / name) for name in bootstrap} == bootstrap
        report["installed_executable_sha256_after"] = sha(executable)
        report["passed"] = True
    finally:
        receipt.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--installed", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--installer-sha", required=True)
    parser.add_argument("--executable-sha", required=True)
    check(**vars(parser.parse_args()))
