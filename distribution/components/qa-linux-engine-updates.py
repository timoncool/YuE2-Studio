"""Linux Studio release candidate acceptance through public HTTP/MCP."""
import contextlib
import functools
import hashlib
import json
import os
import socket
import ssl
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta, timezone
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import ProxyHandler, Request, build_opener

from securesystemslib.signer import CryptoSigner
from tuf.api.metadata import MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp
from test_signed_music_runtime import sign

binary, work, cert, key = map(lambda p: Path(p).resolve(), sys.argv[1:5])
strict = "--strict" in sys.argv[5:]
engine, model = "qa-update-engine", "qa-update-model"
target_name = "music-engine-catalog.json"
data = work / "data"
origin = work / "origin"
origin.mkdir(parents=True)
(origin / "targets").mkdir()
boot = work / "bin/component-channel"
boot.mkdir(parents=True)
studio = boot.parent / "music-server"
os.link(binary, studio)
client = build_opener(ProxyHandler({}))
keys = {name: CryptoSigner.generate_ed25519() for name in ("root", "targets", "snapshot", "timestamp")}
expires = datetime.now(timezone.utc) + timedelta(days=1)
root = Root(expires=expires)
for name, signer in keys.items():
    root.add_key(signer.public_key, name)
trust = sign(root, keys["root"])
(boot / "root.json").write_bytes(trust)
(origin / "1.root.json").write_bytes(trust)
template = Path(__file__).with_name("qa-engine-update-adapter.py.txt").read_text(encoding="utf8")
events = work / "workers.jsonl"
gate = work / "finish-job"
download_release = threading.Event()
download_seen = threading.Event()
http_hits = []
report, calls = {}, []

def digest(body):
    return hashlib.sha256(body).hexdigest()

class Channel(SimpleHTTPRequestHandler):
    def do_GET(self):
        http_hits.append(self.path)
        if self.path == "/slow":
            self.send_response(200)
            self.send_header("Content-Length", str(len(payloads[3])))
            self.end_headers()
            self.wfile.write(payloads[3][:128])
            self.wfile.flush()
            download_seen.set()
            download_release.wait(30)
            try:
                self.wfile.write(payloads[3][128:])
            except (OSError, ssl.SSLError):
                pass
            return
        super().do_GET()

payloads = {version: template.replace("@VERSION@", f"{version}.0").replace("@GATE@", str(gate))
            .replace("@EVENTS@", str(events)).encode()
            for version in range(1, 5)}
for version, payload in payloads.items():
    (origin / str(version)).write_bytes(payload if version != 4 else b"x" * len(payload))
weight = b"shared model bytes"
(origin / "weight").write_bytes(weight)

def publish(version):
    def pin(name, body, file):
        return {"name": name, "sha256": digest(body), "bytes": len(body), "url": f"{base}/{file}"}
    adapter = "music-adapter"
    workers = [{"id": engine, "name": "QA engine", "platform": "linux-x86_64",
                "version": f"{version}.0", "protocol": "music-http-v1", "executable": adapter,
                "files": [pin(adapter, payloads[version], "slow" if version == 3 else str(version))]}]
    models = [{"id": model, "name": "QA model", "engines": [engine],
               "version": "1.0", "files": [pin("weight.bin", weight, "weight")]}]
    signed = json.dumps({"schema": 1, "engines": workers, "models": models}).encode()
    target = TargetFile.from_data(target_name, signed, ["sha256"])
    (origin / "targets" / f"{target.hashes['sha256']}.{target_name}").write_bytes(signed)
    targets = sign(Targets(version=version, expires=expires, targets={target_name: target}), keys["targets"])
    (origin / f"{version}.targets.json").write_bytes(targets)
    snap = sign(Snapshot(version=version, expires=expires,
                meta={"targets.json": MetaFile.from_data(version, targets, ["sha256"])}), keys["snapshot"])
    (origin / f"{version}.snapshot.json").write_bytes(snap)
    (origin / "timestamp.json").write_bytes(sign(Timestamp(version=version, expires=expires,
        snapshot_meta=MetaFile.from_data(version, snap, ["sha256"])), keys["timestamp"]))

@contextlib.contextmanager
def start(logname):
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    env = dict(os.environ, YUE_STUDIO_PORT=str(port), YUE_STUDIO_DATA_ROOT=str(data),
               TIMONCOOL_SHARED_DIR=str(work / "shared"), DO_NOT_TRACK="1",
               STUDIO_HUB_URL="http://127.0.0.1:1", NO_PROXY="localhost,127.0.0.1",
               YUE_QA_CHANNEL_CA_FILE=os.environ["SSL_CERT_FILE"])
    def call(path, body=None, timeout=30):
        request = Request(f"http://127.0.0.1:{port}{path}", data=None if body is None else json.dumps(body).encode(),
                          headers={"Content-Type":"application/json", "Accept":"application/json,text/event-stream"})
        with client.open(request, timeout=timeout) as response:
            return response.read()
    def tool(name, args=None, error=False):
        payload = {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args or {}}}
        result = json.loads(call("/mcp", payload))["result"]
        calls.append({"tool":name, "arguments":args or {}, "result":result})
        assert bool(result.get("isError")) == error, (name, args, result)
        return result.get("structuredContent", result)
    with (work / logname).open("w") as output:
        process = subprocess.Popen([studio], cwd=studio.parent, env=env, stdout=output, stderr=output, start_new_session=True)
        try:
            for _ in range(180):
                assert process.poll() is None
                try:
                    call("/health", timeout=1)
                    break
                except (URLError, TimeoutError):
                    time.sleep(.2)
            else:
                raise TimeoutError("service startup")
            yield call, tool
        finally:
            os.killpg(process.pid, 15)
            process.wait(timeout=15)

