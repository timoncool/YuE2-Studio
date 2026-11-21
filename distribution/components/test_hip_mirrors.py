"""Verify both HIP download attempts in the same Studio test build.

Run with the music-server library test executable and the published gfx1036 ZIP.
The fixture has an isolated data directory and one-use TLS/TUF keys. DNS is
overridden only in the test client's transport, which never contacts GitHub.
"""

from datetime import datetime, timedelta, timezone
from functools import partial
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import time

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID
from securesystemslib.signer import CryptoSigner
from tuf.api.metadata import MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp
from test_installed_channel import sign

SHA = "7ccf312e5236c190b25bca9d119cc8e2062ac49fa7fde228a5aabde1aa5f085c"
CANONICAL = "/timoncool/yue2.cpp/releases/download/studio-hip-188d13b-4e38eee/yue2-windows-gfx1036.zip"


def tls_files(root):
    now = datetime.now(timezone.utc)
    ca_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    tls_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    ca_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Temporary Studio HIP QA")])
    tls_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "localhost")])

    def certificate(subject, key):
        return (x509.CertificateBuilder().subject_name(subject).issuer_name(ca_name)
                .public_key(key.public_key()).serial_number(x509.random_serial_number())
                .not_valid_before(now - timedelta(minutes=1)).not_valid_after(now + timedelta(hours=1)))

    ca = certificate(ca_name, ca_key).add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True).sign(ca_key, hashes.SHA256())
    tls = (certificate(tls_name, tls_key)
           .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
           .add_extension(x509.SubjectAlternativeName([x509.DNSName("localhost"), x509.DNSName("github.com")]), critical=False)
           .sign(ca_key, hashes.SHA256()))
    (root / "ca.pem").write_bytes(ca.public_bytes(serialization.Encoding.PEM))
    (root / "tls.pem").write_bytes(tls.public_bytes(serialization.Encoding.PEM))
    (root / "tls.key").write_bytes(tls_key.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))


def verify(work, binary, archive):
    with archive.open("rb") as stream:
        assert hashlib.file_digest(stream, "sha256").hexdigest() == SHA
    tls_files(work)
    staged = work / "portable"
    channel = staged / "component-channel"
    channel.mkdir(parents=True)
    copied = Path(shutil.copy2(binary, staged))
    original_digest = hashlib.sha256(copied.read_bytes()).hexdigest()
    site = work / "origin"
    (site / "targets").mkdir(parents=True)
    shutil.copyfile(archive, site / "current.zip")
    requested = []

    class Handler(SimpleHTTPRequestHandler):
        def log_request(self, code="-", size="-"):
            requested.append((self.path, int(code)))

    keys = {role: CryptoSigner.generate_ed25519() for role in ("root", "targets", "snapshot", "timestamp")}
    expires = datetime.now(timezone.utc) + timedelta(hours=1)
    root = Root(expires=expires)
    for role, key in keys.items():
        root.add_key(key.public_key, role)
    (channel / "root.json").write_bytes(sign(root, keys["root"]))

    with ThreadingHTTPServer(("127.0.0.1", 0), partial(Handler, directory=site)) as server:
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(work / "tls.pem", work / "tls.key")
        server.socket = context.wrap_socket(server.socket, server_side=True)
        base = f"https://localhost:{server.server_port}/"
        (channel / "url.txt").write_text(base, encoding="utf-8")
        (work / "port").write_text(str(server.server_port), encoding="ascii")
        threading.Thread(target=server.serve_forever, daemon=True).start()

        def publish(revision, source):
            import json
            name = "mirrors.json"
            data = json.dumps({"schema": 1, "revision": revision,
                "archives": {SHA: {"sources": [base + source]}}}).encode()
            target = TargetFile.from_data(name, data, ["sha256"])
            (site / "targets" / f"{target.hashes['sha256']}.{name}").write_bytes(data)
            signed = sign(Targets(version=revision, expires=expires, targets={name: target}), keys["targets"])
            (site / f"{revision}.targets.json").write_bytes(signed)
            snap = sign(Snapshot(version=revision, expires=expires,
                meta={"targets.json": MetaFile.from_data(revision, signed, ["sha256"])}), keys["snapshot"])
            (site / f"{revision}.snapshot.json").write_bytes(snap)
            timestamp = sign(Timestamp(version=revision, expires=expires,
                snapshot_meta=MetaFile.from_data(revision, snap, ["sha256"])), keys["timestamp"])
            (site / "next-timestamp").write_bytes(timestamp)
            (site / "next-timestamp").replace(site / "timestamp.json")

        publish(1, "gone.zip")
        env = dict(os.environ, YUE_HIP_MIRROR_QA=str(work), YUE_STUDIO_DATA_ROOT=str(work / "studio"))
        child = subprocess.Popen([copied, "--ignored", "--exact",
            "engine_hip::tests::pinned_hip_installs_after_the_signed_mirror_changes", "--nocapture"], env=env)
        try:
            deadline = time.monotonic() + 45
            while not (work / "first-failed").exists():
                assert child.poll() is None and time.monotonic() < deadline, requested
                time.sleep(.05)
            assert (CANONICAL, 404) in requested and ("/gone.zip", 404) in requested, requested
            publish(2, "current.zip")
            (work / "second-published").touch()
            assert child.wait(timeout=120) == 0
            assert any(path == "/current.zip" and code in (200, 206) for path, code in requested), requested
            assert hashlib.sha256(copied.read_bytes()).hexdigest() == original_digest
            print(f"PASS same binary/PID {child.pid}, canonical and old mirror 404, signed replacement HIP ZIP installed")
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=10)
            server.shutdown()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="hip-mirror-qa-") as folder:
        verify(Path(folder), *(Path(arg).resolve(strict=True) for arg in sys.argv[1:3]))
