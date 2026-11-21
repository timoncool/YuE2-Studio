"""Test operational renewal against an isolated Git remote and Windows key store."""

from datetime import datetime, timedelta, timezone
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from tuf.api.metadata import Metadata

import renew_channel as renewal
import sign_channel as signer


class PastDay(datetime):
    @classmethod
    def now(cls, tz=None):
        return datetime.now(tz) - timedelta(days=100)


@unittest.skipUnless(os.name == "nt", "Windows DPAPI")
class RenewalTest(unittest.TestCase):
    def test_expired_recovery_fresh_skip_and_concurrent_main_write(self):
        with TemporaryDirectory() as temporary:
            work = Path(temporary)
            source, remote = work / "source", work / "remote.git"
            source.mkdir()
            renewal.git(source, "init", "-b", "main")
            renewal.git(source, "config", "core.autocrlf", "false")
            renewal.git(work, "init", "--bare", "-b", "main", str(remote))
            renewal.git(source, "remote", "add", "origin", remote.as_uri())
            store = work / "store"
            signer.provision(store)
            root_digest = hashlib.sha256((store / "root.json").read_bytes()).hexdigest()
            mirrors = Path(__file__).resolve().parents[1] / "channels/stable/mirrors.json"
            inputs = {"mirrors.json": mirrors}
            signed = work / "signed"
            with patch.object(signer, "datetime", PastDay):
                signer.sign(store, signed, inputs)
            for metadata in ("timestamp.json", "1.snapshot.json", "1.targets.json"):
                expired = Metadata.from_bytes((signed / "metadata" / metadata).read_bytes()).signed
                self.assertLess(expired.expires, datetime.now(timezone.utc))
            site = source / renewal.SITE
            shutil.copytree(signed / "metadata", site)
            shutil.copytree(signed / "targets", site / "targets")
            renewal.git(source, "add", "--", renewal.SITE)

            def commit(name):
                renewal.git(source, "-c", "user.name=Tester", "-c", "user.email=test@example.invalid",
                            "commit", "-m", name)
                renewal.git(source, "push", "origin", "main")
                return renewal.git(source, "rev-parse", "HEAD")

            old = commit("Initial channel, timestamp expired")
            published = work / "pages-timestamp.json"
            published.write_bytes(b"deployment has not caught up")
            with self.assertRaisesRegex(ValueError, "GitHub Pages timestamp differs"):
                renewal.renew(remote.as_uri(), store, work, apply=True, published_timestamp=published.as_uri())
            published.write_bytes((site / "timestamp.json").read_bytes())
            self.assertEqual(renewal.renew(remote.as_uri(), store, work, apply=False,
                                          published_timestamp=published.as_uri()), "staged")
            apply = lambda value: renewal.renew(remote.as_uri(), store, work, apply=value, root_sha256=root_digest)
            self.assertEqual(renewal.git(remote, "rev-parse", renewal.BRANCH), old)

            original_git = renewal.git
            competitor = None

            def push_with_race(repo, *args):
                nonlocal competitor
                if args[0] == "push" and any(arg.startswith("--force-with-lease=") for arg in args):
                    (source / "other.txt").write_text("unrelated concurrent change")
                    original_git(source, "add", "other.txt")
                    competitor = commit("Concurrent main write")
                return original_git(repo, *args)

            with patch.object(renewal, "git", push_with_race):
                with self.assertRaises(subprocess.CalledProcessError):
                    apply(True)
            self.assertEqual(renewal.git(remote, "rev-parse", renewal.BRANCH), competitor)
            # Recovery uses the encrypted portable backup and the same reviewed public root.
            backup = work / "backup"
            password = "fixture passphrase only"
            signer.backup(store, backup, password)
            recovered = work / "recovered"
            signer.restore(recovered, store / "root.json", backup, password)
            store = recovered
            self.assertEqual(apply(True), "submitted")
            new = renewal.git(remote, "rev-parse", renewal.BRANCH)
            self.assertNotEqual(new, competitor)
            self.assertEqual(renewal.git(remote, "rev-parse", f"{new}^"), competitor)
            timestamp = renewal.git(remote, "show", f"{new}:{renewal.SITE}/timestamp.json")
            self.assertEqual(Metadata.from_bytes(timestamp.encode()).signed.version, 2)
            self.assertEqual(apply(True), "fresh")
            self.assertEqual(renewal.git(remote, "rev-parse", renewal.BRANCH), new)


if __name__ == "__main__":
    unittest.main()
