"""Verify the collected Apple Silicon package without opening the UI or loading models."""

import json
import os
from pathlib import Path
import plistlib
import re
import signal
import subprocess
import sys
import tempfile


DYLIB_LOAD_COMMANDS = {
    "LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB",
    "LC_LOAD_UPWARD_DYLIB", "LC_LAZY_LOAD_DYLIB",
}
SYSTEM_PATHS = ("/usr/lib/", "/System/Library/")
REPO = Path(__file__).resolve().parents[1]


def checked_output(*command):
    return subprocess.check_output(command, text=True, timeout=30).strip()


def macos_version(value):
    if not re.fullmatch(r"\d+(?:\.\d+){1,2}", value):
        raise ValueError(f"Invalid macOS version: {value}")
    parts = tuple(map(int, value.split(".")))
    return parts + (0,) * (3 - len(parts))


def check_macho(binary, app, executable_dir, minimum, checked, inherited_rpaths=(), *, exact_arm64=False):
    resolved = binary.resolve(strict=True)
    if not resolved.is_relative_to(app):
        raise ValueError(f"Binary resolves outside the app: {binary}")

    def expand(path):
        for token, directory in (("@loader_path", binary.parent),
                                 ("@executable_path", executable_dir)):
            if path == token or path.startswith(token + "/"):
                result = directory / path[len(token):].lstrip("/")
                if result.resolve().is_relative_to(app):
                    return result
        raise ValueError(f"{binary} has a path outside the app: {path}")

    rpaths, libraries = [], []
    command = None
    minos = None
    for line in checked_output("otool", "-arch", "arm64", "-l", binary).splitlines():
        if match := re.fullmatch(r"\s*cmd (LC_\w+)", line):
            command = match[1]
        elif match := re.fullmatch(r"\s*(?:path|name) (.+) \(offset \d+\)", line):
            if command == "LC_RPATH":
                rpaths.append(expand(match[1]))
            elif command in DYLIB_LOAD_COMMANDS:
                libraries.append(match[1])
        elif match := re.fullmatch(r"\s*(minos|version) (\d+(?:\.\d+){1,2})", line):
            if (command, match[1]) in {("LC_BUILD_VERSION", "minos"), ("LC_VERSION_MIN_MACOSX", "version")}:
                minos = match[2]

    rpaths = tuple(dict.fromkeys((*rpaths, *inherited_rpaths)))
    key = (resolved, binary.parent, executable_dir, rpaths)
    if key in checked:
        return rpaths
    checked.add(key)

    architectures = checked_output("lipo", "-archs", binary).split()
    if "arm64" not in architectures or (exact_arm64 and architectures != ["arm64"]):
        raise ValueError(f"Expected arm64 in {binary}, found: {architectures}")
    if minos is None or macos_version(minos) > macos_version(minimum):
        raise ValueError(f"{binary} has minOS {minos}; app declares macOS {minimum}")
    if binary.name.startswith(("libggml-blas", "libggml-cpu")) and macos_version(minimum) < (13, 3, 0):
        imports = checked_output("nm", "-arch", "arm64", "-u", binary)
        if "$NEWLAPACK" in imports or "$ILP64" in imports:
            raise ValueError(f"{binary} imports Accelerate symbols that require macOS 13.3")
        if binary.name.startswith("libggml-blas") and "_cblas_sgemm" not in imports:
            raise ValueError(f"{binary} is missing the legacy Accelerate cblas_sgemm import")
    subprocess.run(["codesign", "--verify", "--strict", binary], check=True, timeout=30)
    for dependency in libraries:
        if dependency.startswith(SYSTEM_PATHS):
            continue  # macOS may supply these through the dyld shared cache.
        if dependency.startswith("@rpath/"):
            candidates = [path / dependency.removeprefix("@rpath/") for path in rpaths]
        else:
            candidates = [expand(dependency)]
        packaged = next((path for path in candidates if path.is_file() and path.resolve().is_relative_to(app)), None)
        if packaged is None:
            raise ValueError(f"{binary} has an unresolved packaged dependency: {dependency}")
        check_macho(packaged, app, executable_dir, minimum, checked, rpaths)
    print(f"Verified arm64, minOS {minos}, signature and {len(libraries)} imports: {binary.relative_to(app)}", flush=True)
    return rpaths


