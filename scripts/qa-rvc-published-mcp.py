import hashlib
import json
import os
import socket
import subprocess
import tempfile
import time
from pathlib import Path
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener

VOICE = "rvc-50ea3d733073550ef6f7"
HTTP = build_opener(ProxyHandler({}))


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def run():
    binary = Path("target/debug/music-server.exe").resolve()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD^"], text=True).strip()
    studio, engine_port = port(), port()
    base = f"http://127.0.0.1:{studio}"
    with tempfile.TemporaryDirectory(prefix="yue-rvc-published-") as folder:
        root = Path(folder)
        (root / "data").mkdir()
        env = dict(os.environ, YUE_STUDIO_PORT=str(studio), YUE_ENGINE_PORT=str(engine_port),
                   YUE_STUDIO_DATA_ROOT=str(root / "data"), YUE_ENGINE_BIN=str(root / "absent-yue-server.exe"),
                   TIMONCOOL_SHARED_DIR=str(root / "registry"), DO_NOT_TRACK="1", STUDIO_HUB_URL=f"{base}/fixture-hub")
        requests = 0

        def mcp(name, arguments, timeout=30):
            nonlocal requests
            requests += 1
            payload = {"jsonrpc": "2.0", "id": requests, "method": "tools/call",
                       "params": {"name": name, "arguments": arguments}}
            request = Request(f"{base}/mcp", json.dumps(payload).encode(),
                              {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"})
            with HTTP.open(request, timeout=timeout) as response:
                result = json.load(response)
            assert "error" not in result and not result["result"].get("isError"), result
            return result["result"]["structuredContent"]

        with (root / "studio.log").open("wb") as log, subprocess.Popen(
            [str(binary)], env=env, stdout=log, stderr=subprocess.STDOUT
        ) as process:
            try:
                for _ in range(120):
                    assert process.poll() is None, process.returncode
                    try:
                        with HTTP.open(f"{base}/health", timeout=1):
                            break
                    except URLError:
                        time.sleep(.25)
                else:
                    raise TimeoutError("Studio service did not open its MCP endpoint")

                arguments = {
                    "query": "Radio Tapok", "vocal_style": "high", "rvc_version": "v2",
                    "sample_rate_hz": 40000, "has_index": True,
                }
                started = time.monotonic()
                page = mcp("rvc_catalog_list", arguments)
                elapsed = time.monotonic() - started
                assert requests == 1
                assert page["total"] == 1 and page["next_offset"] is None, page
                voice, = page["voices"]
                assert voice["id"] == VOICE
                assert voice["metadata"]["vocal_style"] == "high"
                assert "Radio Tapok" in voice["metadata"]["source_labels"]
                evidence = {"source_revision": revision, "server_sha256": digest(binary),
                            "mcp_calls": requests, "arguments": arguments, "response": page,
                            "seconds": elapsed}
                output = json.dumps(evidence, ensure_ascii=True, indent=2)
                Path("rvc-filtered-mcp.json").write_text(output + "\n", encoding="utf-8")
                print(output)
            finally:
                process.terminate()


if __name__ == "__main__":
    run()
