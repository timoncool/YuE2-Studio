"""Verify H3's manual installation status through the actual Studio MCP transport."""
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path


binary = Path(sys.argv[1]).resolve(strict=True)
port = int(sys.argv[2])
base = f"http://127.0.0.1:{port}"
client = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def rpc(identifier, method, params):
    body = json.dumps(dict(jsonrpc="2.0", id=identifier, method=method, params=params)).encode()
    req = urllib.request.Request(base + "/mcp", data=body, headers={
        "Content-Type": "application/json", "Accept": "application/json, text/event-stream",
    })
    with client.open(req, timeout=10) as response:
        raw = response.read().decode()
    if raw.startswith(("event:", "data:")):
        raw = next(line[6:] for line in raw.splitlines() if line.startswith("data: "))
    reply = json.loads(raw)
    assert "error" not in reply, reply
    return reply["result"]


with tempfile.TemporaryDirectory(prefix="yue-h3-mcp-") as folder:
    root = Path(folder)
    env = dict(os.environ, YUE_STUDIO_PORT=str(port), YUE_STUDIO_DATA_ROOT=folder,
               TIMONCOOL_SHARED_DIR=str(root / "registry"), YUE_AUDIOCPP_SERVER=str(root / "absent"),
               DO_NOT_TRACK="1")
    with (root / "service.log").open("w+") as log:
        service = subprocess.Popen([binary], env=env, cwd=folder, stdout=log, stderr=subprocess.STDOUT)
        try:
            for attempt in range(100):
                if service.poll() is not None:
                    raise RuntimeError(f"Studio exited {service.returncode}")
                try:
                    with client.open(base + "/health", timeout=1):
                        break
                except OSError:
                    time.sleep(.1)
            else:
                raise TimeoutError("Studio health")
            tools = rpc(1, "tools/list", {})["tools"]
            tool = next(item for item in tools if item["name"] == "audio_cpp_music_models")
            assert "Manual profiles" in tool["description"], tool
            reply = rpc(2, "tools/call", {"name": tool["name"], "arguments": {}})
            assert not reply.get("isError"), reply
            payload = reply.get("structuredContent") or json.loads(reply["content"][0]["text"])
            model = next(item for item in payload["models"] if item["id"] == "minimax_h3")
            assert model["ready"] is False and model["status"] == "experimental", model
            assert model["missing_bytes"] == model["bytes"] == 32_443_213_394, model
            assert "dit.gguf" in model["missing"], model
            assert model["manual_install"] == {
                "source_url": "https://huggingface.co/audio-cpp/audio.cpp-gguf/tree/32c1ce1fea57a513eb9386aa62408d47b1cf7ab2/MiniMax-H3-Q4-GGUF",
                "license_url": "https://huggingface.co/MiniMaxAI/MiniMax-H3/blob/main/LICENSE",
                "directory": "audiocpp/models/MiniMax-H3-Q4-GGUF",
            }, model
            print(json.dumps(model))
        except BaseException:
            log.seek(0)
            print(log.read(), file=sys.stderr)
            raise
        finally:
            service.terminate()
            service.wait(timeout=10)