def check_help(executable, expected_exit, usage):
    # No search-path injection from the build environment. The DMG must run on its own.
    env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
    result = subprocess.run([executable, "--help"], cwd=executable.parent, env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
    print(result.stdout, end="", flush=True)
    if result.returncode != expected_exit or not re.search(usage, result.stdout, re.MULTILINE):
        raise ValueError(f"{executable.name} --help returned {result.returncode}; expected usage and {expected_exit}")
    print(f"Verified {executable.name} --help (exit {expected_exit})", flush=True)


def check_app(mount):
    apps = list(mount.glob("*.app"))
    if len(apps) != 1 or apps[0].is_symlink():
        raise ValueError(f"Expected one app in the mounted DMG, found: {apps}")
    app = apps[0].resolve()
    contents = app / "Contents"
    checkout_bootstrap = REPO / "distribution/channels/stable/bootstrap"
    packaged_bootstrap = contents / "Resources" / "component-channel"
    for name in ("root.json", "url.txt"):
        if (packaged_bootstrap / name).read_bytes() != (checkout_bootstrap / name).read_bytes():
            raise ValueError(f"Packaged component-channel/{name} differs from the checkout")
        print(f"Verified component-channel/{name} byte for byte", flush=True)

    with (contents / "Info.plist").open("rb") as metadata:
        info = plistlib.load(metadata)
    main = contents / "MacOS" / info["CFBundleExecutable"]
    config = REPO / "desktop/src-tauri/tauri.macos-release.conf.json"
    minimum = json.loads(config.read_text(encoding="utf-8"))["bundle"]["macOS"]["minimumSystemVersion"]
    if macos_version(info["LSMinimumSystemVersion"]) != macos_version(minimum):
        raise ValueError(f"Info.plist minimum macOS version differs from {minimum}")
    subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", app], check=True, timeout=30)
    checked = set()
    check_macho(main, app, main.parent, minimum, checked, exact_arm64=True)

    resources = contents / "Resources" / "resources"
    tools = (
        (resources / "yue2-cpp" / "yue-server", 1, r"^Usage: .+ --model <gguf> --vae <gguf>"),
        (resources / "music-midi" / "music-midi", 2, r"^usage: ace-midi --model <dir>"),
        (resources / "music-midi" / "music-caption", 2, r"^usage: ace-caption --models <dir>"),
    )
    runtime_rpaths = {}
    for tool, status, usage in tools:
        rpaths = check_macho(tool, app, tool.parent, minimum, checked, exact_arm64=True)
        runtime_rpaths.setdefault(tool.parent, rpaths)
        check_help(tool, status, usage)
    for directory, rpaths in runtime_rpaths.items():
        libraries = sorted(directory.glob("*.dylib"))
        if not libraries:
            raise ValueError(f"No packaged dylibs: {directory}")
        for library in sorted({path.resolve(strict=True) for path in libraries}):
            check_macho(library, app, directory, minimum, checked, rpaths)


def interrupt(signum, _frame):
    raise SystemExit(128 + signum)


def main(dmg):
    signal.signal(signal.SIGTERM, interrupt)
    mount = Path(tempfile.mkdtemp(prefix="yue2-dmg-"))
    mounted = False
    try:
        subprocess.run(["hdiutil", "attach", "-readonly", "-nobrowse", "-noautoopen",
                        "-mountpoint", mount, dmg], check=True, timeout=120)
        mounted = True
        check_app(mount)
    finally:
        if mounted or mount.is_mount():
            print(f"Detaching {mount}", flush=True)
            try:
                subprocess.run(["hdiutil", "detach", mount], check=True, timeout=30)
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                subprocess.run(["hdiutil", "detach", "-force", mount], check=True, timeout=30)
        mount.rmdir()


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: check-macos-dmg.py PACKAGE.dmg")
    main(Path(sys.argv[1]).resolve(strict=True))
