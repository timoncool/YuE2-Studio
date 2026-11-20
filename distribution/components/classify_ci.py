"""Allow a mirror-only edit to pass CI after checking the new mirrors in full."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile

from verify import read_catalogue


CATALOGUE = Path("distribution/channels/stable/mirrors.json")
VERIFIER = Path(__file__).with_name("verify.py")


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


def check_change(base):
    if not base:
        return False
    if subprocess.run(["git", "cat-file", "-e", base + "^{commit}"], capture_output=True).returncode:
        return False
    paths = git("diff", "--no-renames", "--name-only", "-z", base, "HEAD")
    if paths != CATALOGUE.as_posix().encode() + b"\0":
        return False
    # Only rerouting existing archives qualifies for the lightweight path.
    with tempfile.TemporaryDirectory() as tmp:
        previous = Path(tmp) / "previous.json"
        previous.write_bytes(git("show", f"{base}:{CATALOGUE.as_posix()}"))
        before, after = read_catalogue(previous), read_catalogue(CATALOGUE)
        if before["archives"].keys() != after["archives"].keys():
            return False
        subprocess.run(
            [sys.executable, str(VERIFIER), str(CATALOGUE),
             "--previous", str(previous), "--fetch"], check=True,
        )
    return True


if __name__ == "__main__":
    mirrors_only = check_change(base_commit())
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        print(f"mirrors_only={str(mirrors_only).lower()}", file=output)
