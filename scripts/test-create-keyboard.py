"""Headless keyboard acceptance against the release UI bundle and a real Studio service."""
import base64
import contextlib
from functools import partial
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import wave

from playwright.sync_api import expect, sync_playwright

MODEL = "fixture/keyboard-music"
STYLE = "Gentle fingerpicked acoustic folk."
LYRICS = "[Verse]\nThe morning light is on the hill."
loopback = urllib.request.build_opener(urllib.request.ProxyHandler({}))
calls = []
pcm = struct.pack("<h", 1200) * 8000
buffer = io.BytesIO()
with wave.open(buffer, "wb") as output:
    output.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
    output.writeframes(pcm)
audio = base64.b64encode(buffer.getvalue()).decode()


class Provider(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        assert self.path.endswith("/chat/completions"), self.path
        assert self.headers["Authorization"] == "Bearer fixture-only"
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        calls.append(body)
        assert body["model"] == MODEL, body
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        data = {"choices": [{"delta": {"audio": {"data": audio}}}]}
        self.wfile.write(f"data: {json.dumps(data)}\n\ndata: [DONE]\n\n".encode())


@contextlib.contextmanager
def local_http(handler):
    with http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server.server_port
        finally:
            server.shutdown()
            thread.join()


def ephemeral_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def read(url):
    with loopback.open(url, timeout=5) as response:
        return response.read()


def wait_until(check):
    for _ in range(150):
        try:
            if answer := check():
                return answer
        except OSError:
            pass
        time.sleep(.2)
    raise TimeoutError(check.__name__)


def run(binary, bundle, port):
    with tempfile.TemporaryDirectory(prefix="yue-create-keys-") as temp, local_http(Provider) as upstream, local_http(
        partial(http.server.SimpleHTTPRequestHandler, directory=bundle)
    ) as ui_port:
        profile = Path(temp)
        catalogue = {"models": [{
            "id": MODEL, "name": "Keyboard fixture audio", "input_modalities": ["text"],
            "output_modalities": ["audio"], "supported_parameters": [], "capabilities": ["music_generation"],
        }]}
        (profile / "openrouter-catalog-v2.json").write_text(json.dumps(catalogue), encoding="utf-8")
        env = dict(os.environ, YUE_STUDIO_DATA_ROOT=temp, YUE_STUDIO_PORT=str(port),
                   TIMONCOOL_SHARED_DIR=str(profile / "registry"), YUE_ENGINE_PORT=str(ephemeral_port()),
                   YUE2_QA_OPENROUTER_PORT=str(upstream), OPENROUTER_API_KEY="fixture-key-not-for-upstream")
        base = f"http://127.0.0.1:{port}"
        with (profile / "service.log").open("w+") as log:
            service = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
            try:
                wait_until(lambda: read(base + "/health"))
                with sync_playwright() as playwright:
                    browser = playwright.chromium.launch(headless=True)
                    page = browser.new_page(viewport={"width": 1440, "height": 1000})
                    inputs = {"studio.musicEngine": "minimaxmusic-cpp", "language": "en",
                              "studio.createDraft": json.dumps({"style": STYLE, "lyrics": LYRICS}),
                              "studio.legacyLibraryOfferSeen": "1"}
                    page.add_init_script("for (const [key,value] of Object.entries(" + json.dumps(inputs)
                                         + ")) localStorage.setItem(key, value)")
                    page.goto(f"http://127.0.0.1:{ui_port}/", wait_until="domcontentloaded")
                    model = page.get_by_role("combobox", name="Model", exact=True)
                    engine = page.get_by_role("combobox", name="Music engine", exact=True)
                    expect(model).to_have_value("minimaxmusic-cpp", timeout=15000)
                    expect(engine).to_have_value("minimaxmusic-cpp")
                    expect(model.locator(f"option[value='cloud:{MODEL}']")).to_have_count(1)
                    model.focus()
                    page.keyboard.press("Tab")
                    expect(engine).to_be_focused()
                    page.keyboard.press("ArrowDown")
                    expect(engine).to_have_value("audio.cpp")
                    expect(model).to_have_value("minimaxmusic-cpp")
                    page.keyboard.press("ArrowUp")
                    expect(engine).to_have_value("minimaxmusic-cpp")
                    page.keyboard.press("Shift+Tab")
                    expect(model).to_be_focused()
                    page.keyboard.press("End")
                    expect(model).to_have_value(f"cloud:{MODEL}")
                    expect(engine).to_have_value("openrouter")
                    assert STYLE in page.locator("textarea").evaluate_all("(items) => items.map(x => x.value)")
                    assert LYRICS in page.locator("textarea").evaluate_all("(items) => items.map(x => x.value)")
                    create = page.locator("footer").get_by_role("button", name="Create", exact=True)
                    expect(create).to_be_enabled()
                    for _ in range(70):
                        page.keyboard.press("Tab")
                        if create.evaluate("(button) => document.activeElement === button"):
                            break
                    else:
                        raise AssertionError("Create is absent from the keyboard focus order")
                    with page.expect_response(lambda result: result.url == base + "/v1/music/jobs"
                                              and result.request.method == "POST", timeout=12000) as activity:
                        page.keyboard.press("Enter")
                    response = activity.value
                    assert response.ok, response.text()
                    request = response.request.post_data_json
                    assert request["music_source"] == {"kind": "openrouter", "model": MODEL}, request
                    assert request["style"] == STYLE and request["lyrics"] == LYRICS, request
                    job_id = response.json()["id"]
                    browser.close()

                def completed():
                    record = json.loads(read(base + f"/v1/music/jobs/{job_id}"))
                    assert record["status"] not in ("failed", "cancelled"), record
                    return record if record["status"] == "completed" else None

                job = wait_until(completed)
                take = job.get("song") or job["songs"][0]
                with wave.open(io.BytesIO(read(base + take["audio_url"])), "rb") as result:
                    assert result.readframes(result.getnframes()) == pcm
                assert len(calls) == 1
                print(json.dumps({"result": "pass", "keys": ["Tab", "ArrowDown", "ArrowUp", "Shift+Tab", "End", "Enter"],
                                  "engine_switch": "minimaxmusic-cpp → audio.cpp → minimaxmusic-cpp",
                                  "model": MODEL, "job": job_id, "song": take["id"],
                                  "pcm_sha256": hashlib.sha256(pcm).hexdigest()}, ensure_ascii=False))
            except BaseException:
                log.flush()
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                service.terminate()
                service.wait(timeout=15)


if __name__ == "__main__":
    run(str(Path(sys.argv[1]).resolve(strict=True)), str(Path(sys.argv[2]).resolve(strict=True)), int(sys.argv[3]))
