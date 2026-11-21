from pathlib import Path
from urllib.request import ProxyHandler, Request, build_opener
from urllib.error import URLError
import hashlib
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time

binary_path = Path("studio/target/debug/music-server.exe").resolve()
ref = subprocess.check_output(["git", "-C", "studio", "rev-parse", "HEAD"], text=True).strip()
assert ref == "30505a3eef88b77f3975e2ac6cba5afb5416cc80"
http = build_opener(ProxyHandler({}))
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
base = f"http://127.0.0.1:{port}"
with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as folder:
    root = Path(folder)
    assert shutil.disk_usage(root).free >= 3 * 1024**3
    env = dict(os.environ, YUE_STUDIO_PORT=str(port), YUE_STUDIO_DATA_ROOT=str(root / "data"),
               YUE_ENGINE_BIN=str(root / "missing-yue-server.exe"), TIMONCOOL_SHARED_DIR=str(root / "shared"),
               DO_NOT_TRACK="1", STUDIO_HUB_URL=base + "/fixture-hub")
    env.pop("YUE_AUDIOCPP_SERVER", None)

    def request(route, data=None, method=None):
        body = None if data is None else json.dumps(data).encode()
        headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
        with http.open(Request(base + route, body, headers, method=method), timeout=30) as reply:
            return json.load(reply)

    def mcp(name, arguments):
        payload = {"jsonrpc":"2.0", "id":1, "method":"tools/call",
                   "params": {"name":name, "arguments":arguments}}
        response = request("/mcp", payload)["result"]
        assert not response.get("isError"), response
        return response.get("structuredContent") or json.loads(response["content"][0]["text"])

    with (root / "studio.log").open("wb") as log, subprocess.Popen([binary_path], env=env, stdout=log, stderr=subprocess.STDOUT) as studio:
        try:
            for _ in range(120):
                assert studio.poll() is None
                try:
                    engine = request("/engine/options")["options"]
                    break
                except URLError:
                    time.sleep(.25)
            else:
                raise TimeoutError("Studio")
            engine["backend"] = "cpu"
            request("/engine/options", engine, "PUT")
            catalog = lambda: next(row for row in mcp("audio_cpp_music_models", {})["models"] if row["id"] == "soulx_singer")
            assert not catalog()["ready"]
            mcp("audio_cpp_music_model_install", {"model":"soulx_singer"})
            for _ in range(420):
                row = catalog()
                if not row["installing"]:
                    assert row["ready"], row
                    break
                time.sleep(1)
            else:
                raise TimeoutError(row)
            binary, = (root/"data").rglob("audiocpp_server.exe")
            build = subprocess.check_output([binary,"--version"], cwd=binary.parent, text=True).strip()
            assert build.startswith("audio.cpp 41242418") and "backends: cpu" in build
            with (root/"data/audiocpp/models/soulx_singer/soulx-singer-q8_0.gguf").open("rb") as checkpoint:
                digest = hashlib.file_digest(checkpoint,"sha256").hexdigest()
            assert digest == "c146c09427f88202db37a71869cdd498e6c0e92fa2cd08065bd5c2f735eb82fd"
            record = {"studio_revision":ref, "installed_by_mcp":True, "runtime_override":False,
                      "verified_runtime_version":build, "weights_sha256":digest, "catalog_ready":row["ready"]}
            output = json.dumps(record,indent=2)
            Path("soulx-published-install.json").write_text(output,encoding="utf-8")
            print(output)
        finally:
            if studio.poll() is None:
                subprocess.run(["taskkill", "/PID", str(studio.pid), "/T", "/F"],capture_output=True)
            studio.wait(timeout=20)

