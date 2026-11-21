"""Release-side acceptance, independent of a Studio build."""
from contextlib import contextmanager
from datetime import datetime, timedelta, timezone
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
from tempfile import TemporaryDirectory
from threading import Thread
import time
import unittest
from urllib.error import URLError
from urllib.request import Request, urlopen

from securesystemslib.signer import CryptoSigner
from tuf.api.exceptions import LengthOrHashMismatchError, UnsignedMetadataError
from tuf.api.metadata import Metadata, MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp

from tuf.ngclient import Updater

from stage_channel import stage_channel
from test_installed_channel import sign


@contextmanager
def installed_catalogue(work, bootstrap, channel_url):
    binary = Path(os.environ["CHANNEL_QA_STUDIO"])
    package = work / "install"
    channel = package / "component-channel"
    channel.mkdir(parents=True)
    shutil.copyfile(bootstrap, channel / "root.json")
    (channel / "url.txt").write_text(channel_url, encoding="ascii")
    executable = Path(shutil.copy2(binary, package))
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    profile = work / "profile"
    env = dict(os.environ, YUE_STUDIO_DATA_ROOT=str(profile), YUE_STUDIO_PORT=str(port))
    base = f"http://127.0.0.1:{port}"

    def refresh(expected_voice):
        for _ in range(120):
            try:
                with urlopen(base + "/health", timeout=2):
                    break
            except URLError:
                time.sleep(0.25)
        else:
            raise TimeoutError("installed Studio did not start")
        for name in ("rvc_catalog_refresh", "rvc_catalog_list"):
            body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                               "params": {"name": name, "arguments": {}}}).encode()
            with urlopen(Request(base + "/mcp", body, {"Content-Type": "application/json"}),
                         timeout=30) as response:
                result = json.load(response)["result"]
                if result.get("isError"):
                    raise AssertionError(result)
                value = result["structuredContent"]
        assert value["total"] == 1 and value["voices"][0]["name"] == expected_voice, value

    with subprocess.Popen([executable], cwd=package, env=env) as studio:
        try:
            yield refresh
            assert (channel / "root.json").read_bytes() == bootstrap.read_bytes()
        finally:
            studio.terminate()
            studio.wait(timeout=20)


