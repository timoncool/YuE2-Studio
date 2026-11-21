import array
import io
import math
import struct
import wave
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


def make_fixture(root):
    speech = root / "spoken-vocals.wav"
    subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", """
Add-Type -AssemblyName System.Speech
$s = [System.Speech.Synthesis.SpeechSynthesizer]::new()
$f = [System.Speech.AudioFormat.SpeechAudioFormatInfo]::new(44100, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$s.SetOutputToWaveFile($env:RVC_QA_SPEECH, $f)
$s.Speak("The morning wind is gentle. We can sing and hear the river flowing through the valley.")
$s.Dispose()
"""], env=dict(os.environ, RVC_QA_SPEECH=str(speech)), check=True)
    with wave.open(str(speech), "rb") as reader:
        assert reader.getnchannels() == 1 and reader.getsampwidth() == 2
        rate = reader.getframerate()
        voice = array.array("h", reader.readframes(reader.getnframes()))
    left = [int(1500 * math.sin(2 * math.pi * 330 * i / rate)) for i in range(len(voice))]
    right = [int(1700 * math.sin(2 * math.pi * 440 * i / rate)) for i in range(len(voice))]
    backing = array.array("h", (value for frame in zip(left, right) for value in frame))
    mixed = array.array("h", (max(-32768, min(32767, sample + tone))
                             for sample, l, r in zip(voice, left, right) for tone in (l, r)))
    def write(name, data):
        path = root / name
        with wave.open(str(path), "wb") as writer:
            writer.setparams((2, 2, rate, 0, "NONE", "not compressed"))
            writer.writeframes(data.tobytes())
        return path
    return speech, write("instrumental.wav", backing), write("original.wav", mixed), rate, len(voice)


def wave_shape(body):
    assert body[:4] == b"RIFF" and body[8:12] == b"WAVE"
    offset = 12
    while offset + 8 <= len(body):
        kind, length = struct.unpack_from("<4sI", body, offset)
        start = offset + 8
        if kind == b"fmt ":
            code, channels, rate, _, align, bits = struct.unpack_from("<HHIIHH", body, start)
        if kind == b"data":
            return code, channels, rate, bits, length // align
        offset = start + length + (length & 1)
    raise AssertionError("WAV has no audio")


def run():
    binary = Path("target/debug/music-server.exe").resolve()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    studio, engine_port = port(), port()
    base = f"http://127.0.0.1:{studio}"
    with tempfile.TemporaryDirectory(prefix="yue-rvc-published-") as folder:
        root = Path(folder)
        (root / "data").mkdir()
        runtime = root / "data/rvc/native/v1"
        runtime.mkdir(parents=True)
        for asset in ("hubert.vfrm", "rmvpe.vfrm"):
            (runtime / asset).write_bytes(b"test asset")
        worker = root / "worker.rs"
        worker.write_text(r"""
use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    writeln!(out, "{}", r#"{"protocol":1,"backend":"tensorrt-rtx","version":"0.1.7"}"#).unwrap();
    out.flush().unwrap();
    for request in stdin.lock().lines() {
        request.unwrap();
        writeln!(out, "{}", r#"{"ok":false,"error":"engine backend is unavailable: TensorRT-RTX API failed: createExecutionContext"}"#).unwrap();
        out.flush().unwrap();
    }
}
""", encoding="utf-8")
        subprocess.run(["rustc", str(worker), "-o", str(runtime / "vocal-fast.exe")], check=True)
        env = dict(os.environ, YUE_STUDIO_PORT=str(studio), YUE_ENGINE_PORT=str(engine_port),
                   YUE_STUDIO_DATA_ROOT=str(root / "data"), YUE_ENGINE_BIN=str(root / "absent-yue-server.exe"),
                   TIMONCOOL_SHARED_DIR=str(root / "registry"), DO_NOT_TRACK="1", STUDIO_HUB_URL=f"{base}/fixture-hub")
        requests = 0

        def mcp(name, arguments, timeout=30, expect_error=False):
            nonlocal requests
            requests += 1
            payload = {"jsonrpc": "2.0", "id": requests, "method": "tools/call",
                       "params": {"name": name, "arguments": arguments}}
            request = Request(f"{base}/mcp", json.dumps(payload).encode(),
                              {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"})
            with HTTP.open(request, timeout=timeout) as response:
                result = json.load(response)
            assert "error" not in result, result
            if expect_error:
                assert result["result"].get("isError"), result
                return result["result"]
            assert not result["result"].get("isError"), result
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

                vocal, instrumental, original, rate, frames = make_fixture(root)
                added = mcp("library_import_audio", {"path": str(original), "title": "RVC remix fixture"})
                song = added.get("song", added)
                song_id = song["id"]
                with HTTP.open(f"{base}/v1/library/media/{song_id}", timeout=15) as response:
                    source_before = response.read()
                media = root / "data/media"
                vocal_stem = media / f"{song_id}-vocals.wav"
                backing_stem = media / f"{song_id}-instrumental.wav"
                vocal_stem.write_bytes(vocal.read_bytes())
                backing_stem.write_bytes(instrumental.read_bytes())
                stems_before = {item.name: digest(item) for item in (vocal_stem, backing_stem)}

                setup = mcp("rvc_backend_status", {})
                if not setup["backends"]["audio.cpp"]["ready"]:
                    setup = mcp("rvc_apply_install", {}, timeout=900)
                assert setup["backends"]["audio.cpp"]["ready"], setup
                assert setup["effective_backend"] == "audio.cpp", setup
                voice = mcp("rvc_catalog_install", {"id": VOICE}, timeout=600)
                parameters = {"song_id": song_id, "voice_id": voice["id"], "backend": "auto", "retrieval_blend": 0.35}
                started = time.monotonic()
                take = mcp("rvc_apply", parameters, timeout=600)
                elapsed = time.monotonic() - started
                assert take["song_id"] != song_id, take
                assert take["requested_backend"] == "auto" and take["effective_backend"] == "audio.cpp", take
                assert "createExecutionContext" in take["fallback_reason"], take
                explicit_error = mcp("rvc_apply", {**parameters, "backend": "vocal-fast-rs"}, expect_error=True)
                assert "createExecutionContext" in json.dumps(explicit_error), explicit_error
                with HTTP.open(f"{base}/v1/library/songs", timeout=15) as response:
                    library_ids = [row["id"] for row in json.load(response)]
                assert len(library_ids) == 2 and set(library_ids) == {song_id, take["song_id"]}, library_ids
                with HTTP.open(base + take["audio_url"], timeout=15) as response:
                    rendered = response.read()
                assert wave_shape(rendered)[1:] == (2, rate, 32, frames), wave_shape(rendered)
                assert source_before != rendered
                with HTTP.open(f"{base}/v1/library/media/{song_id}", timeout=15) as response:
                    assert response.read() == source_before
                assert {item.name: digest(item) for item in (vocal_stem, backing_stem)} == stems_before
                saved = mcp("library_song_get", {"song_id": take["song_id"], "response_format": "detailed"})
                derived = saved["metadata"]["derived"]
                assert derived["from"] == song_id and derived["tool"] == "rvc", derived
                assert derived["settings"]["voice_id"] == voice["id"]
                assert derived["settings"]["retrieval_blend"] == 0.35
                assert derived["settings"]["effective_backend"] == "audio.cpp"
                assert derived["settings"]["fallback_reason"] == take["fallback_reason"]
                assert derived["settings"]["voice_sha256"].startswith("50ea3d733073550ef6f7")
                evidence = {"source_revision": revision, "server_sha256": digest(binary), "source_song": song_id,
                            "source_sha256": hashlib.sha256(source_before).hexdigest(), "source_preserved": True,
                            "stems_preserved": stems_before, "take": take, "derived": derived,
                            "wav_shape": wave_shape(rendered), "take_sha256": hashlib.sha256(rendered).hexdigest(),
                            "seconds": elapsed, "library_ids": library_ids, "explicit_native_error": explicit_error}
                output = json.dumps(evidence, ensure_ascii=True, indent=2)
                Path("rvc-fallback-single-take-mcp.json").write_text(output + "\n", encoding="utf-8")
                print(output)
            finally:
                subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True)
                process.wait(timeout=15)


if __name__ == "__main__":
    run()
