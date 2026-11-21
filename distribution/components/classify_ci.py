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


def check_change(base, paths):
    if CATALOGUE.as_posix().encode() not in paths:
        return False
    after = read_catalogue(CATALOGUE)
    command = [sys.executable, str(VERIFIER), str(CATALOGUE), "--fetch"]
    with tempfile.TemporaryDirectory() as tmp:
        previous = Path(tmp) / "previous.json"
        old = subprocess.run(["git", "show", f"{base}:{CATALOGUE.as_posix()}"], capture_output=True)
        same_archives = False
        if old.returncode == 0:
            previous.write_bytes(old.stdout)
            same_archives = read_catalogue(previous)["archives"].keys() == after["archives"].keys()
            command.extend(["--previous", str(previous)])
        # Mixed code and catalogue changes must pass both the network check and the builds.
        subprocess.run(command, check=True)
    return len(paths) == 1 and same_archives


if __name__ == "__main__":
    base = base_commit()
    if base and subprocess.run(["git", "cat-file", "-e", base + "^{commit}"], capture_output=True).returncode:
        base = None
    paths = set(git("diff", "--no-renames", "--name-only", "-z", base, "HEAD").split(b"\0")[:-1]) if base else set()
    mirrors_only = check_change(base, paths)
    publisher_files = {b"distribution/components/" + name for name in
                       [b"stage_channel.py", b"test_stage_channel.py", b"PUBLISH.md"]}
    publisher_only = bool(paths) and paths <= publisher_files
    publisher_test = not base or bool(paths & (publisher_files | {
        b"distribution/components/verify.py", b"distribution/components/test_installed_channel.py",
        b"distribution/channels/stable/mirrors.json", b".github/workflows/ci.yml"
    }))
    channel_inputs = ["crates/music-server/src/component_catalogue.rs", "crates/music-server/src/downloads.rs",
                      "distribution/components/test_installed_channel.py", "distribution/components/check_installed_channel.sh",
                      "distribution/components/check_offline_onnx.sh"]
    channel_changed = not base or subprocess.run(
        ["git", "diff", "--quiet", base, "HEAD", "--", *channel_inputs]
    ).returncode != 0
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
        print(f"mirrors_only={str(mirrors_only).lower()}", file=output)
        print(f"skip_build={str(mirrors_only or publisher_only).lower()}", file=output)
        print(f"publisher_test={str(publisher_test).lower()}", file=output)
        print(f"channel_changed={str(channel_changed).lower()}", file=output)
        print(f"macos_rvc_changed={str(macos_rvc_changed).lower()}", file=output)
