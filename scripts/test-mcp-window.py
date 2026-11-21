"""Exercise the built window's real MCP bridge in a temporary Chromium profile."""
import functools
import http.server
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import wave

from playwright.sync_api import sync_playwright


def audio(path, hz):
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as handle:
        handle.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
        handle.writeframes(b"".join(struct.pack("<h", int(6000 * math.sin(n * hz * math.tau / 8000)))
                                   for n in range(8000 * 8)))


def run(binary, port, data, frontend):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    base = f"http://127.0.0.1:{port}"

    def call(path, payload=None):
        request = urllib.request.Request(base + path, None if payload is None else json.dumps(payload).encode(),
            headers={"Content-Type": "application/json", "Accept": "application/json, text/event-stream"})
        with opener.open(request, timeout=22) as response:
            return json.load(response)

    next_id = 0

    def mcp(name, **args):
        nonlocal next_id
        next_id += 1
        answer = call("/mcp", {"jsonrpc": "2.0", "id": next_id, "method": "tools/call",
                              "params": {"name": name, "arguments": args}})
        assert "result" in answer and not answer["result"].get("isError"), (name, answer)
        result = answer["result"]
        return result.get("structuredContent") or " ".join(entry.get("text", "") for entry in result["content"])

    def title(expected):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            actual = mcp("ui_read_page")
            if isinstance(actual, dict):
                actual = actual["text"]
            if actual.splitlines()[0] == f"Page: {expected}":
                print(f"PASS Page: {expected}", flush=True)
                return
            time.sleep(.1)
        raise AssertionError((expected, actual[:240]))

    env = dict(os.environ, YUE_STUDIO_PORT=str(port), YUE_STUDIO_DATA_ROOT=str(data),
               DO_NOT_TRACK="1", STUDIO_HUB_URL="http://127.0.0.1:1", TIMONCOOL_SHARED_DIR=str(data / "shared"))
    server = subprocess.Popen([binary], env=env)
    try:
        for _ in range(160):
            try:
                call("/health")
                break
            except OSError:
                assert server.poll() is None
                time.sleep(.15)
        else:
            raise TimeoutError("Studio service did not start")
        sample = data / "Stem fixture.wav"
        audio(sample, 330)
        loaded = mcp("library_import_audio", path=str(sample), title="Stem fixture")
        song = loaded.get("song", loaded)
        song_id = song["id"]
        path = Path(song["audio_path"])
        assert path.is_file(), song
        assert path.name.startswith(song_id), path
        audio(path.parent / f"{song_id}-drums.wav", 440)
        audio(path.parent / f"{song_id}-bass.wav", 660)

        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(args=["--autoplay-policy=no-user-gesture-required", "--mute-audio"])
            page = browser.new_page(viewport={"width": 1400, "height": 900})
            page.on("pageerror", lambda error: print(f"Browser error: {error}", flush=True))
            page.add_init_script("""localStorage.setItem('language', 'en');
              localStorage.setItem('studio.musicEngine', 'minimaxmusic-cpp');
              localStorage.setItem('studio.legacyLibraryOfferSeen', '1');""")
            page.goto(f"http://127.0.0.1:{frontend}/", wait_until="domcontentloaded")
            page.get_by_role("heading", name="Create music").wait_for(timeout=20_000)
            title("Create music")
            mcp("ui_open_settings", section="engine")
            title("Local engine")
            mcp("ui_open_settings", section="cloud")
            title("Providers")
            mcp("ui_open_settings", section="privacy")
            title("Privacy")
            page.get_by_role("dialog").get_by_role("button", name="Done", exact=True).click()
            mcp("ui_navigate", view="library")
            title("Library")
            mcp("ui_navigate", view="search")
            title("Search")
            mcp("ui_navigate", view="tools")
            title("Work on a finished track")
            controls = page.get_by_test_id("stem-mixer")
            controls.get_by_role("button", name="Play the stems together", exact=True).wait_for()
            start = time.monotonic()
            reply = mcp("ui_click", text="Play the stems together")
            elapsed = time.monotonic() - start
            assert "Clicked Play the stems together." in json.dumps(reply) and elapsed < 5, (reply, elapsed)
            page.wait_for_function("""() => {
              const tracks = [...document.querySelectorAll('[data-testid=stem-mixer] audio')];
              return tracks.length === 2 && tracks.every(track => !track.paused && track.currentTime > 0.2)
              && Math.abs(tracks[0].currentTime - tracks[1].currentTime) < 0.15;
            }""")
            print(f"PASS MCP click acknowledged in {elapsed:.3f}s and both native Chromium tracks advanced", flush=True)
            browser.close()
    finally:
        server.terminate()
        server.wait(timeout=15)


if __name__ == "__main__":
    binary, bundle = [Path(arg).resolve() for arg in sys.argv[1:3]]
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=bundle)
    with tempfile.TemporaryDirectory(prefix="yue-window-") as temp, http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler) as frontend:
        threading.Thread(target=frontend.serve_forever, daemon=True).start()
        try:
            run(binary, int(sys.argv[3]), Path(temp), frontend.server_port)
        finally:
            frontend.shutdown()
