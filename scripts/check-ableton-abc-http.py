import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import wave
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("server", type=Path)
parser.add_argument("receipt", type=Path)
parser.add_argument("--expect-fix", action="store_true")
args = parser.parse_args()

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
url = f"http://127.0.0.1:{port}"
client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
session = None
seq = 0


def send(route, body=None, mcp=False):
    global session
    headers = {"Content-Type": "application/json"}
    if mcp:
        headers["Accept"] = "application/json, text/event-stream"
        if session:
            headers["Mcp-Session-Id"] = session
    request = urllib.request.Request(url + route, None if body is None else json.dumps(body).encode(), headers)
    try:
        with client.open(request, timeout=25) as result:
            session = result.headers.get("Mcp-Session-Id", session)
            return result.status, result.read()
    except urllib.error.HTTPError as result:
        return result.code, result.read()


def rpc(method, params, notify=False):
    global seq
    seq += 1
    body = dict(jsonrpc="2.0", method=method, params=params)
    if not notify:
        body["id"] = seq
    code, response = send("/mcp", body, mcp=True)
    assert code in (200, 202), (code, response)
    if notify:
        return None
    payload = next(line[6:] for line in response.splitlines() if line.startswith(b"data: ")) if response.startswith(b"event:") else response
    result = json.loads(payload)["result"]
    assert not result.get("isError"), result
    return result


def call(name, **options):
    result = rpc("tools/call", {"name": name, "arguments": options})
    return result.get("structuredContent") or json.loads(result["content"][0]["text"])


mono = "X:1\nT:Hosted score\nM:4/4\nL:1/4\nK:C\nC D E F|G A B c|"
native = (
    'X:1\nT:\nM:3/4\nL:1/4\nQ:1/4=92\n'
    'V: Vocal clef=treble name="Vocal Melody" snm="Vocal"\n'
    'V: Ins clef=treble name="Ins Melody" snm="Inst."\nK:C\n'
    'V: Vocal\nC D E|\nV: Ins\nF G A|'
)
cases = {
    "native": (native, 92, "3/4", 2, True, False),
    "mono_tempo": (mono.replace("M:4/4", "M:2/4").replace("K:C", "Q:1/4=104\nK:C").replace("C D E F|G A B c|", "C D|E F|"), 104, "2/4", 1, True, False),
    "mono_without_tempo": (mono, 120, "4/4", 1, False, False),
    "damaged": (mono.replace("C D E F|G A B c|", "???"), 120, "4/4", 0, False, True),
}

with tempfile.TemporaryDirectory(prefix="yue-ableton-http-", dir=args.receipt.parent) as tmp:
    root = Path(tmp)
    env = dict(os.environ, YUE_STUDIO_PORT=str(port), YUE_STUDIO_DATA_ROOT=str(root),
               TIMONCOOL_SHARED_DIR=str(root / "shared"), DO_NOT_TRACK="1", CUDA_VISIBLE_DEVICES="")
    with (args.receipt.parent / ("ableton-service-" + args.receipt.stem + ".log")).open("wb") as log:
        server_args = [str(args.server)]
        if args.server.name.casefold() == "yue2-studio.exe":
            server_args.append("--headless")
        process = subprocess.Popen(server_args, cwd=args.server.parent, env=env, stdout=log, stderr=log)
        try:
            for _ in range(80):
                assert process.poll() is None, process.returncode
                try:
                    if send("/health")[0] == 200:
                        break
                except OSError:
                    time.sleep(.25)
            else:
                raise RuntimeError("Isolated service did not answer /health")
            rpc("initialize", dict(protocolVersion="2025-03-26", capabilities={}, clientInfo=dict(name="ableton-abc-qa", version="1")))
            rpc("notifications/initialized", {}, notify=True)
            source = root / "fixture.wav"
            with wave.open(str(source), "wb") as audio:
                audio.setparams((1, 2, 24000, 0, "NONE", "not compressed"))
                audio.writeframes(bytes(24000 * 8))
            results = {}
            for label, (score, bpm, meter, midi_count, explicit, omitted) in cases.items():
                song = call("library_import_audio", path=str(source), title="ABC fixture " + label)
                current = call("library_song_get", song_id=song["id"], response_format="detailed")
                current["generation_settings"] = {**(current.get("generation_settings") or {}), "abc": score}
                call("library_song_update", song_id=song["id"], song=current)
                library_wav = Path(call("library_song_files", song_id=song["id"])["audio"])
                original = library_wav.read_bytes()
                with wave.open(io.BytesIO(original), "rb") as stored, wave.open(str(source), "rb") as input_wav:
                    assert stored.getparams() == input_wav.getparams()
                    assert stored.readframes(stored.getnframes()) == input_wav.readframes(input_wav.getnframes())
                code, raw = send("/v1/exports/live", {"song_id": song["id"]})
                got = json.loads(raw)
                results[label] = dict(http=code, response=got)
                print(label, code, got.get("bpm"), got.get("meter"), got.get("midi_parts"), flush=True)
                if args.expect_fix:
                    assert code == 200 and got["bpm"] == bpm and got["meter"] == meter, (label, code, got)
                    assert got["midi_parts"] == midi_count and got["tempo_from_score"] == explicit, (label, got)
                    assert got["score_midi_omitted"] == omitted, (label, got)
                    download_code, payload = send(f"/v1/exports/live/{got['id']}/file")
                    assert download_code == 200
                    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
                        assert archive.testzip() is None
                        names = archive.namelist()
                        master, = [name for name in names if name.endswith(" - Master.wav")]
                        assert archive.read(master) == original == library_wav.read_bytes()
                        assert sum(name.endswith(".mid") for name in names) == midi_count
                    results[label].update(archive_members=names, archive_sha256=hashlib.sha256(payload).hexdigest())
                    results[label]["master_sha256"] = hashlib.sha256(original).hexdigest()
            args.receipt.write_text(json.dumps(results, indent=2) + "\n", encoding="utf8")
            if not args.expect_fix:
                assert results["native"]["http"] == 200 and results["mono_without_tempo"]["http"] == 422, results
        finally:
            process.terminate()
            process.wait(timeout=20)
