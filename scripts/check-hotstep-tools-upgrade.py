"""Install a pinned Windows trainer archive via MCP, restart, and adopt it again."""
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener


WEIGHTS = (
    "yue2_3b_int8_convrot.safetensors", "yue2-vae-standard-f32.gguf",
    "yue2-tok-f16.gguf", "sheetsage2-f16.gguf", "mms-fa-f32.gguf",
)


def sha(file):
    with file.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def run(work, binary, folder, previous_data, new_tag):
    work.mkdir()
    data = work / "data"
    models = data / "training/models"
    models.mkdir(parents=True)
    source_models = previous_data / "training/models"
    for name in WEIGHTS:
        os.link(source_models / name, models / name)
    old_instance = data / "hotstep/hotstep-tools-75923e70"
    old_source = previous_data / "hotstep/hotstep-tools-75923e70"
    shutil.copytree(old_source, old_instance, copy_function=os.link)
    checkpoint = data / "training/runs/upgrade-fixture/checkpoints/step-42.safetensors"
    checkpoint.parent.mkdir(parents=True)
    checkpoint.write_bytes(b"checkpoint marker for step 42")
    before = {name: sha(models / name) for name in WEIGHTS}
    checkpoint_hash = sha(checkpoint)
    old_exe_hash = sha(old_source / "music-train.exe")
    client = build_opener(ProxyHandler({}))
    rpc = []

    @contextlib.contextmanager
    def studio(label):
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        env = dict(os.environ, YUE_STUDIO_PORT=str(port), YUE_STUDIO_DATA_ROOT=str(data), DO_NOT_TRACK="1",
                   STUDIO_HUB_URL="http://127.0.0.1:1", TIMONCOOL_SHARED_DIR=str(work / "shared"))
        for name in ("YUE_TRAIN_BIN", "YUE_CAPTION_BIN"):
            env.pop(name, None)

        def call(path, body=None):
            request = Request(f"http://127.0.0.1:{port}{path}",
                              None if body is None else json.dumps(body).encode(),
                              headers={"Content-Type": "application/json", "Accept": "application/json, text/event-stream"})
            with client.open(request, timeout=180) as response:
                return json.load(response)
        with (work / f"{label}.log").open("w") as log:
            process = subprocess.Popen([binary], env=env, stdout=log, stderr=log)
            try:
                for _ in range(150):
                    try:
                        call("/health")
                        break
                    except URLError:
                        assert process.poll() is None
                        time.sleep(.2)
                else:
                    raise TimeoutError("Studio startup")
                yield call
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=20)

    def adopt(call):
        request = {"jsonrpc": "2.0", "id": len(rpc) + 1, "method": "tools/call",
                   "params": {"name": "models_adopt", "arguments": {"path": str(folder)}}}
        response = call("/mcp", request)
        rpc.append(response)
        assert not response["result"].get("isError"), response
        details = response["result"]["structuredContent"]
        assert not details["refused"], details["refused"]
        return details

    def trainer(call):
        return next(item for item in call("/v1/training")["pack"] if item["id"] == "music-train")

    try:
        with studio("first") as call:
            assert not trainer(call)["installed"] and old_instance.is_dir()
            first = adopt(call)
            assert "music-train" in first["adopted"], first
            assert trainer(call)["installed"]
            updated = data / "hotstep" / new_tag / "music-train.exe"
            assert updated.is_file()
            chosen = sha(updated)
        with studio("restart") as call:
            assert trainer(call)["installed"] and not old_instance.exists()
            second = adopt(call)
            assert second["adopted"] == [] and second["copied"] == [], second
            assert sha(updated) == chosen and trainer(call)["installed"]
        after = {name: sha(models / name) for name in WEIGHTS}
        assert before == after and sha(checkpoint) == checkpoint_hash and sha(old_source / "music-train.exe") == old_exe_hash
        (work / "result.json").write_text(json.dumps({"trainer_sha256": chosen, "weights": after,
            "checkpoint_sha256": checkpoint_hash, "old_source_exe_sha256": old_exe_hash,
            "first_adopted": first["adopted"], "repeat_adopted": second["adopted"]}, indent=2))
    finally:
        (work / "mcp.json").write_text(json.dumps(rpc, indent=2))


if __name__ == "__main__":
    run(*(Path(value).resolve() for value in sys.argv[1:5]), sys.argv[5])
