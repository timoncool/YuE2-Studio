"""Exercise actual Windows DPAPI and a portable recovery backup with disposable keys."""

import os
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from tuf.api.exceptions import LengthOrHashMismatchError
from tuf.api.metadata import Metadata

import sign_channel as signer
from stage_channel import stage_channel


@unittest.skipUnless(os.name == "nt", "Windows DPAPI")
class SigningTest(unittest.TestCase):
    def test_initial_update_and_restore_under_same_public_root(self):
        distribution = Path(__file__).resolve().parents[1]
        reviewed = distribution / "channels/stable/mirrors.json"
        voices = distribution.parent / "config/rvc-voice-catalog.json"
        with TemporaryDirectory() as temporary:
            work = Path(temporary)
            store, recovered = work / "private", work / "recovered"
            initial, update, after_recovery = (work / label for label in ("initial", "update", "after-recovery"))
            version1, version2, version3 = (work / f"version{number}" for number in (1, 2, 3))
            archive = work / "backup"
            password = "disposable test recovery only"
            signer.provision(store)
            root = store / "root.json"
            public_root = root.read_bytes()
            encrypted = (store / "keys.dpapi").read_bytes()
            with self.assertRaises(FileExistsError):
                signer.provision(store)
            self.assertEqual((store / "keys.dpapi").read_bytes(), encrypted)
            self.assertNotIn(b"PRIVATE KEY", encrypted)
            catalogues = {"mirrors.json": reviewed, "rvc-voice-catalog.json": voices}
            signer.sign(store, initial, catalogues)
            stage_channel(root, initial / "metadata", initial / "targets", reviewed, voices, version1)
            with self.assertRaisesRegex(ValueError, "include all"):
                signer.sign(store, update, {"mirrors.json": reviewed}, version1)
            changed = work / "changed.json"
            changed.write_bytes(reviewed.read_bytes() + b"\n")
            catalogues["mirrors.json"] = changed
            signer.sign(store, update, catalogues, version1)
            stage_channel(root, update / "metadata", update / "targets", changed, voices, version2, version1)
            self.assertEqual(Metadata.from_bytes((version2 / "timestamp.json").read_bytes()).signed.version, 2)
            snapshot = version2 / "2.snapshot.json"
            intact = snapshot.read_bytes()
            snapshot.write_bytes(intact + b"\n")
            with self.assertRaises(LengthOrHashMismatchError):
                signer.sign(store, after_recovery, catalogues, version2)
            self.assertFalse(after_recovery.exists())
            snapshot.write_bytes(intact)
            signer.backup(store, archive, password)
            self.assertNotIn(b"PRIVATE KEY", archive.read_bytes())
            with self.assertRaisesRegex(ValueError, "authentication failed"):
                signer.restore(recovered, root, archive, "a different recovery passphrase")
            self.assertFalse(recovered.exists())
            # The second machine has only public files, the portable backup and its
            # passphrase. Failing any DPAPI decrypt here proves restore needs no old profile.
            independent_root = work / "reviewed-root.json"
            independent_root.write_bytes(public_root)
            store.rename(work / "unavailable")
            dpapi = signer.dpapi

            def protect_only(data, *, protect):
                self.assertTrue(protect)
                return dpapi(data, protect=protect)

            with patch.object(signer, "dpapi", protect_only):
                signer.restore(recovered, independent_root, archive, password)
            self.assertEqual((recovered / "root.json").read_bytes(), public_root)
            with self.assertRaises(FileExistsError):
                signer.restore(recovered, independent_root, archive, password)
            signer.sign(recovered, after_recovery, catalogues, version2)
            stage_channel(independent_root, after_recovery / "metadata", after_recovery / "targets",
                          changed, voices, version3, version2)
            self.assertEqual(Metadata.from_bytes((version3 / "timestamp.json").read_bytes()).signed.version, 3)
            self.assertTrue((version3 / "1.targets.json").is_file())
            self.assertTrue((version3 / "2.targets.json").is_file())


if __name__ == "__main__":
    unittest.main()
