"""Decide which CI jobs a change needs: a website-only edit skips the app builds after the pages are rebuilt and
checked; macOS RVC runs only when its inputs change."""

import os
from pathlib import Path
import subprocess
import sys


def git(*args):
    return subprocess.check_output(["git", *args])


def base_commit():
    event = os.environ["GITHUB_EVENT_NAME"]
    if event == "pull_request":
        # The checkout is GitHub's test merge. Compare it against the exact base.
        return os.environ["PR_BASE"]
    if event == "push":
        before = os.environ["PUSH_BASE"]
        if before and before.strip("0"):
            return before
        return git("merge-base", "HEAD", "origin/" + os.environ["DEFAULT_BRANCH"]).decode().strip()
    return None


if __name__ == "__main__":
    base = base_commit()
    if base and subprocess.run(["git", "cat-file", "-e", base + "^{commit}"], capture_output=True).returncode:
        base = None
    paths = set(git("diff", "--no-renames", "--name-only", "-z", base, "HEAD").split(b"\0")[:-1]) if base else set()
    matrix_file = b"docs/music-gpu-benchmarks.json"
    matrix_check = Path("scripts/check-music-gpu-benchmarks.py")
    site_pages = {f"docs/{lang}.html".encode() for lang in ("index", "ru", "ja", "ko", "pt", "zh")}
    generated_pages = site_pages | {b"docs/donate.html", b"docs/sitemap.xml"}
    matrix_files = generated_pages | {
        matrix_file, b"README.md", b"scripts/build-pages.mjs", b"docs/site/music-benchmarks.mjs",
        b"docs/site/strings.mjs", matrix_check.as_posix().encode()
    }
    matrix_only = False
    if paths & matrix_files and Path(os.fsdecode(matrix_file)).is_file() and matrix_check.is_file():
        subprocess.run([sys.executable, str(matrix_check)], check=True)
        # Only standalone website/documentation edits can dispense with compiling the app.
        matrix_only = paths <= matrix_files
        if matrix_only:
            subprocess.run(["node", "scripts/build-pages.mjs"], check=True)
            subprocess.run([sys.executable, str(matrix_check)], check=True)
            subprocess.run(["git", "diff", "--exit-code", "--", *sorted(map(os.fsdecode, generated_pages))], check=True)
    macos_rvc_inputs = [
        "crates/music-server/src/rvc_fast*",
        "crates/music-server/src/rvc_apply.rs",
        "crates/music-server/src/downloads.rs",
        "crates/music-server/src/install_record.rs",
        ".github/workflows/ci.yml",
    ]
    macos_rvc_changed = not base or subprocess.run(
        ["git", "diff", "--quiet", base, "HEAD", "--", *macos_rvc_inputs]
    ).returncode != 0
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        print(f"skip_build={str(matrix_only).lower()}", file=output)
        print(f"macos_rvc_changed={str(macos_rvc_changed).lower()}", file=output)