class Publication(unittest.TestCase):
    def test_only_verified_files_enter_the_staged_release(self):
        with TemporaryDirectory() as tmp:
            work = Path(tmp)
            meta = work / "metadata"
            targets = work / "targets"
            meta.mkdir()
            targets.mkdir()
            reviewed = Path(__file__).resolve().parents[1] / "channels/stable/mirrors.json"
            data = reviewed.read_bytes()
            expiry = datetime.now(timezone.utc) + timedelta(days=1)
            root = Root(expires=expiry)
            keys = {role: CryptoSigner.generate_ed25519() for role in root.roles}
            for role, key in keys.items():
                root.add_key(key.public_key, role)
            anchor = work / "root.json"
            anchor.write_bytes(sign(root, keys["root"]))
            target = TargetFile.from_data("mirrors.json", data, ["sha256"])
            payload = targets / (target.hashes["sha256"] + ".mirrors.json")
            payload.write_bytes(data)
            voices = Path(__file__).resolve().parents[2] / "config/rvc-voice-catalog.json"
            voices_data = voices.read_bytes()
            voice_target = TargetFile.from_data("rvc-voice-catalog.json", voices_data, ["sha256"])
            voice_filename = voice_target.hashes["sha256"] + ".rvc-voice-catalog.json"
            (targets / voice_filename).write_bytes(voices_data)
            role = Targets(expires=expiry, targets={"mirrors.json": target, "rvc-voice-catalog.json": voice_target})
            signed = sign(role, keys["targets"])
            (meta / "1.targets.json").write_bytes(signed)
            snapshot = sign(Snapshot(expires=expiry, meta={"targets.json": MetaFile.from_data(1, signed, ["sha256"])}), keys["snapshot"])
            (meta / "1.snapshot.json").write_bytes(snapshot)
            (meta / "timestamp.json").write_bytes(sign(Timestamp(expires=expiry, snapshot_meta=MetaFile.from_data(1, snapshot, ["sha256"])), keys["timestamp"]))
            (meta / "unrelated-file").write_bytes(b"must stay outside publication")
            site = work / "site"
            with self.assertRaisesRegex(ValueError, "exactly the reviewed catalogues"):
                stage_channel(anchor, meta, targets, reviewed, None, site)
            self.assertFalse(site.exists())
            stage_channel(anchor, meta, targets, reviewed, voices, site)
            self.assertEqual((site / "targets" / payload.name).read_bytes(), data)
            self.assertEqual((site / "targets" / voice_filename).read_bytes(), voices_data)
            self.assertFalse((site / "unrelated-file").exists())
            self.assertEqual((site / "1.root.json").read_bytes(), anchor.read_bytes())
            stage_channel(anchor, meta, targets, reviewed, voices, work / "repeat", previous_site=site)
            future = Timestamp(version=2, expires=expiry, snapshot_meta=MetaFile.from_data(1, snapshot, ["sha256"]))
            (site / "timestamp.json").write_bytes(sign(future, keys["timestamp"]))
            with self.assertRaisesRegex(ValueError, "roll back"):
                stage_channel(anchor, meta, targets, reviewed, voices, work / "rollback", previous_site=site)
            self.assertFalse((work / "rollback").exists())
            payload.write_bytes(b"x" * len(data))
            bad = work / "bad"
            with self.assertRaises(LengthOrHashMismatchError):
                stage_channel(anchor, meta, targets, reviewed, voices, bad)
            self.assertFalse(bad.exists())


    def test_rotated_replacement_serves_both_bootstraps_and_an_inflight_download(self):
        with TemporaryDirectory() as tmp:
            work = Path(tmp)
            reviewed = Path(__file__).resolve().parents[1] / "channels/stable/mirrors.json"
            expiry = datetime.now(timezone.utc) + timedelta(days=1)
            studio_binary = os.environ.get("CHANNEL_QA_STUDIO")
            bundled = Path(__file__).resolve().parents[2] / "config/rvc-voice-catalog.json"
            voice = json.loads(bundled.read_bytes())["voices"][0]

            def publish(version, old_root_key=None, previous=None):
                metadata, targets = work / f"meta-{version}", work / f"targets-{version}"
                metadata.mkdir()
                targets.mkdir()
                root = Root(version=version, expires=expiry)
                keys = {role: CryptoSigner.generate_ed25519() for role in root.roles}
                for role, key in keys.items():
                    root.add_key(key.public_key, role)
                anchor = work / f"bootstrap-{version}.json"
                envelope = Metadata(root)
                envelope.sign(keys["root"])
                if old_root_key is not None:
                    envelope.sign(old_root_key, append=True)
                anchor.write_bytes(envelope.to_bytes())

                catalogue = work / f"reviewed-{version}.json"
                catalogue.write_bytes(reviewed.read_bytes() + b" " * version)
                target = TargetFile.from_data("mirrors.json", catalogue.read_bytes(), ["sha256"])
                name = target.hashes["sha256"] + ".mirrors.json"
                (targets / name).write_bytes(catalogue.read_bytes())
                voice_file = work / f"voices-{version}.json"
                voice_data = json.dumps({"schema_version": 1, "voices": [
                    dict(voice, name=f"Rotation fixture {version}"),
                ]}).encode()
                voice_file.write_bytes(voice_data)
                voice_target = TargetFile.from_data("rvc-voice-catalog.json", voice_data, ["sha256"])
                (targets / (voice_target.hashes["sha256"] + ".rvc-voice-catalog.json")).write_bytes(voice_data)
                signed = sign(Targets(version=version, expires=expiry, targets={
                    "mirrors.json": target, "rvc-voice-catalog.json": voice_target,
                }), keys["targets"])
                (metadata / f"{version}.targets.json").write_bytes(signed)
                snapshot = sign(Snapshot(version=version, expires=expiry, meta={
                    "targets.json": MetaFile.from_data(version, signed, ["sha256"]),
                }), keys["snapshot"])
                (metadata / f"{version}.snapshot.json").write_bytes(snapshot)
                (metadata / "timestamp.json").write_bytes(sign(Timestamp(
                    version=version, expires=expiry,
                    snapshot_meta=MetaFile.from_data(version, snapshot, ["sha256"]),
                ), keys["timestamp"]))
                site = work / f"release-{version}"
                stage_channel(anchor, metadata, targets, catalogue, voice_file, site, previous_site=previous)
                return site, anchor, keys["root"], catalogue

            first, bootstrap1, old_key, catalogue1 = publish(1)
            second, bootstrap2, _, catalogue2 = publish(2, old_key, first)
            invalid = Metadata.from_bytes(bootstrap2.read_bytes())
            del invalid.signatures[old_key.public_key.keyid]
            disconnected = work / "disconnected.json"
            disconnected.write_bytes(invalid.to_bytes())
            rejected = work / "rejected"
            with self.assertRaises(UnsignedMetadataError):
                stage_channel(disconnected, work / "meta-2", work / "targets-2",
                              catalogue2, work / "voices-2.json", rejected, previous_site=first)
            self.assertFalse(rejected.exists())

            pointer = work / "current"
            pointer.write_text(first.name, encoding="ascii")

            class Handler(SimpleHTTPRequestHandler):
                def __init__(self, *args):
                    release = work / pointer.read_text(encoding="ascii")
                    super().__init__(*args, directory=release)

                def log_message(self, *_args):
                    pass

            with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
                base = f"http://127.0.0.1:{server.server_port}/"
                if studio_binary:
                    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                    tls.load_cert_chain(os.environ["CHANNEL_QA_CERT"], os.environ["CHANNEL_QA_TLS_KEY"])
                    server.socket = tls.wrap_socket(server.socket, server_side=True)
                    base = f"https://localhost:{server.server_port}/"
                thread = Thread(target=server.serve_forever, daemon=True)
                thread.start()

                def updater(name, bootstrap=None):
                    cache = work / name
                    cache.mkdir(exist_ok=True)
                    return Updater(str(cache), base,
                                   bootstrap=bootstrap.read_bytes() if bootstrap else None,
                                   target_base_url=base + "targets/")

                def promote():
                    next_pointer = work / "next"
                    next_pointer.write_text(second.name, encoding="ascii")
                    os.replace(next_pointer, pointer)

                try:
                    cached = updater("profile", bootstrap1)
                    cached.refresh()
                    old_info = cached.get_targetinfo("mirrors.json")
                    if studio_binary:
                        with installed_catalogue(work / "existing", bootstrap1, base) as refresh:
                            refresh("Rotation fixture 1")
                            promote()
                            refresh("Rotation fixture 2")
                        with installed_catalogue(work / "fresh", bootstrap1, base) as refresh:
                            refresh("Rotation fixture 2")
                    else:
                        promote()
                    for path in ("1.root.json", "1.snapshot.json", "1.targets.json"):
                        with urlopen(base + path, timeout=3) as old_response:
                            self.assertEqual(old_response.read(), (first / path).read_bytes())
                    saved_old = Path(cached.download_target(old_info, str(work / "inflight")))
                    self.assertEqual(saved_old.read_bytes(), catalogue1.read_bytes())
                    for client in (updater("profile"), updater("old-installed", bootstrap1),
                                   updater("new-installed", bootstrap2)):
                        client.refresh()
                        new_info = client.get_targetinfo("mirrors.json")
                        saved_new = Path(client.download_target(new_info, str(work / "updated")))
                        self.assertEqual(saved_new.read_bytes(), catalogue2.read_bytes())
                finally:
                    server.shutdown()
                    thread.join()


if __name__ == "__main__":
    unittest.main()
