import argparse
import contextlib
import ctypes
from datetime import datetime, timezone
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import ProxyHandler, Request, build_opener

BASE = "https://timoncool.github.io/YuE2-Studio/component-channel/"
HTTP = build_opener(ProxyHandler({}))


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def probe(url, http=HTTP):
    try:
        with http.open(url, timeout=12) as response:
            return hashlib.sha256(response.read()).hexdigest()
    except (URLError, TimeoutError):
        return None


@contextlib.contextmanager
def windows_offline_proxy():
    denied = []

    class RejectEgress(BaseHTTPRequestHandler):
        def reject(self):
            denied.append(self.path)
            self.send_error(502, "Outbound access blocked for installed-channel QA")

        do_CONNECT = do_GET = do_POST = reject

        def log_message(self, *_args):
            pass

    with ThreadingHTTPServer(("127.0.0.1", 0), RejectEgress) as server:
        t = threading.Thread(target=server.serve_forever, daemon=True)
        t.start()
        address = f"http://127.0.0.1:{server.server_port}"
        try:
            yield address, denied
        finally:
            server.shutdown()
            t.join()


def signed_snapshot(manifest, read):
    from tuf.api.metadata import Metadata, Root, Snapshot, Targets, Timestamp

    assert manifest["base"] == BASE
    files = {name: read(name) for name in manifest["files"]}
    for name, data in files.items():
        pin = manifest["files"][name]
        assert len(data) == pin["bytes"] and hashlib.sha256(data).hexdigest() == pin["sha256"], name

    root = Metadata.from_bytes(files["1.root.json"])
    assert isinstance(root.signed, Root) and root.signed.version == 1
    assert root.signed.consistent_snapshot
    root.verify_delegate("root", root)
    timestamp = Metadata.from_bytes(files["timestamp.json"])
    assert isinstance(timestamp.signed, Timestamp)
    snap_pin = timestamp.signed.snapshot_meta
    snapshot_bytes = files[f"{snap_pin.version}.snapshot.json"]
    snap_pin.verify_length_and_hashes(snapshot_bytes)
    snapshot = Metadata.from_bytes(snapshot_bytes)
    assert isinstance(snapshot.signed, Snapshot)
    target_pin = snapshot.signed.meta["targets.json"]
    target_bytes = files[f"{target_pin.version}.targets.json"]
    target_pin.verify_length_and_hashes(target_bytes)
    targets = Metadata.from_bytes(target_bytes)
    assert isinstance(targets.signed, Targets)
    version = manifest.get("metadata_version", 1)
    assert timestamp.signed.version == snapshot.signed.version == targets.signed.version == version
    assert snap_pin.version == target_pin.version == version

    now = datetime.now(timezone.utc)
    assert not root.signed.is_expired(now), "expired root"
    for role, metadata in (("timestamp", timestamp), ("snapshot", snapshot), ("targets", targets)):
        root.verify_delegate(role, metadata)
        assert not metadata.signed.is_expired(now), f"expired {role}"
    names = set()
    for name, target in targets.signed.targets.items():
        filename = f"targets/{target.hashes['sha256']}.{name}"
        target.verify_length_and_hashes(files[filename])
        names.add(filename)
    assert names == {name for name in files if name.startswith("targets/")}
    return {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}


def fetch_public(name):
    with HTTP.open(BASE + name, timeout=20) as response:
        assert response.geturl() == BASE + name
        return response.read()


