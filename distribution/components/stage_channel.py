"""Stage signed TUF metadata and approved catalogues for an atomic site deployment.

Run on the publishing machine with python-tuf, using the same public root as
the installer. The signing repository can keep metadata and targets apart.
The output is a complete site with verified current metadata and immutable
files from the preceding publication.
"""
import argparse
from pathlib import Path
import re
import shutil
from tempfile import TemporaryDirectory
from urllib.parse import urlsplit

from tuf.api.exceptions import DownloadHTTPError
from tuf.api.metadata import Metadata, Root
from tuf.ngclient import Updater, UpdaterConfig
from tuf.ngclient.fetcher import FetcherInterface

from verify import read_catalogue


class StagingFetcher(FetcherInterface):
    def __init__(self, metadata, targets, stage):
        self.metadata = metadata
        self.targets = targets
        self.stage = stage

    def _fetch(self, url):
        # Only flat, hash-prefixed catalogue names and versioned role files
        # occur in the Studio channel. No arbitrary input directory is copied.
        parts = urlsplit(url).path.lstrip("/").split("/")
        if len(parts) == 1:
            source = self.metadata
        elif len(parts) == 2 and parts[0] == "targets":
            source = self.targets
        else:
            raise ValueError(f"unsupported repository path {parts}")
        name = parts[-1]
        if not name or not all(ch.isascii() and (ch.isalnum() or ch in "-_.") for ch in name) or name in (".", ".."):
            raise ValueError("invalid repository filename")
        path = source / name
        staged = self.stage.joinpath(*parts)
        if not path.is_file():
            if parts == ["timestamp.json"] or not staged.is_file():
                raise DownloadHTTPError("file not present in signing repository", 404)
            with staged.open("rb") as reader:
                while chunk := reader.read(65536):
                    yield chunk
            return
        if path.resolve().parent != source.resolve():
            raise ValueError(f"file escapes signing directory: {name}")
        with path.open("rb") as reader, staged.open("wb") as writer:
            while chunk := reader.read(65536):
                writer.write(chunk)
                yield chunk


ROLE_PATH = re.compile(r"[1-9][0-9]*\.(root|snapshot|targets)\.json")
TARGET_PATH = re.compile(r"(?:[0-9a-f]{64}|[0-9a-f]{128})\.(mirrors|rvc-voice-catalog|music-engine-catalog)\.json")


def retain_immutable_files(previous, candidate):
    """Copy only public channel artifacts and return the original paths for comparison."""
    paths = [p.relative_to(previous) for p in previous.iterdir() if ROLE_PATH.fullmatch(p.name)]
    target_dir = previous / "targets"
    if target_dir.is_symlink():
        raise ValueError("published targets directory must not be a link")
    if target_dir.is_dir():
        paths.extend(p.relative_to(previous) for p in target_dir.iterdir() if TARGET_PATH.fullmatch(p.name))
    for relative in paths:
        source = previous / relative
        if source.is_symlink() or not source.is_file():
            raise ValueError(f"published artifact is not a regular file: {relative}")
        shutil.copyfile(source, candidate / relative)
    return paths


def check_previous_timestamp(previous, candidate):
    old_timestamp = previous / "timestamp.json"
    new_timestamp = candidate / "timestamp.json"
    if old_timestamp.read_bytes() == new_timestamp.read_bytes():
        return
    old = Metadata.from_file(str(old_timestamp)).signed
    new = Metadata.from_file(str(new_timestamp)).signed
    if new.version <= old.version or new.snapshot_meta.version < old.snapshot_meta.version:
        raise ValueError("publication would roll back or reuse a signed version")
    old_snapshot = Metadata.from_file(str(previous / f"{old.snapshot_meta.version}.snapshot.json")).signed
    new_snapshot = Metadata.from_file(str(candidate / f"{new.snapshot_meta.version}.snapshot.json")).signed
    if new_snapshot.meta["targets.json"].version < old_snapshot.meta["targets.json"].version:
        raise ValueError("publication would roll back signed targets")


