"""Renew the published stable catalogue from the Windows release account."""

import argparse
from contextlib import nullcontext, redirect_stdout
import hashlib
import os
from datetime import datetime, timedelta, timezone
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
from urllib.request import Request, urlopen

from tuf.api.exceptions import RepositoryError
from tuf.api.metadata import Metadata

from sign_channel import CATALOGUES, read_previous, sign
from stage_channel import stage_channel


REPOSITORY = "https://github.com/timoncool/YuE2-Studio.git"
BRANCH = "refs/heads/main"
SITE = "docs/component-channel"
PUBLIC_TIMESTAMP = "https://timoncool.github.io/YuE2-Studio/component-channel/timestamp.json"
RENEW_BEFORE = timedelta(days=5)


def git(repo, *args):
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0", GCM_INTERACTIVE="Never", GH_PROMPT_DISABLED="1")
    result = subprocess.run(
        ["git", "-c", "credential.interactive=never", "-C", str(repo), *args],
        check=True, capture_output=True, text=True, env=env,
    )
    return result.stdout.strip()


def publish(repo, base):
    if git(repo, "rev-parse", "HEAD^") != base:
        raise ValueError("renewal commit must be a direct child of the inspected main")
    if git(repo, "ls-remote", "origin", BRANCH).split()[:1] != [base]:
        raise ValueError("main advanced during renewal; retry from the latest publication")
    # The explicit expected ref makes the server enforce the same check atomically.
    git(repo, "push", f"--force-with-lease={BRANCH}:{base}", "origin", f"HEAD:{BRANCH}")


def renew(repository, store, work_dir, *, apply, root_sha256=None, published_timestamp=None):
    with TemporaryDirectory(prefix="tuf-renew-", dir=work_dir) as temporary:
        work = Path(temporary)
        repo = work / "checkout"
        git(work, "clone", "--quiet", "--depth=1", "--filter=blob:none", "--sparse",
            "--config", "core.autocrlf=false", "--config", "core.eol=lf",
            "--branch", "main", "--single-branch", repository, str(repo))
        git(repo, "sparse-checkout", "set", SITE)
        base = git(repo, "rev-parse", "HEAD")
        site = repo / SITE
        if not (site / "timestamp.json").is_file():
            raise ValueError("initial stable channel has not been published")
        root_path = store / "root.json"
        root_bytes = root_path.read_bytes()
        if root_sha256 and hashlib.sha256(root_bytes).hexdigest() != root_sha256:
            raise ValueError("unexpected public root fingerprint")
        root = Metadata.from_bytes(root_bytes)
        versions, names = read_previous(site, root, root_bytes)
        timestamp_bytes = (site / "timestamp.json").read_bytes()
        if published_timestamp is not None:
            request = Request(published_timestamp, headers={"Cache-Control": "no-cache"})
            with urlopen(request, timeout=20) as response:
                if response.read(len(timestamp_bytes) + 1) != timestamp_bytes:
                    raise ValueError("GitHub Pages timestamp differs from main; investigate deployment")
        timestamp = Metadata.from_bytes(timestamp_bytes).signed
        now = datetime.now(timezone.utc)
        if timestamp.expires - now > RENEW_BEFORE:
            print(f"Skip: timestamp v{timestamp.version} is valid until {timestamp.expires.isoformat()}")
            return "fresh"
        if timestamp.expires <= now:
            print(f"Recovering expired timestamp v{timestamp.version} from the verified main history")
        if "mirrors.json" not in names or not names <= set(CATALOGUES.values()):
            raise ValueError("published target set is unsupported by the local signer")
        previous_targets = Metadata.from_bytes(
            (site / f"{versions['targets']}.targets.json").read_bytes()
        ).signed
        inputs = {}
        for name in sorted(names):
            target = previous_targets.targets[name]
            path = site / "targets" / f"{target.hashes['sha256']}.{name}"
            target.verify_length_and_hashes(path.read_bytes())
            inputs[name] = path
        signed, staged = work / "signed", work / "staged"
        sign(store, signed, inputs, site)
        stage_channel(root_path, signed / "metadata", signed / "targets", inputs["mirrors.json"],
                      inputs.get("rvc-voice-catalog.json"), staged, site,
                      inputs.get("music-engine-catalog.json"))
        next_timestamp = Metadata.from_bytes((staged / "timestamp.json").read_bytes()).signed
        description = f"timestamp v{next_timestamp.version}, expires {next_timestamp.expires.isoformat()}"
        if not apply:
            print(f"Dry run: verified renewal to {description}")
            return "staged"
        shutil.copytree(staged, site, dirs_exist_ok=True)
        git(repo, "add", "--", SITE)
        changed = git(repo, "diff", "--cached", "--name-only").splitlines()
        expected = {
            f"{SITE}/timestamp.json",
            f"{SITE}/{versions['snapshot'] + 1}.snapshot.json",
            f"{SITE}/{versions['targets'] + 1}.targets.json",
        }
        if set(changed) != expected:
            raise ValueError("unexpected renewal commit contents")
        for name in changed:
            if git(repo, "rev-parse", f":{name}") != git(repo, "hash-object", "--no-filters", name):
                raise ValueError(f"Git filters changed signed bytes in {name}")
        git(repo, "-c", "user.name=Studio component publisher",
            "-c", "user.email=components@users.noreply.github.com",
            "commit", "-m", f"Renew Studio component channel timestamp v{next_timestamp.version}")
        publish(repo, base)
        print(f"Submitted {description} to main for GitHub Pages")
        return "submitted"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--store", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True, help="parent for temporary public staging")
    parser.add_argument("--root-sha256", required=True, help="reviewed public root fingerprint")
    parser.add_argument("--publish", action="store_true", help="push the renewal to main after staging")
    parser.add_argument("--log-file", type=Path, help="append outcome and expiry for scheduled runs")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("requires the Windows release account")
    log = args.log_file.open("a", encoding="utf-8") if args.log_file else nullcontext()
    with log as output, redirect_stdout(output) if output else nullcontext():
        print(f"Renewal check {datetime.now(timezone.utc).isoformat()}")
        try:
            renew(REPOSITORY, args.store, args.work_dir, apply=args.publish,
                  root_sha256=args.root_sha256.lower(), published_timestamp=PUBLIC_TIMESTAMP)
        except (OSError, ValueError, RepositoryError, subprocess.CalledProcessError) as error:
            print(f"Renewal failed: {error}")
            if isinstance(error, subprocess.CalledProcessError) and error.stderr:
                print(error.stderr.strip())
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
