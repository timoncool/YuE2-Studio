import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import sqlite3
import struct
import subprocess
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener
import wave

work = Path(os.environ["RUNNER_TEMP"]) / "rvc-proof"
work.mkdir()
input_dir = work.parent / "rvc-input"
input_dir.mkdir()
profile = work.parent / "Профиль RVC"
profile.mkdir()
native = profile / "rvc/native"
(native / "v1").mkdir(parents=True)
app = work.parent / "rvc-install" / "YuE2 Studio.app"
app.parent.mkdir()
exe = app / "Contents/MacOS/YuE2-Studio"
repo = os.environ["GITHUB_REPOSITORY"]
base = "http://127.0.0.1:8791"
http = build_opener(ProxyHandler({}))
dmgs = [
    ("44885352475", "9e50a4b0dc1240bf573ad19009eba5e59aaf17ab", "8aeff76ffde1d7a24dab053f7b7bb61d1bf9a014873d8fde4bfa425d108ec419"),
    ("44894247344", "2a9bde22843bdf23a96632bf7b1f8e0d0b0b9f87", "1893260dd2f0b546d09098fa815fdf65178f593ead025be353c91fd93d91f94f"),
]
hf = "https://huggingface.co/licyk/rvc-model/resolve/ad931ef3dc18f2d63ef81ecafddd498a111c8ecd/voices"
assets = [
    (input_dir / "guanguanV1.pth", f"{hf}/guanguanV1/guanguanV1.pth", "5a4920ef7b8c28be41dbd3afe2bd8cc852bccc5da82ac0590afb2a160ca81bb2", 55024402),
    (input_dir / "guanguanV1.index", f"{hf}/guanguanV1/guanguanV1.index", "d04454f009988ad7e3d830d3d8870b3d107978738e2e3b10bd415038ed882cac", 33496795),
    (input_dir / "youzhanv2-xi.pth", f"{hf}/youzhanv2-xi/youzhanv2-xi.pth", "88ad512ce774a929972ed27650aa80687fbe220ecdc20809a164c65dca4a408c", 57551491),
]
for target, sha, size in [
    ("hubert.vfrm", "a84fed477f49a040d53b44b7c27184d711fb246d5f9119c7c046941aaee65c59", 378427648),
    ("rmvpe.vfrm", "462ee5494525ed1029abfad368abb56ca20285478e24aa846575ed400bff4734", 362064656),
]:
    model_url = "https://modelscope.cn/api/v1/models/MosRat/vfrm-models/repo?Revision=9527a1bcf861c70717d4aba619ba92f2b5f42396&FilePath=shared%2F"
    assets.append((native / "v1" / target, model_url + target, sha, size))
for target, ref, sha, size in [
    ("vocal-fast-macos-arm64-cpu.tar.gz", "component-03c3be316067", "409f24075b4a7b9dd86953ff9da0ea25de10c4f97f1727076b8681091738c873", 7753829),
    ("vocal-fast-macos-arm64-metal-preview.tar.gz", "component-300ad493ace0", "c31ff0237af06a964a8178fdd6c684e2ff4291a147c4d6237c609ecab552703b", 7756736),
]:
    assets.append((native / target, f"https://github.com/timoncool/vocal-fast-rs/releases/download/{ref}/{target}", sha, size))


def command(*argv):
    return subprocess.check_output(argv, text=True).strip()


def digest(path):
    with path.open("rb") as handle:
        sha = hashlib.sha256()
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            sha.update(chunk)
        return sha.hexdigest()


