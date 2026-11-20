import concurrent.futures
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

VOICES = [("rvc-50ea3d733073550ef6f7", False), ("rvc-arkandash-faruzan-jp-v2", True)]
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

                saved_media = root / "data/media/rendered.wav"
                saved_media.parent.mkdir(exist_ok=True)
                saved_media.write_bytes(b"pre-existing rendered audio")
                store = root / "data/rvc/voices"
                results = []
                for voice_id, archived in VOICES:
                    arguments = {"query": "Faruzan" if archived else "Radio Tapok", "has_index": True}
                    if not archived:
                        arguments["vocal_style"] = "high"
                    page = mcp("rvc_catalog_list", arguments)
                    candidate = next(item for item in page["voices"] if item["id"] == voice_id)
                    assert candidate["installed_id"] is None
                    progress = []
                    started = time.monotonic()
                    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
                        pending = executor.submit(mcp, "rvc_catalog_install", {"id": voice_id}, 600)
                        while not pending.done():
                            try:
                                item = mcp("rvc_catalog_progress_get", {"id": voice_id})
                            except KeyError:
                                item = None
                            if item is not None and (not progress or progress[-1] != item):
                                progress.append(item)
                            time.sleep(.05)
                        installed = pending.result()
                    duration = time.monotonic() - started
                    assert installed["origin"]["catalog_id"] == voice_id
                    stages = {entry["stage"] for entry in progress}
                    assert ("downloading_archive" if archived else "downloading_weights") in stages, progress
                    folder = store / installed["id"]
                    model, index = folder / "voice.pth", folder / "retrieval.index"
                    hashes = {"weights": digest(model), "index": digest(index)}
                    expected = ({"weights": candidate["archive"]["weights_sha256"], "index": candidate["archive"]["index_sha256"]}
                                if archived else {key: candidate[key]["sha256"] for key in ("weights", "index")})
                    assert hashes == expected and len(list(folder.iterdir())) == 3
                    assert [item["id"] for item in mcp("rvc_voices_list", {})] == [installed["id"]]
                    assert mcp("rvc_voice_remove", {"id": installed["id"]})["removed"] is True
                    assert not folder.exists() and not list(store.iterdir())
                    assert saved_media.read_bytes() == b"pre-existing rendered audio"
                    assert mcp("rvc_voices_list", {}) == []
                    removed = next(item for item in mcp("rvc_catalog_list", arguments)["voices"] if item["id"] == voice_id)
                    assert removed["installed_id"] is None
                    results.append({"id": voice_id, "installed": installed, "hashes": hashes, "seconds": duration,
                                    "observed_stages": sorted(stages), "progress": progress, "removed": True})
                evidence = {"source_revision": revision, "server_sha256": digest(binary),
                            "results": results, "rendered_audio_preserved": True}
                output = json.dumps(evidence, ensure_ascii=True, indent=2)
                Path("rvc-zip-delete-mcp.json").write_text(output + "\n", encoding="utf-8")
                print(output)
            finally:
                process.terminate()


if __name__ == "__main__":
    run()
