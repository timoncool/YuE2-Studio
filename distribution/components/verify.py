"""Check a mirror catalogue before publishing it, without building the Studio."""

import argparse
import hashlib
import json
import re
from pathlib import Path
from urllib.parse import urlsplit
from urllib.request import urlopen


def unique_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate key: {key}")
        result[key] = value
    return result


def read_catalogue(path):
    with path.open(encoding="utf-8") as stream:
        catalogue = json.load(stream, object_pairs_hook=unique_keys)
    if type(catalogue["schema"]) is not int or catalogue["schema"] != 1:
        raise ValueError("unsupported catalogue schema")
    if type(catalogue["revision"]) is not int or catalogue["revision"] < 1:
        raise ValueError("expected a positive integer revision")
    if not isinstance(catalogue["archives"], dict) or not catalogue["archives"]:
        raise ValueError("expected an archive map")
    for digest, archive in catalogue["archives"].items():
        if not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError(f"invalid digest: {digest}")
        sources = archive["sources"]
        if not isinstance(sources, list) or not sources or len(sources) != len(set(sources)):
            raise ValueError(f"expected a nonempty list of distinct mirrors for {digest}")
        for url in sources:
            parsed = urlsplit(url)
            if parsed.scheme != "https" or not parsed.hostname or parsed.username is not None or parsed.fragment:
                raise ValueError(f"invalid mirror URL for {digest}")
    return catalogue


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("catalogue", type=Path)
    parser.add_argument("--previous", type=Path, help="last published catalogue")
    parser.add_argument("--fetch", action="store_true", help="download each new mirror and verify its bytes")
    args = parser.parse_args()
    candidate = read_catalogue(args.catalogue)
    previous = read_catalogue(args.previous) if args.previous else {"revision": 0, "archives": {}}
    if candidate != previous and candidate["revision"] <= previous["revision"]:
        raise ValueError("changed catalogue requires a newer revision")
    verified = 0
    for digest, archive in candidate["archives"].items():
        for url in archive["sources"]:
            if not args.fetch or url in previous["archives"].get(digest, {}).get("sources", []):
                continue
            with urlopen(url, timeout=60) as response:
                if urlsplit(response.geturl()).scheme != "https":
                    raise ValueError("mirror redirected outside HTTPS")
                actual = hashlib.file_digest(response, "sha256").hexdigest()
            if actual != digest:
                raise ValueError(f"wrong bytes at {url}: expected {digest}, got {actual}")
            verified += 1
    print(f"revision {candidate['revision']}, {len(candidate['archives'])} archives, {verified} network hashes verified")


if __name__ == "__main__":
    main()
