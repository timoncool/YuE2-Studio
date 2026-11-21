"""Verify the collected Apple Silicon package without opening the UI or loading models."""

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


def checked_output(*command):
    return subprocess.check_output(command, text=True, timeout=30).strip()


def check_macho(binary, app, executable_dir):
    if not binary.resolve(strict=True).is_relative_to(app):
        raise ValueError(f"Binary resolves outside the app: {binary}")
    if checked_output("lipo", "-archs", binary) != "arm64":
        raise ValueError(f"Expected an arm64 Mach-O: {binary}")

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
    for line in checked_output("otool", "-l", binary).splitlines():
        if match := re.fullmatch(r"\s*cmd (LC_\w+)", line):
            command = match[1]
        elif match := re.fullmatch(r"\s*(?:path|name) (.+) \(offset \d+\)", line):
            if command == "LC_RPATH":
                rpaths.append(expand(match[1]))
            elif command in DYLIB_LOAD_COMMANDS:
                libraries.append(match[1])

    for dependency in libraries:
        if dependency.startswith(SYSTEM_PATHS):
            continue  # macOS may supply these through the dyld shared cache.
        if dependency.startswith("@rpath/"):
            candidates = [path / dependency.removeprefix("@rpath/") for path in rpaths]
        else:
            candidates = [expand(dependency)]
        if not any(path.is_file() and path.resolve().is_relative_to(app) for path in candidates):
            raise ValueError(f"{binary} has an unresolved packaged dependency: {dependency}")
    subprocess.run(["codesign", "--verify", "--strict", binary], check=True, timeout=30)
    print(f"Verified arm64, signature and {len(libraries)} imports: {binary.relative_to(app)}", flush=True)


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
    with (contents / "Info.plist").open("rb") as metadata:
        main = contents / "MacOS" / plistlib.load(metadata)["CFBundleExecutable"]
    subprocess.run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", app], check=True, timeout=30)
    check_macho(main, app, main.parent)

    resources = contents / "Resources" / "resources"
    tools = (
        (resources / "yue2-cpp" / "yue-server", 1, r"^Usage: .+ --model <gguf> --vae <gguf>"),
        (resources / "music-midi" / "music-midi", 2, r"^usage: ace-midi --model <dir>"),
        (resources / "music-midi" / "music-caption", 2, r"^usage: ace-caption --models <dir>"),
    )
    for directory in sorted({tool.parent for tool, _, _ in tools}):
        libraries = sorted(directory.glob("*.dylib"))
        if not libraries:
            raise ValueError(f"No packaged dylibs: {directory}")
        for library in sorted({path.resolve(strict=True) for path in libraries}):
            check_macho(library, app, directory)
    for tool, status, usage in tools:
        check_macho(tool, app, tool.parent)
        check_help(tool, status, usage)


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
