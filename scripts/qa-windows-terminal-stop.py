import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import time
import urllib.request
import zipfile

parser = argparse.ArgumentParser()
source = parser.add_mutually_exclusive_group(required=True)
source.add_argument("--url")
source.add_argument("--archive", type=Path)
parser.add_argument("--sha256", required=True)
parser.add_argument("--bytes", type=int, required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--evidence", type=Path, required=True)
args = parser.parse_args()

if os.name != "nt":
    raise RuntimeError("This fixture needs the Windows desktop shell")

args.evidence.mkdir(parents=True)
archive = args.archive
if args.url:
    archive = args.evidence / args.url.rsplit("/", 1)[-1]
    with urllib.request.urlopen(args.url, timeout=180) as response, archive.open("wb") as file:
        shutil.copyfileobj(response, file)
with archive.open("rb") as file:
    checksum = hashlib.file_digest(file, "sha256").hexdigest()
if checksum != args.sha256.lower() or archive.stat().st_size != args.bytes:
    raise RuntimeError(f"Wrong portable archive: {archive}")

user32 = ctypes.WinDLL("user32", use_last_error=True)
callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

def visible_windows(pid):
    found = []

    @callback_type
    def collect(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            title = ctypes.create_unicode_buffer(512)
            user32.GetWindowTextW(hwnd, title, len(title))
            rect = wintypes.RECT()
            if user32.GetWindowRect(hwnd, ctypes.byref(rect)) and rect.right > rect.left and rect.bottom > rect.top:
                found.append({"handle": f"0x{hwnd:x}", "title": title.value,
                              "width": rect.right - rect.left, "height": rect.bottom - rect.top})
        return True

    user32.EnumWindows(collect, 0)
    return found

def health():
    try:
        with opener.open("http://127.0.0.1:8791/health", timeout=1.5) as response:
            return json.load(response)
    except OSError:
        return None

def require_ready(process, visible):
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Studio quit during startup with {process.returncode}")
        status = health()
        windows = visible_windows(process.pid)
        if status and status.get("status") == "ok" and (not visible or windows):
            return status, windows
        time.sleep(.25)
    raise TimeoutError(f"Studio PID {process.pid} did not become ready (visible={visible})")

def start(exe, flags, log):
    # CREATE_NO_WINDOW applies to console processes. GUI visibility is decided by Studio.
    return subprocess.Popen([str(exe), *flags], cwd=exe.parent, stdout=log,
                            stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW)

def wait_service_down():
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if health() is None:
            return
        time.sleep(.25)
    raise RuntimeError("Studio service still answers after the original process quit")

def check(mode, report):
    if health() is not None:
        raise RuntimeError("Expected isolated runner with no Studio service before startup")
    sandbox = args.evidence / mode
    sandbox.mkdir()
    with zipfile.ZipFile(archive) as package:
        package.extractall(sandbox)
    candidates = list(sandbox.rglob("YuE2-Studio.exe"))
    if len(candidates) != 1:
        raise RuntimeError(f"Expected exactly one portable executable: {candidates}")
    exe = candidates[0]
    state = {"executable": str(exe)}
    report["cases"][mode] = state
    app = None
    stop = None
    try:
        with (args.evidence / f"{mode}-studio.log").open("wb") as app_log:
            app = start(exe, ["--headless"] if mode == "headless" else [], app_log)
            state["pid"] = app.pid
            state["health_before"], state["visible_before"] = require_ready(app, mode == "visible")
            if mode == "headless":
                time.sleep(2)
                state["visible_before"] = visible_windows(app.pid)
                if state["visible_before"]:
                    raise RuntimeError("--headless displayed a window before --stop")
            with (args.evidence / f"{mode}-stop.log").open("wb") as stop_log:
                stop = start(exe, ["--stop"], stop_log)
                state["stop_pid"] = stop.pid
                state["stop_exit_code"] = stop.wait(timeout=30)
            if state["stop_exit_code"] != 0:
                raise RuntimeError(f"--stop returned {state['stop_exit_code']}")
            if mode == "headless":
                state["studio_exit_code"] = app.wait(timeout=25)
                if state["studio_exit_code"] != 0:
                    raise RuntimeError(f"Headless Studio quit with {state['studio_exit_code']}")
                wait_service_down()
                state["health_after"] = None
                state["visible_after"] = []
            else:
                deadline = time.monotonic() + 8
                before = {window["handle"] for window in state["visible_before"]}
                while time.monotonic() < deadline:
                    windows = visible_windows(app.pid)
                    if app.poll() is not None or not before.intersection(window["handle"] for window in windows):
                        raise RuntimeError("--stop closed the original visible Studio window")
                    time.sleep(.25)
                state["visible_after"] = windows
                state["health_after"] = health()
                state["studio_still_running"] = app.poll() is None
                if not state["health_after"] or not state["studio_still_running"]:
                    raise RuntimeError("Visible Studio lost its process or service after --stop")
    finally:
        for process in (stop, app):
            if process and process.poll() is None:
                process.terminate()
                process.wait(timeout=15)
        wait_service_down()

report = {"release": args.version, "archive": archive.name, "archive_sha256": checksum,
          "archive_bytes": args.bytes, "windows": platform.platform(), "cases": {}}
try:
    check("headless", report)
    check("visible", report)
    report["passed"] = True
finally:
    (args.evidence / "results.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report), flush=True)