def check(args):
    exe, work = args.executable.resolve(), args.work.resolve()
    channel = exe.parent / "component-channel"
    if sys.platform.startswith("linux") and exe.parent.parts[-2:] == ("usr", "bin"):
        candidate = exe.parent.parent / "lib/YuE2 Studio/component-channel"
        if (candidate / "root.json").is_file():
            channel = candidate
    root, locator = channel / "root.json", channel / "url.txt"
    manifest = json.loads(args.public_receipt.read_text(encoding="utf-8"))
    pins = manifest["files"]
    root_sha = pins["1.root.json"]["sha256"]
    assert sha(exe) == args.executable_sha, exe
    assert json.loads(root.read_text(encoding="utf-8")) is not None, f"{root}: null bootstrap"
    assert sha(root) == root_sha and locator.read_text(encoding="utf-8").strip() == BASE
    locator_sha = sha(locator)

    work.mkdir(parents=True, exist_ok=(args.phase == "offline"))
    profile, shared = work / "profile", work / "shared"
    names = {"HOME": "home", "USERPROFILE": "home", "LOCALAPPDATA": "local",
             "APPDATA": "roaming", "XDG_CONFIG_HOME": "xdg-config",
             "XDG_DATA_HOME": "xdg-data", "XDG_RUNTIME_DIR": "xdg-runtime"}
    local_vars = {name: str(work / folder) for name, folder in names.items()}
    for path in (profile, shared, *map(Path, local_vars.values())):
        path.mkdir(mode=0o700, exist_ok=True)

    port, engine_port = 8791, free_port()
    while port == engine_port:
        engine_port = free_port()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", port))
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(("YUE", "TIMONCOOL_"))
           and not key.lower().endswith("_proxy")
           and key.upper() not in {"SSL_CERT_FILE", "SSL_CERT_DIR", "REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE"}}
    env.update(local_vars, TIMONCOOL_SHARED_DIR=str(shared), YUE_STUDIO_DATA_ROOT=str(profile),
               YUE_STUDIO_SETTINGS_PATH=str(work / "settings.json"), YUE_STUDIO_PORT=str(port),
               YUE_ENGINE_PORT=str(engine_port), DO_NOT_TRACK="1")

    def call(route, body=None):
        request = Request(f"http://127.0.0.1:{port}{route}", body,
                          headers={"Content-Type": "application/json"})
        with HTTP.open(request, timeout=10) as response:
            return json.load(response)

    @contextlib.contextmanager
    def running(label):
        with (work / f"{label}.log").open("wb") as log:
            proc = subprocess.Popen([exe, "--headless"], cwd=work, env=env, stdout=log, stderr=log)
            try:
                until = time.monotonic() + 60
                while time.monotonic() < until and proc.poll() is None:
                    try:
                        health = call("/health")
                    except (URLError, TimeoutError):
                        time.sleep(0.2)
                        continue
                    assert health["status"] == "ok" and health["runtime"] == "native", health
                    assert Path(health["service_executable"]).samefile(exe), health
                    if sys.platform.startswith("linux"):
                        assert sha(Path(f"/proc/{proc.pid}/exe")) == args.executable_sha
                    yield health
                    return
                raise RuntimeError(f"Studio startup failed ({proc.poll()}); see {log.name}")
            finally:
                if proc.poll() is None:
                    proc.terminate()
                try:
                    proc.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()

    def runtime():
        pattern = "onnxruntime.dll" if os.name == "nt" else "libonnxruntime.so.1.32.0"
        matches = {p.resolve() for base in (profile, shared) for p in base.rglob(pattern) if p.is_file()}
        assert len(matches) == 1, matches
        return matches.pop()

    def install():
        initial = call("/v1/karaoke/install", b'{"asset_id":"onnxruntime"}')
        until = time.monotonic() + 180
        while time.monotonic() < until:
            status = call("/v1/karaoke/status").get("active_download")
            if status and status["asset_id"] == "onnxruntime" and status["done"]:
                assert not status["error"], status
                return status
            time.sleep(0.2)
        raise RuntimeError(f"Installation timed out: {initial!r}")

    receipt = work / "public-channel.json"
    if args.phase == "online":
        assert not (profile / "component-channel").exists()
        public = signed_snapshot(manifest, fetch_public)
        with running("online") as health:
            installed = install()
            assert installed["downloaded_bytes"] > 0, installed
            runtime_sha = sha(runtime())
            engines = None
            if os.name == "nt" and manifest.get("metadata_version", 1) >= 2:
                engines = call("/v1/components/music-engines/refresh", b"{}")
                assert engines["source"] == "signed" and engines["platform"] == "windows-x86_64", engines
                pinned_catalog = next(name for name in pins if name.endswith(".music-engine-catalog.json"))
                catalog_bytes = fetch_public(pinned_catalog)
                assert hashlib.sha256(catalog_bytes).hexdigest() == pins[pinned_catalog]["sha256"]
                signed_engines = json.loads(catalog_bytes)["engines"]
                expected = {entry["id"]: entry["version"] for entry in signed_engines
                            if entry["protocol"] == "music-native-windows-v2" and entry["platform"] == engines["platform"]}
                actual = {entry["id"]: entry["version"] for entry in engines["engines"]
                          if entry["protocol"] == "music-native-windows-v2" and entry["platform"] == engines["platform"]}
                assert expected.keys() >= {"acestep-cpp", "minimaxmusic-cpp"} and actual == expected
                engines = actual
            cache = {p.relative_to(profile).as_posix(): sha(p) for p in
                     (profile / "component-channel").rglob("*.json")}
            assert pins["timestamp.json"]["sha256"] in cache.values(), cache
            repeated = install()
            assert repeated["downloaded_bytes"] == 0 and sha(runtime()) == runtime_sha, repeated
        results = dict(executable=str(exe), executable_sha256=args.executable_sha,
                       packaged_root_sha256=root_sha, packaged_url_sha256=locator_sha,
                       public_http_sha256=public, trusted_cache_sha256=cache,
                       signed_metadata_version=manifest.get("metadata_version", 1),
                       signed_native_engines=engines,
                       online_health=health, install=installed, installed_repeat=repeated,
                       runtime_sha256=runtime_sha)
    else:
        mirror = ("onnxruntime-win-x64-1.32.0.zip" if os.name == "nt"
                  else "onnxruntime-linux-x64-1.32.0.tgz")
        mirror_url = "https://github.com/timoncool/onnxruntime/releases/download/v1.32.0/" + mirror
        results = json.loads(receipt.read_text(encoding="utf-8"))
        assert results["executable_sha256"] == args.executable_sha
        assert results["packaged_root_sha256"] == root_sha
        assert results["public_http_sha256"] == {name: item["sha256"] for name, item in pins.items()}
        isolation = windows_offline_proxy() if os.name == "nt" else contextlib.nullcontext((None, []))
        with isolation as (proxy, denied):
            http = HTTP
            if proxy:
                env.update(HTTP_PROXY=proxy, HTTPS_PROXY=proxy, ALL_PROXY=proxy,
                           NO_PROXY="localhost,127.0.0.1,::1")
                http = build_opener(ProxyHandler({"https": proxy}))
            for endpoint in (BASE + "timestamp.json", mirror_url):
                assert probe(endpoint, http) is None, f"public endpoint still reachable: {endpoint}"
                if proxy:
                    assert f"{urlsplit(endpoint).hostname}:443" in denied, (endpoint, denied)
            python_denials = list(denied)
            with running("offline") as health:
                if proxy:
                    try:
                        call("/v1/components/music-engines/refresh", b"{}")
                    except HTTPError as error:
                        assert error.code == 502, error.read()
                    else:
                        raise AssertionError("Studio refreshed the public catalogue while offline")
                    assert "timoncool.github.io:443" in denied[len(python_denials):], denied
                    cached = call("/v1/components/music-engines")
                    assert cached["source"] == "signed"
                    assert {item["id"]: item["version"] for item in cached["engines"]
                            if item["protocol"] == "music-native-windows-v2"} == results["signed_native_engines"]
                repeated = install()
                path = runtime()
                assert repeated["downloaded_bytes"] == 0 and sha(path) == results["runtime_sha256"]
                lib = ctypes.CDLL(str(path))
                lib.OrtGetApiBase.restype = ctypes.c_void_p
                assert lib.OrtGetApiBase()
            results.update(offline_health=health, offline_install=repeated, offline_ort_get_api_base=True)
            if proxy:
                results.update(offline_python_denials=python_denials, offline_studio_denials=denied[len(python_denials):])

    assert sha(exe) == args.executable_sha and sha(root) == root_sha and sha(locator) == locator_sha
    results["executable_sha256_after"] = sha(exe)
    receipt.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    print(receipt)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Installed Studio public-channel QA")
    parser.add_argument("phase", choices=("online", "offline", "verify"))
    parser.add_argument("--executable", type=Path)
    parser.add_argument("--executable-sha")
    parser.add_argument("--work", type=Path)
    parser.add_argument("--public-receipt", type=Path, required=True)
    parser.add_argument("--staging-dir", type=Path)
    args = parser.parse_args()
    if args.phase == "verify":
        manifest = json.loads(args.public_receipt.read_text(encoding="utf-8"))
        read = (lambda name: (args.staging_dir / name).read_bytes()) if args.staging_dir else fetch_public
        verified = signed_snapshot(manifest, read)
        print(json.dumps({"origin": str(args.staging_dir) if args.staging_dir else BASE,
                          "metadata_version": manifest.get("metadata_version", 1),
                          "files_verified": len(verified), "sha256": verified}, indent=2))
    else:
        if not args.executable or not args.executable_sha or not args.work or args.staging_dir:
            parser.error("online and offline require --executable, --executable-sha and --work")
        check(args)
