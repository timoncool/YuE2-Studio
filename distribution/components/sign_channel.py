"""Provision and use the local Windows signer for the Studio TUF channel."""

import argparse
import base64
import ctypes
from ctypes import wintypes
from datetime import datetime, timedelta, timezone
from getpass import GetPassWarning, getpass
import json
import os
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
import warnings

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.scrypt import Scrypt
from securesystemslib.signer import CryptoSigner
from tuf.api.exceptions import RepositoryError
from tuf.api.metadata import Metadata, MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp


ROLE_DAYS = {"root": 365, "targets": 90, "snapshot": 30, "timestamp": 7}
CATALOGUES = {
    "mirrors": "mirrors.json",
    "voices": "rvc-voice-catalog.json",
    "engines": "music-engine-catalog.json",
}
BACKUP_HEADER = b"Studio TUF signer backup v1\n"


class DataBlob(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("data", ctypes.POINTER(ctypes.c_ubyte))]


def dpapi(data, *, protect):
    if os.name != "nt":
        raise ValueError("the local signing store requires Windows CurrentUser DPAPI")
    crypt32 = ctypes.WinDLL("crypt32", use_last_error=True)
    crypt = crypt32.CryptProtectData if protect else crypt32.CryptUnprotectData
    crypt.argtypes = [ctypes.POINTER(DataBlob), ctypes.c_void_p, ctypes.c_void_p,
                      ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(DataBlob)]
    crypt.restype = wintypes.BOOL
    buffer = (ctypes.c_ubyte * len(data)).from_buffer_copy(data)
    source, result = DataBlob(len(data), buffer), DataBlob()
    release = ctypes.WinDLL("kernel32", use_last_error=True).LocalFree
    release.argtypes = [ctypes.c_void_p]
    release.restype = ctypes.c_void_p
    # CRYPTPROTECT_UI_FORBIDDEN, CurrentUser scope via absence of LOCAL_MACHINE.
    if not crypt(ctypes.byref(source), None, None, None, None, 1, ctypes.byref(result)):
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        return ctypes.string_at(result.data, result.size)
    finally:
        release(result.data)


def new_destination(destination):
    if os.path.lexists(destination):
        raise FileExistsError(f"destination already exists: {destination}")


def put_new_file(path, data):
    new_destination(path)
    with TemporaryDirectory(dir=path.parent) as temp:
        candidate = Path(temp) / "candidate"
        candidate.write_bytes(data)
        # On Windows rename refuses to replace an existing destination.
        candidate.rename(path)


def signed_bytes(role, signer):
    metadata = Metadata(role)
    metadata.sign(signer)
    return metadata.to_bytes()


def expiry(now, role):
    return now + timedelta(days=ROLE_DAYS[role])


def read_signers(root_bytes, encoded_keys):
    root = Metadata.from_bytes(root_bytes)
    if not isinstance(root.signed, Root) or root.signed.version != 1 or not root.signed.consistent_snapshot:
        raise ValueError("expected a consistent-snapshot root version 1")
    root.verify_delegate("root", root)
    keys = json.loads(encoded_keys)
    if set(keys) != set(ROLE_DAYS):
        raise ValueError("unexpected roles in the signing store")
    signers = {}
    for role, private in keys.items():
        pem = base64.b64decode(private, validate=True)
        signer = CryptoSigner(serialization.load_pem_private_key(pem, password=None))
        authorised = root.signed.roles[role]
        if authorised.keyids != [signer.public_key.keyid] or authorised.threshold != 1:
            raise ValueError(f"signing key does not match the reviewed root for {role}")
        signers[role] = signer
    if len({signer.public_key.keyid for signer in signers.values()}) != len(ROLE_DAYS):
        raise ValueError("each role requires a separate key")
    return root, signers


