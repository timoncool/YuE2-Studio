"""Stage signed TUF metadata and approved catalogues for an atomic site deployment.

Run on the publishing machine with python-tuf, using the same public root as
the installer. The signing repository can keep metadata and targets apart.
The output contains only the files fetched and verified by a fresh client.
"""
import argparse
from pathlib import Path
import shutil
from tempfile import TemporaryDirectory
from urllib.parse import urlsplit

from tuf.api.exceptions import DownloadHTTPError
from tuf.api.metadata import Metadata
from tuf.ngclient import Updater
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
        if not path.is_file():
            raise DownloadHTTPError("file not present in signing repository", 404)
        if path.resolve().parent != source.resolve():
            raise ValueError(f"file escapes signing directory: {name}")
        with path.open("rb") as reader, self.stage.joinpath(*parts).open("wb") as writer:
            while chunk := reader.read(65536):
                writer.write(chunk)
                yield chunk


def stage_channel(root, metadata, targets, mirrors, voices, output, previous_site=None):
    if output.exists():
        raise ValueError("staging destination must be new")
    read_catalogue(mirrors)
    with TemporaryDirectory(dir=output.parent) as temporary:
        work = Path(temporary)
        candidate = work / "site"
        (candidate / "targets").mkdir(parents=True)
        cache = work / "cache"
        cache.mkdir()
        client = Updater(str(cache), "https://channel.invalid/", bootstrap=root.read_bytes(),
                         target_base_url="https://channel.invalid/targets/",
                         fetcher=StagingFetcher(metadata, targets, candidate))
        client.refresh()
        reviewed = {"mirrors.json": mirrors}
        if voices is not None:
            reviewed["rvc-voice-catalog.json"] = voices
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
        # Retain root bootstrap for independent verification; only the public
        # root provided by the packager is used as the trust anchor.
        version = Metadata.from_file(str(root)).signed.version
        shutil.copyfile(root, candidate / f"{version}.root.json")
        if previous_site is not None:
            old_timestamp = previous_site / "timestamp.json"
            new_timestamp = candidate / "timestamp.json"
            if old_timestamp.read_bytes() != new_timestamp.read_bytes():
                old = Metadata.from_file(str(old_timestamp)).signed
                new = Metadata.from_file(str(new_timestamp)).signed
                if new.version <= old.version or new.snapshot_meta.version < old.snapshot_meta.version:
                    raise ValueError("publication would roll back or reuse a signed version")
                old_snapshot = Metadata.from_file(str(previous_site / f"{old.snapshot_meta.version}.snapshot.json")).signed
                new_snapshot = Metadata.from_file(str(candidate / f"{new.snapshot_meta.version}.snapshot.json")).signed
                if new_snapshot.meta["targets.json"].version < old_snapshot.meta["targets.json"].version:
                    raise ValueError("publication would roll back signed targets")
            for new_file in candidate.rglob("*"):
                if new_file.is_file() and new_file != new_timestamp:
                    old_file = previous_site / new_file.relative_to(candidate)
                    if old_file.exists() and new_file.read_bytes() != old_file.read_bytes():
                        raise ValueError(f"immutable published file changed: {new_file.name}")
        candidate.rename(output)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("root", "metadata", "targets", "mirrors", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--voices", type=Path)
    parser.add_argument("--previous-site", type=Path, help="current public site for all releases after bootstrap")
    args = parser.parse_args()
    stage_channel(**vars(args))