def stage_channel(root, metadata, targets, mirrors, voices, output, previous_site=None, engines=None):
    if output.exists():
        raise ValueError("staging destination must be new")
    read_catalogue(mirrors)
    with TemporaryDirectory(dir=output.parent) as temporary:
        work = Path(temporary)
        candidate = work / "site"
        (candidate / "targets").mkdir(parents=True)
        published = retain_immutable_files(previous_site, candidate) if previous_site is not None else []
        bootstrap = root.read_bytes()
        signed_root = Metadata.from_bytes(bootstrap).signed
        if not isinstance(signed_root, Root):
            raise ValueError("the installer bootstrap must be TUF root metadata")
        version = signed_root.version
        bootstrap_path = candidate / f"{version}.root.json"
        if bootstrap_path.exists() and bootstrap_path.read_bytes() != bootstrap:
            raise ValueError(f"immutable published file changed: {bootstrap_path.name}")
        bootstrap_path.write_bytes(bootstrap)
        roots = [
            int(relative.name.split(".", 1)[0])
            for relative in published if relative.name.endswith(".root.json")
        ]
        fetcher = StagingFetcher(metadata, targets, candidate)
        config = UpdaterConfig()
        base = "https://channel.invalid/"
        # Studio also caches root 1..N for offline catalogue verification.
        # Start at root 1 and verify continuity through the supplied installer root.
        first_root = candidate / "1.root.json"
        try:
            anchor = first_root.read_bytes() if first_root.exists() else fetcher.download_bytes(
                base + first_root.name, config.root_max_length
            )
        except DownloadHTTPError as error:
            raise ValueError("publication must include the complete root chain starting at version 1") from error
        first = Metadata.from_bytes(anchor).signed
        if not isinstance(first, Root) or first.version != 1:
            raise ValueError("1.root.json must be root metadata of version 1")
        cache = work / "cache"
        cache.mkdir()
        client = Updater(str(cache), base, bootstrap=anchor, config=config,
                         target_base_url=base + "targets/", fetcher=fetcher)
        client.refresh()
        verified_bootstrap = cache / "root_history" / bootstrap_path.name
        if not verified_bootstrap.is_file() or verified_bootstrap.read_bytes() != bootstrap:
            raise ValueError("the installer bootstrap is not in the verified public root chain")
        if roots and Metadata.from_file(str(cache / "root.json")).signed.version < max(roots):
            raise ValueError("publication would roll back the trusted root")
        reviewed = {"mirrors.json": mirrors}
        if voices is not None:
            reviewed["rvc-voice-catalog.json"] = voices
        if engines is not None:
            reviewed["music-engine-catalog.json"] = engines
        signed = Metadata.from_file(str(cache / "targets.json")).signed
        if signed.delegations or set(signed.targets) != set(reviewed):
            raise ValueError("the signed channel must contain exactly the reviewed catalogues")
        for name, expected in reviewed.items():
            info = client.get_targetinfo(name)
            if info is None:
                raise ValueError(f"missing signed target {name}")
            verified = work / name
            client.download_target(info, str(verified))
            if verified.read_bytes() != expected.read_bytes():
                raise ValueError(f"signed target differs from reviewed file: {name}")
        if previous_site is not None:
            check_previous_timestamp(previous_site, candidate)
            for relative in published:
                if (candidate / relative).read_bytes() != (previous_site / relative).read_bytes():
                    raise ValueError(f"immutable published file changed: {relative.name}")
        candidate.rename(output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("root", "metadata", "targets", "mirrors", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--voices", type=Path)
    parser.add_argument("--engines", type=Path)
    parser.add_argument("--previous-site", type=Path, help="current public site for all releases after bootstrap")
    args = parser.parse_args()
    stage_channel(**vars(args))