def save_store(store, root_bytes, encoded_keys):
    new_destination(store)
    with TemporaryDirectory(dir=store.parent) as temp:
        candidate = Path(temp) / "candidate"
        candidate.mkdir()
        (candidate / "root.json").write_bytes(root_bytes)
        (candidate / "keys.dpapi").write_bytes(dpapi(encoded_keys, protect=True))
        candidate.rename(store)


def provision(store):
    new_destination(store)
    root = Root(expires=expiry(datetime.now(timezone.utc), "root"), consistent_snapshot=True)
    signers = {role: CryptoSigner.generate_ed25519() for role in ROLE_DAYS}
    for role, signer in signers.items():
        root.add_key(signer.public_key, role)
    keys = {role: base64.b64encode(signer.private_bytes).decode("ascii") for role, signer in signers.items()}
    save_store(store, signed_bytes(root, signers["root"]), json.dumps(keys).encode())


def backup_cipher(password, salt):
    if len(password) < 16:
        raise ValueError("use a recovery passphrase of at least 16 characters")
    key = Scrypt(salt=salt, length=32, n=2**17, r=8, p=1).derive(password.encode())
    return AESGCM(key)


def backup(store, output, password):
    new_destination(output)
    keys = dpapi((store / "keys.dpapi").read_bytes(), protect=False)
    read_signers((store / "root.json").read_bytes(), keys)
    salt, nonce = os.urandom(16), os.urandom(12)
    ciphertext = backup_cipher(password, salt).encrypt(nonce, keys, BACKUP_HEADER)
    put_new_file(output, BACKUP_HEADER + salt + nonce + ciphertext)


def restore(store, root_path, backup_path, password):
    new_destination(store)
    data = backup_path.read_bytes()
    body = data.removeprefix(BACKUP_HEADER)
    if len(body) == len(data) or len(body) < 44:
        raise ValueError("invalid backup format")
    salt, nonce, ciphertext = body[:16], body[16:28], body[28:]
    try:
        keys = backup_cipher(password, salt).decrypt(nonce, ciphertext, BACKUP_HEADER)
    except InvalidTag:
        raise ValueError("backup authentication failed") from None
    root_bytes = root_path.read_bytes()
    read_signers(root_bytes, keys)
    save_store(store, root_bytes, keys)


def read_previous(site, root, root_bytes):
    if (site / "1.root.json").read_bytes() != root_bytes or (site / "2.root.json").exists():
        raise ValueError("previous site does not use this unrotated root")
    timestamp = Metadata.from_bytes((site / "timestamp.json").read_bytes())
    if not isinstance(timestamp.signed, Timestamp):
        raise ValueError("previous timestamp has the wrong role")
    root.verify_delegate("timestamp", timestamp)
    info = timestamp.signed.snapshot_meta
    snapshot_bytes = (site / f"{info.version}.snapshot.json").read_bytes()
    info.verify_length_and_hashes(snapshot_bytes)
    snapshot = Metadata.from_bytes(snapshot_bytes)
    if not isinstance(snapshot.signed, Snapshot) or snapshot.signed.version != info.version:
        raise ValueError("previous snapshot version or role does not match")
    root.verify_delegate("snapshot", snapshot)
    info = snapshot.signed.meta["targets.json"]
    targets_bytes = (site / f"{info.version}.targets.json").read_bytes()
    info.verify_length_and_hashes(targets_bytes)
    targets = Metadata.from_bytes(targets_bytes)
    if not isinstance(targets.signed, Targets) or targets.signed.version != info.version or targets.signed.delegations:
        raise ValueError("previous targets version or role does not match")
    root.verify_delegate("targets", targets)
    versions = {"timestamp": timestamp.signed.version, "snapshot": snapshot.signed.version,
                "targets": targets.signed.version}
    return versions, set(targets.signed.targets)


