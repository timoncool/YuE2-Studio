import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile
from urllib.request import urlopen
from zipfile import ZipFile

import zstandard


work = Path(__file__).resolve().parent
proof = work
packages = json.loads((proof / "ort-132-ventura-conda-lock.json").read_text())
tree = json.loads((proof / "ort-132-ventura-dylib-tree.json").read_text())
libraries = tree["dlls"]
names = set()
for row in libraries.values():
    names.update(dep.split("/")[1] for dep in row["non_system_dependencies"])
links = {name: tree["aliases"][name] for name in names if name not in libraries}
links["libonnxruntime.dylib"] = "libonnxruntime.1.32.0.dylib"
assert all(target in libraries for target in links.values())
files = {}
notices = []
for package in packages:
    with urlopen(package["url"], timeout=60) as response:
        blob = response.read()
    assert hashlib.sha256(blob).hexdigest() == package["sha256"]
    seen_licenses = set()
    with ZipFile(io.BytesIO(blob)) as archive:
        for member in archive.namelist():
            if not member.endswith(".tar.zst"):
                continue
            with archive.open(member) as compressed, zstandard.ZstdDecompressor().stream_reader(compressed) as stream, tarfile.open(fileobj=stream,mode="r|") as tar:
                for item in tar:
                    if not item.isfile():
                        continue
                    if item.name in tree["licenses"][package["name"]]:
                        notices.extend([f'\n{package["name"]} / {item.name}\n'.encode(),tar.extractfile(item).read(),b"\n"])
                        seen_licenses.add(item.name)
                    if not item.name.startswith("lib/"):
                        continue
                    name = item.name[4:]
                    if name in libraries:
                        assert name not in files
                        data = tar.extractfile(item).read()
                        assert hashlib.sha256(data).hexdigest() == libraries[name]["sha256"]
                        files[name] = data
    assert seen_licenses == set(tree["licenses"][package["name"]])
assert set(files) == set(libraries)
files["ONNX_RUNTIME_LICENSES"] = b"".join(notices)
target = work / "onnxruntime-osx-arm64-ventura-1.32.0.tgz"
tmp = target.with_suffix(".part")
with tmp.open("wb") as raw, gzip.GzipFile(fileobj=raw,filename="",mode="wb",mtime=0) as compressed, tarfile.open(fileobj=compressed,mode="w|") as tar:
    for name, data in sorted(files.items()):
        header = tarfile.TarInfo(name)
        header.size = len(data)
        header.mode = 0o644
        tar.addfile(header,io.BytesIO(data))
    for name, dest in sorted(links.items()):
        header = tarfile.TarInfo(name)
        header.type = tarfile.SYMTYPE
        header.linkname = dest
        header.mode = 0o777
        tar.addfile(header)
with tarfile.open(tmp,"r:gz") as tar:
    assert set(tar.getnames()) == files.keys() | links.keys()
    for name in tar.getnames():
        assert tar.extractfile(name).read() == files[links.get(name,name)]
tmp.replace(target)
result = dict(archive=target.name,sha256=hashlib.sha256(target.read_bytes()).hexdigest(),bytes=target.stat().st_size,
              libraries=len(libraries),loader_aliases=links,license_bytes=len(files["ONNX_RUNTIME_LICENSES"]))
(proof/"ort-132-ventura-package.json").write_text(json.dumps(result,indent=2)+"\n")
print(result)
