import itertools
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
from urllib.request import Request, urlopen

root = Path(os.environ["RUNNER_TEMP"]) / "yue2-webview-qa"
root.mkdir()
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    port = listener.getsockname()[1]
base = f"http://127.0.0.1:{port}"
env = dict(os.environ, YUE_STUDIO_PORT=str(port), YUE_STUDIO_DATA_ROOT=str(root / "data"),
           YUE_ENGINE_BIN=str(root / "absent-engine.exe"), YUE_STUDIO_SETTINGS_PATH=str(root / "settings.json"),
           TIMONCOOL_SHARED_DIR=str(root / "shared"))
ids = itertools.count(1)
calls = []

def mcp(name, args=None):
    payload = {"jsonrpc": "2.0", "id": next(ids), "method": "tools/call",
               "params": {"name": name, "arguments": args or {}}}
    headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
    with urlopen(Request(base + "/mcp", json.dumps(payload).encode(), headers), timeout=22) as response:
        reply = json.load(response)
    calls.append({"call": payload, "reply": reply})
    result = reply["result"]
    assert not result.get("isError"), result
    if "structuredContent" in result:
        return result["structuredContent"]
    content = result["content"][0]["text"]
    try:
        return json.loads(content)
    except json.JSONDecodeError:
        return content

def wait(check, duration=45):
    deadline = time.monotonic() + duration
    last = None
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except OSError as error:
            last = error
        time.sleep(.5)
    raise TimeoutError((last, calls[-3:]))

with (root / "studio.log").open("wb") as log:
    studio = subprocess.Popen([sys.argv[1]], env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        wait(lambda: mcp("studio_status").get("window_open"), 90)
        mcp("ui_navigate", {"view": "create"})
        mcp("create_form_set", {"fields": {"style": "Windows WebView folk", "lyrics": "Keep these words"}})
        mcp("ui_select", {"text": "Model", "value": "minimaxmusic-cpp"})
        mcp("ui_select", {"text": "Local model", "value": "minimal"})
        pair = wait(lambda: (item if (item := mcp("music_pair_status"))["stage"] == "needs_install" else None))
        form = wait(lambda: (item if (item := mcp("create_form_get")).get("music_pair", {}).get("revision") == pair["revision"] else None))
        assert form["music_pair"]["stage"] == pair["stage"], (form, pair)
        assert pair["requested"] == {"kind": "native", "engine": "minimaxmusic-cpp", "model": "minimal"}, pair
        assert "Keep these words" in json.dumps(form) and "Windows WebView folk" in json.dumps(form), form
        page = mcp("ui_read_page")
        assert "Retry" in json.dumps(page), page
        print("PASS Windows WebView Create, live pair selection, missing weights feedback and retained draft")
    finally:
        (root / "transcript.json").write_text(json.dumps(calls, indent=2), encoding="utf-8")
        studio.terminate()
        studio.wait(timeout=20)