def await_until(read, accept, timeout=20):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = read()
        if accept(value):
            return value
        time.sleep(.2)
    raise TimeoutError(value)

def runs():
    return [json.loads(line) for line in events.read_text().splitlines()] if events.exists() else []

source = {"kind":"signed", "engine":engine, "model":model}
prompt = {"title":"Update QA","style":"ambient test fixture","lyrics":"","duration_seconds":4,"steps":8,
          "seed":42,"cot":"off","output_format":"wav32","music_source":source}
id_arg = {"id": engine}
progress_args = id_arg | {"kind":"music-engines"}
status_path = f"/v1/components/music-engines/{engine}/status?probe=presence"
active_path = data / "music-engines" / engine / "active.json"

def completed(tool, call, job, version):
    job = await_until(lambda: tool("song_job_get", {"job_id":job["id"],"response_format":"detailed"}),
                      lambda j: j["status"] in ("completed","failed","cancelled"))
    assert job["status"] == "completed", job
    song = tool("library_song_get", {"song_id":job["song"]["id"],"response_format":"detailed"})
    assert song["generation_settings"]["adapter_settings"]["version"] == f"{version}.0", song
    assert song["replay_request"]["signed_engine"]["version"] == f"{version}.0", song
    wav = call(f"/v1/library/media/{song['id']}")
    assert wav[:4] == b"RIFF" and len(wav) > 32000
    return {"id": song["id"], "reported": song["replay_request"]["signed_engine"],
            "worker": song["generation_settings"]["adapter_settings"], "wav_sha256": digest(wav)}

try:
    with ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Channel, directory=origin)) as origin_server:
        base = f"https://localhost:{origin_server.server_port}"
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(cert, key)
        origin_server.socket = tls.wrap_socket(origin_server.socket, server_side=True)
        (boot / "url.txt").write_text(base + "/", encoding="utf8")
        threading.Thread(target=origin_server.serve_forever, daemon=True).start()
        with start("studio.log") as (call, tool), ThreadPoolExecutor(max_workers=2) as pool:
            status = lambda: json.loads(call(status_path))
            refresh = lambda: tool("music_engine_catalog_refresh")
            publish(1)
            refresh()
            first = tool("music_engine_package_install", id_arg)
            try:
                tool("music_model_package_status", {"engine":engine,"model":model}, error=True)
            finally:
                tool("music_model_package_install", {"id":model})
            start_a = tool("song_create", prompt)
            worker = await_until(runs, lambda hits: any(item["event"] == "submitted" for item in hits))[0]
            assert worker["version"] == "1.0" and Path(f"/proc/{worker['pid']}").exists(), worker
            publish(2)
            refresh()
            planned = tool("music_package_updates_get")
            assert len(planned["updates"]) == 1 and planned["updates"][0]["changed_bytes"] == len(payloads[2]), planned
            install = pool.submit(tool, "music_engine_package_install", id_arg)
            await_until(lambda: install.done() or
                        tool("music_package_progress", progress_args).get("active", {}).get("stage") == "waiting_for_idle"
                        if tool("music_package_progress", progress_args).get("active") else install.done(), bool)
            now = {"old_worker":worker, "old_pid_alive":Path(f"/proc/{worker['pid']}").exists(),
                   "active":json.loads(active_path.read_text()), "shown":status(), "update_returned":install.done()}
            concurrent = tool("song_create", prompt | {"seed":44}, error=True)
            assert now["old_pid_alive"] and "busy" in json.dumps(concurrent), (now, concurrent)
            report.update({"during_update":now,"parallel_submission":concurrent,"update_plan":planned})
            gate.touch()
            a = completed(tool, call, start_a, 1)
            second = install.result(timeout=20)
            assert first["generation"] != second["generation"] and status()["version"] == "2.0"
            b = completed(tool, call, tool("song_create", prompt | {"seed":45}), 2)
            assert a["worker"]["pid"] != b["worker"]["pid"]
            assert a["reported"]["generation"] == first["generation"] and b["reported"]["generation"] == second["generation"]
            assert tool("music_package_updates_get")["updates"] == []
            report["takes"] = [a,b]
            publish(3)
            refresh()
            canceled = pool.submit(tool, "music_engine_package_install", id_arg, True)
            assert download_seen.wait(12)
            active = tool("music_package_progress", progress_args)["active"]
            assert active["stage"] == "downloading", active
            assert tool("music_package_cancel", progress_args)["cancel_requested"]
            abort = canceled.result(timeout=15)
            assert "cancelled" in json.dumps(abort).lower() and status()["version"] == "2.0"
            download_release.set()
            publish(4)
            refresh()
            corrupt = tool("music_engine_package_install", id_arg, error=True)
            assert "sha-256" in json.dumps(corrupt).lower() and status()["version"] == "2.0", corrupt
            report.update({"cancel":abort, "corrupt":corrupt, "retained":status()})
        origin_server.shutdown()
    with start("offline.log") as (call, tool):
        assert json.loads(call(status_path))["version"] == "2.0"
        c = completed(tool, call, tool("song_create", prompt | {"seed":46}), 2)
        report["offline"] = c
        assert tool("music_engine_catalog_list")["engines"]
    report["downloads"] = http_hits
    assert http_hits.count("/weight") == 1, http_hits
    if strict:
        assert not now["update_returned"] and now["shown"]["version"] == "1.0", now
        assert now["active"]["active"] == first["generation"], now
finally:
    for name, value in (("receipt.json", report), ("mcp.json", calls)):
        (work / name).write_text(json.dumps(value, indent=2), encoding="utf8")
