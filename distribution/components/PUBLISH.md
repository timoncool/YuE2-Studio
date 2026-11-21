# Publish a Studio component catalogue

The Studio executable reads the public trust anchor from
`component-channel/root.json` next to the executable. Its `url.txt` contains
the HTTPS directory ending in a slash that serves `timestamp.json`.
The Studio expects the signed targets under `targets/` inside that directory.

1. Update the reviewed mirror catalogue. Run `verify.py candidate/mirrors.json --previous previous/mirrors.json --fetch`
   to download and check each changed mirror before signing. For the first
   publication omit `--previous` to verify all mirrors. Keep older archives
   available during rollout.
2. Sign the catalogue in the project's TUF repository. The root installed
   with the Studio is the input to staging. The signing process produces
   consistent-snapshot metadata and hash-prefixed target files. Use the
   repository's normal threshold, expiry, renewal and key-rotation process.
3. Install the release-side dependencies `tuf==7.0.1` and
   `securesystemslib[crypto]==1.6.0` in a Python virtual environment.
   Stage the signed update using the checkout of the currently published
   channel as the comparison point:

   ```sh
   python distribution/components/stage_channel.py \
     --root installer/component-channel/root.json \
     --metadata signed/metadata --targets signed/targets \
     --mirrors distribution/channels/stable/mirrors.json \
     --previous-site published --output incoming
   ```

   On initial deployment omit `--previous-site`. If a voice catalogue is
   offered, also pass `--voices config/rvc-voice-catalog.json` on every
   deployment. The verified target must match its reviewed file byte for byte.
4. Apply the staged files as an overlay to the current channel in a single
   versioned deployment. Keep older numbered roles and hashed targets for
   clients already downloading. For a host with individually uploaded files,
   upload immutable files first, verify they are reachable, and replace
   `timestamp.json` last. Publish from one writer at a time. Recheck the
   base deployment immediately before promotion if another writer exists.
5. Check the published channel with a previously installed Studio and a clean
   profile. To withdraw a bad mirror, sign the previous working URLs as a new
   revision and publish using the current channel as the comparison point.
   Devices keep their installed components while the source list changes.

Run `python distribution/components/test_stage_channel.py` to exercise the
publishing step without compiling the app. This uses disposable signing keys.
The staged directory contains verified public files, and the production signing
keys stay in the project's signing system.

See the [TUF repository maintenance manual](https://github.com/theupdateframework/tuf-on-ci/blob/main/docs/REPOSITORY-MAINTENANCE.md)
for initial signer setup and rotation.