def sign(store, output, catalogues, previous=None):
    new_destination(output)
    root_path = store / "root.json"
    root_bytes = root_path.read_bytes()
    keys = dpapi((store / "keys.dpapi").read_bytes(), protect=False)
    root, signers = read_signers(root_bytes, keys)
    now = datetime.now(timezone.utc)
    if root.signed.expires <= expiry(now, "timestamp"):
        raise ValueError("root expires before the new timestamp, renew through the root ceremony")
    versions = dict.fromkeys(("targets", "snapshot", "timestamp"), 0)
    if previous is not None:
        versions, previously_published = read_previous(previous, root, root_bytes)
        if not previously_published <= catalogues.keys():
            raise ValueError("include all previously published catalogues")
    # Build the whole signed candidate before making its destination visible.
    with TemporaryDirectory(dir=output.parent) as temp:
        candidate = Path(temp) / "signed"
        metadata, targets = candidate / "metadata", candidate / "targets"
        metadata.mkdir(parents=True)
        targets.mkdir()
        (metadata / "1.root.json").write_bytes(root_bytes)
        specs = {}
        for name, path in catalogues.items():
            data = path.read_bytes()
            info = TargetFile.from_data(name, data, ["sha256"])
            specs[name] = info
            (targets / f"{info.hashes['sha256']}.{name}").write_bytes(data)
        targets_version = versions["targets"] + 1
        targets_bytes = signed_bytes(Targets(version=targets_version, expires=min(expiry(now, "targets"), root.signed.expires),
                                              targets=specs), signers["targets"])
        (metadata / f"{targets_version}.targets.json").write_bytes(targets_bytes)
        snapshot_version = versions["snapshot"] + 1
        snapshot_bytes = signed_bytes(Snapshot(version=snapshot_version, expires=min(expiry(now, "snapshot"), root.signed.expires),
            meta={"targets.json": MetaFile.from_data(targets_version, targets_bytes, ["sha256"])}), signers["snapshot"])
        (metadata / f"{snapshot_version}.snapshot.json").write_bytes(snapshot_bytes)
        timestamp = Timestamp(version=versions["timestamp"] + 1, expires=expiry(now, "timestamp"),
                              snapshot_meta=MetaFile.from_data(snapshot_version, snapshot_bytes, ["sha256"]))
        (metadata / "timestamp.json").write_bytes(signed_bytes(timestamp, signers["timestamp"]))
        candidate.rename(output)


def ask_passphrase(prompt):
    with warnings.catch_warnings():
        warnings.simplefilter("error", GetPassWarning)
        try:
            return getpass(prompt)
        except GetPassWarning:
            raise ValueError("recovery requires a console with concealed input") from None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--store", type=Path, required=True, help="private Windows signing store")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("provision")
    commands.add_parser("backup").add_argument("--output", type=Path, required=True)
    recover = commands.add_parser("restore")
    recover.add_argument("--backup", type=Path, required=True)
    recover.add_argument("--root", type=Path, required=True)
    for command in ("initial", "update"):
        signing = commands.add_parser(command)
        signing.add_argument("--output", type=Path, required=True)
        for argument in CATALOGUES:
            signing.add_argument(f"--{argument}", type=Path, required=argument == "mirrors")
        if command == "update":
            signing.add_argument("--previous-site", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("requires Windows")
    try:
        if args.command == "provision":
            provision(args.store)
        elif args.command == "backup":
            password = ask_passphrase("Recovery passphrase: ")
            if password != ask_passphrase("Confirm recovery passphrase: "):
                raise ValueError("recovery passphrases do not match")
            backup(args.store, args.output, password)
        elif args.command == "restore":
            restore(args.store, args.root, args.backup, ask_passphrase("Recovery passphrase: "))
        else:
            catalogues = {name: getattr(args, arg) for arg, name in CATALOGUES.items() if getattr(args, arg)}
            sign(args.store, args.output, catalogues, getattr(args, "previous_site", None))
    except (OSError, ValueError, RepositoryError) as error:
        sys.exit(f"Signing failed: {error}")


if __name__ == "__main__":
    main()
