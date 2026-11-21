"""Release-side acceptance, independent of a Studio build."""
from datetime import datetime, timedelta, timezone
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from securesystemslib.signer import CryptoSigner
from tuf.api.exceptions import LengthOrHashMismatchError
from tuf.api.metadata import MetaFile, Root, Snapshot, TargetFile, Targets, Timestamp

from stage_channel import stage_channel
from test_installed_channel import sign


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
            role = Targets(expires=expiry, targets={"mirrors.json": target})
            signed = sign(role, keys["targets"])
            (meta / "1.targets.json").write_bytes(signed)
            snapshot = sign(Snapshot(expires=expiry, meta={"targets.json": MetaFile.from_data(1, signed, ["sha256"])}), keys["snapshot"])
            (meta / "1.snapshot.json").write_bytes(snapshot)
            (meta / "timestamp.json").write_bytes(sign(Timestamp(expires=expiry, snapshot_meta=MetaFile.from_data(1, snapshot, ["sha256"])), keys["timestamp"]))
            (meta / "unrelated-file").write_bytes(b"must stay outside publication")
            site = work / "site"
            stage_channel(anchor, meta, targets, reviewed, None, site)
            self.assertEqual((site / "targets" / payload.name).read_bytes(), data)
            self.assertFalse((site / "unrelated-file").exists())
            self.assertEqual((site / "1.root.json").read_bytes(), anchor.read_bytes())
            stage_channel(anchor, meta, targets, reviewed, None, work / "repeat", previous_site=site)
            future = Timestamp(version=2, expires=expiry, snapshot_meta=MetaFile.from_data(1, snapshot, ["sha256"]))
            (site / "timestamp.json").write_bytes(sign(future, keys["timestamp"]))
            with self.assertRaisesRegex(ValueError, "roll back"):
                stage_channel(anchor, meta, targets, reviewed, None, work / "rollback", previous_site=site)
            self.assertFalse((work / "rollback").exists())
            payload.write_bytes(b"x" * len(data))
            bad = work / "bad"
            with self.assertRaises(LengthOrHashMismatchError):
                stage_channel(anchor, meta, targets, reviewed, None, bad)
            self.assertFalse(bad.exists())


if __name__ == "__main__":
    unittest.main()
