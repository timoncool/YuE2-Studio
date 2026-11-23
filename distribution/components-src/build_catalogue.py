"""Build the component catalogue from the source tables in this folder.

The tables are the reviewed data; this script assembles them and holds every entry to the
full rules (the studio itself only leaves out what it cannot use). Publishing (signature,
sequence, address checks) is publish-components.ps1.

  build_catalogue.py [output]   # default docs/components/1/components.json
"""
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PRODUCT = "yue2-studio"
KINDS = {"model", "runtime"}
LANGUAGES = ("en", "ru", "zh", "ja", "ko", "pt")
# A file that differs by platform is listed as <id>@<platform>, the names Rust's std::env::consts::OS gives.
PLATFORMS = ("windows", "linux", "macos")
ROLES = {
    "acestep-cpp": {"dit", "lm", "text_encoder", "vae"},
    "minimaxmusic-cpp": {"lm", "depth", "cond", "dit", "vae"},
    "yue2-cpp": {"backbone", "vae", "transcriber", "transcriber-base", "companion"},
    # the audio.cpp model families: a set is a family, its roles are the files by name inside its folder
    "audio.cpp": set(),
    # HOT-Step: a set per profile, its roles the files by path inside HOT-Step's folder
    "hotstep": set(),
}
MOVING = re.compile(r"/(resolve|raw|blob)/(main|master)/")


def fail(message):
    sys.exit(f"[ERROR] {message}")


def main():
    published = ROOT / "docs" / "components" / "1" / "components.json"
    target = Path(sys.argv[1]) if len(sys.argv) > 1 else published
    files = {}
    sets = {}
    voices = None
    engines = None
    for table in sorted(HERE.glob("*.json")):
        data = json.loads(table.read_text(encoding="utf-8"))
        if "engines" in data:
            if engines is not None:
                fail(f"{table.name}: the engine builds are defined twice")
            engines = data["engines"]
            for build in engines:
                where = f"{table.name}: engine {build.get('engine')} {build.get('platform')} {build.get('revision', '')[:12]}"
                if not build.get("url", "").startswith("https://") or MOVING.search(build["url"]):
                    fail(f"{where}: the archive address is not a pinned HTTPS release")
                hashes = [build.get("sha256", ""), *build.get("required", {}).values()]
                if not all(re.fullmatch(r"[0-9a-f]{64}", sha) for sha in hashes):
                    fail(f"{where}: an archive or file has no SHA-256")
                if build.get("executable") not in build.get("required", {}):
                    fail(f"{where}: the executable is not among its required files")
        if "voices" in data:
            if voices is not None:
                fail(f"{table.name}: the RVC voices are defined twice")
            voices = data["voices"]
            if voices.get("schema_version") != 1 or not voices.get("voices"):
                fail(f"{table.name}: the RVC voices need schema_version 1 and at least one voice")
        for file_id, entry in data.get("files", {}).items():
            where = f"{table.name}: file {file_id}"
            if file_id in files:
                fail(f"{where} is defined twice")
            if entry.get("kind") not in KINDS:
                fail(f"{where} has kind {entry.get('kind')!r}, not one of {sorted(KINDS)}")
            if not re.fullmatch(r"[0-9a-f]{64}", entry.get("sha256", "")):
                fail(f"{where} has no SHA-256")
            for url in [entry["url"], *entry.get("mirrors", [])]:
                if not url.startswith("https://"):
                    fail(f"{where}: {url} is not HTTPS")
                if MOVING.search(url):
                    fail(f"{where}: {url} follows a branch; pin a commit")
            files[file_id] = entry
    for file_id in files:
        base, _, platform = file_id.partition("@")
        if platform and platform not in PLATFORMS:
            fail(f"file {file_id}: the platform is one of {PLATFORMS}")
        if platform and base in files:
            fail(f"file {file_id}: {base} exists for every platform as well")
    for platform in PLATFORMS:
        paths = {}
        for file_id, entry in files.items():
            if file_id.partition("@")[2] not in ("", platform):
                continue
            other = paths.setdefault(entry["path"], (file_id, entry["sha256"]))
            if other[1] != entry["sha256"]:
                fail(f"files {other[0]} and {file_id} land at {entry['path']} on {platform} with different contents")
    for table in sorted(HERE.glob("*.json")):
        for engine, engine_sets in json.loads(table.read_text(encoding="utf-8")).get("sets", {}).items():
            sets.setdefault(engine, []).extend(engine_sets)
    for engine, engine_sets in sets.items():
        if engine not in ROLES:
            fail(f"sets for {engine}: the studio has no roles for this engine")
        ids = [model_set["id"] for model_set in engine_sets]
        if len(ids) != len(set(ids)):
            fail(f"sets for {engine}: an id is used twice in {ids}")
        for model_set in engine_sets:
            where = f"set {engine}/{model_set['id']}"
            missing = ROLES[engine] - model_set["roles"].keys()
            if missing:
                fail(f"{where} has no {sorted(missing)}")
            for role, file_id in model_set["roles"].items():
                if file_id not in files:
                    fail(f"{where} role {role} names unknown file {file_id}")
            for language in LANGUAGES:
                if language not in model_set["label"]:
                    fail(f"{where} has no {language} name")
    previous = json.loads(published.read_text(encoding="utf-8")) if published.exists() else {"sequence": 0}
    catalogue = {"product": PRODUCT, "sequence": previous["sequence"], "published": previous.get("published", ""),
                 "files": files, "sets": sets}
    if engines is not None:
        catalogue["engines"] = engines
    if voices is not None:
        catalogue["voices"] = voices
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(catalogue, ensure_ascii=False, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(f"[OK] {len(files)} files, {sum(len(s) for s in sets.values())} sets -> {target}")


if __name__ == "__main__":
    main()