def install_dmg(index):
    run, revision, expected = dmgs[index]
    assert command("gh", "run", "view", run, "-R", repo, "--json", "headSha", "--jq", ".headSha") == revision
    destination = input_dir / str(index)
    subprocess.run(["gh", "run", "download", run, "-R", repo, "-n", "yue2-studio-macos-arm64", "-D", str(destination)], check=True)
    dmg = destination / "YuE2-Studio-5.0.0-macos-arm64.dmg"
    assert digest(dmg) == expected
    mount = destination / "mount"
    mount.mkdir()
    subprocess.run(["hdiutil", "attach", "-nobrowse", "-noautoopen", "-readonly", "-mountpoint", mount, dmg], check=True)
    try:
        if app.exists():
            app.rename(app.parent / "old-backup.app")
        subprocess.run(["ditto", mount / "YuE2 Studio.app", app], check=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", app], check=True)
    finally:
        subprocess.run(["hdiutil", "detach", mount], check=True)
    return {"run": run, "source": revision, "dmg": expected, "installed_macho": digest(exe)}


def get(path, timeout=10):
    with http.open(base + path, timeout=timeout) as response:
        return response.read()


def tool(name, args):
    body = json.dumps({"jsonrpc":"2.0", "id":1, "method":"tools/call",
                       "params":{"name":name, "arguments":args}}).encode()
    request = Request(base + "/mcp", data=body, headers={"Content-Type":"application/json","Accept":"application/json,text/event-stream"})
    with http.open(request, timeout=180) as response:
        text = response.read().decode()
    if text.startswith(("event:", "data:")):
        text = next(line[6:] for line in text.splitlines() if line.startswith("data: "))
    result = json.loads(text)["result"]
    assert not result.get("isError"), result
    return result.get("structuredContent") or json.loads(result["content"][0]["text"])


def pcm(data):
    assert data[:4] == b"RIFF" and data[8:12] == b"WAVE"
    pos = 12
    chunks = {}
    while pos + 8 <= len(data):
        tag, size = struct.unpack_from("<4sI", data, pos)
        pos += 8
        chunks[tag] = data[pos:pos+size]
        pos += size + (size & 1)
    return {"fmt": hashlib.sha256(chunks[b"fmt "]).hexdigest(),
            "data": hashlib.sha256(chunks[b"data"]).hexdigest(), "bytes": len(chunks[b"data"])}


def statuses(install):
    results = {}
    for metal in (False, True):
        args = {"metal": metal}
        status = tool("rvc_native_runtime_status", args)
        assert status["supported"], status
        if install and not status["installed"]:
            tool("rvc_native_runtime_install", args)
            for _ in range(180):
                status = tool("rvc_native_runtime_status", args)
                if status["installed"]:
                    break
                progress = status.get("download")
                assert not (progress and progress.get("error")), status
                time.sleep(.5)
        assert status["installed"] and status["missing_bytes"] == 0, status
        if not install:
            assert status["download"] is None, status
        results[str(metal)] = status
    return results


def verify_worker(folder):
    binary = native / "v1" / folder / "vocal-fast"
    assert command("lipo", "-archs", binary) == "arm64"
    command("codesign", "--verify", "--strict", binary)
    imports = command("otool", "-L", binary)
    for line in imports.splitlines()[1:]:
        path = line.strip().split(" (", 1)[0]
        assert path.startswith(("@", "/usr/lib/", "/System/Library/")), line
    help_result = subprocess.run([binary, "--help"], cwd=binary.parent, capture_output=True, text=True, timeout=20)
    assert help_result.returncode in (0, 1, 2) and "commands:" in (help_result.stdout + help_result.stderr).lower(), help_result
    return {"sha256": digest(binary), "imports": imports, "help_exit": help_result.returncode}


def snapshot(song, install=False):
    runtime = statuses(install)
    voices = json.loads(get("/v1/rvc/voices"))
    shapes = [(v["model_name"], v["index_name"]) for v in voices]
    assert shapes == [("guanguanV1.pth", "guanguanV1.index"), ("youzhanv2-xi.pth", None)], voices
    saved = tool("library_song_get", {"song_id": song, "response_format":"detailed"})
    assert song in json.dumps(saved) and "RVC upgrade fixture" in json.dumps(saved)
    wave_pcm = pcm(get(f"/v1/library/media/{song}"))
    assert wave_pcm["data"] == pcm(input_wav.read_bytes())["data"]
    paths = sorted(p for root in ("rvc/voices", "rvc/native") for p in (profile / root).rglob("*") if p.is_file())
    files = {str(path.relative_to(profile)): digest(path) for path in paths}
    with sqlite3.connect((profile / "library.sqlite").as_uri() + "?mode=ro", uri=True) as connection:
        assert connection.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
    return {"runtime":runtime, "voices":voices, "song_id":song, "pcm":wave_pcm, "files":files,
            "loader":{folder:verify_worker(folder) for folder in ("macos-cpu","macos-metal")}}


def start(env, log):
    with log.open("wb") as output:
        process = subprocess.Popen([exe, "--headless"], env=env, stdout=output, stderr=subprocess.STDOUT)
    try:
        for _ in range(100):
            assert process.poll() is None, log.read_text(errors="replace")
            try:
                health = json.loads(get("/health", 2))
                assert health["service_executable"] == str(exe), health
                return process
            except URLError:
                time.sleep(.3)
        raise TimeoutError(log.read_text(errors="replace"))
    except BaseException:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=15)
        raise


def stop(process, env):
    try:
        subprocess.run([exe, "--stop"], env=env, check=True, timeout=15)
        assert process.wait(timeout=15) == 0
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=15)


assert command("uname", "-m") == "arm64"
for target, url, sha, size in assets:
    subprocess.run(["curl", "-fLsS", "--retry", "2", "--max-time", "200", "-o", target, url], check=True)
    assert target.stat().st_size == size and digest(target) == sha, target
    print("verified", target.name, sha, flush=True)
input_wav = input_dir / "input.wav"
with wave.open(str(input_wav), "wb") as audio:
    audio.setparams((1, 2, 44100, 0, "NONE", "not compressed"))
    audio.writeframes(b"".join(struct.pack("<h", int(8500 * math.sin(i * 2 * math.pi * 261.63 / 44100))) for i in range(44100)))
env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(profile),
           YUE_STUDIO_SETTINGS_PATH=str(profile / "studio-settings.json"), DO_NOT_TRACK="1")
old = install_dmg(0)
process = start(env, work / "old.log")
try:
    indexed = tool("rvc_voice_import", {"model_path":str(input_dir / "guanguanV1.pth"),
                                        "index_path":str(input_dir / "guanguanV1.index")})
    unindexed = tool("rvc_voice_import", {"model_path":str(input_dir / "youzhanv2-xi.pth")})
    imported = tool("library_import_audio", {"path":str(input_wav), "title":"RVC upgrade fixture"})
    song = imported.get("id") or imported["song"]["id"]
    before = snapshot(song, install=True)
    (work / "before.json").write_text(json.dumps(before, ensure_ascii=False, indent=2))
finally:
    stop(process, env)
new = install_dmg(1)
assert old["installed_macho"] != new["installed_macho"]
process = start(env, work / "new.log")
try:
    after = snapshot(song)
finally:
    stop(process, env)
for key in ("voices", "song_id", "pcm", "files"):
    assert before[key] == after[key], key
proof = {"old_dmg":old, "new_dmg":new, "old":before, "updated":after,
         "fixture":{target.name: {"bytes":size, "sha256":sha, "url":url} for target, url, sha, size in assets},
         "arch":"arm64", "os":command("sw_vers", "-productVersion")}
(work / "upgrade.json").write_text(json.dumps(proof, ensure_ascii=False, indent=2))
print("PASS old DMG -> new DMG: PCM, two prior voices, old FAISS, CPU and Metal package files and loader", flush=True)
