"""Build docs/components/components.json from the source tables in this folder.

The tables are the reviewed data; this script only assembles and checks them.
Publishing (signature, sequence bump, address checks) is publish-components.ps1.
"""
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def main():
    files = {}
    sets = {}
    for table in sorted(HERE.glob("*.json")):
        data = json.loads(table.read_text(encoding="utf-8"))
        for file_id, entry in data.get("files", {}).items():
            if file_id in files:
                sys.exit(f"[ERROR] {table.name}: file {file_id} is defined twice")
            if not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
                sys.exit(f"[ERROR] {table.name}: file {file_id} has no SHA-256")
            files[file_id] = entry
        for engine, engine_sets in data.get("sets", {}).items():
            sets.setdefault(engine, []).extend(engine_sets)
    for engine, engine_sets in sets.items():
        for model_set in engine_sets:
            for role, file_id in model_set["roles"].items():
                if file_id not in files:
                    sys.exit(f"[ERROR] set {engine}/{model_set['id']} role {role} names unknown file {file_id}")
            for language in ("en", "ru", "zh", "ja", "ko", "pt"):
                if language not in model_set["label"]:
                    sys.exit(f"[ERROR] set {engine}/{model_set['id']} has no {language} name")
    target = ROOT / "docs" / "components" / "components.json"
    previous = json.loads(target.read_text(encoding="utf-8")) if target.exists() else {"sequence": 0}
    catalogue = {"sequence": previous["sequence"], "published": previous.get("published", ""),
                 "files": files, "sets": sets}
    target.write_text(json.dumps(catalogue, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"[OK] {len(files)} files, {sum(len(s) for s in sets.values())} sets -> {target}")


if __name__ == "__main__":
    main()
