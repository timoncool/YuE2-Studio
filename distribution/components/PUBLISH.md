# Publish a Studio component catalogue

NSIS, portable, deb, AppImage and DMG package exactly the public `root.json`
and `url.txt` from `distribution/channels/stable/bootstrap/`. Set the former to
the reviewed public TUF root when activating production. The JSON value
`null` keeps the channel inactive in builds prepared before that activation.
The files are installed in `component-channel/` beside the Windows or standalone
executable, under `Contents/Resources` on macOS, and under
`usr/lib/YuE2 Studio` in Linux packages. The URL file contains
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
   deployment. Pass `--engines` with the reviewed engine catalogue if the
   signing repository publishes `music-engine-catalog.json`. Each verified
   target must match its reviewed file byte for byte.
4. Promote the staged directory in one versioned deployment. Staging retains
   the previous publication's numbered roots, numbered roles and hash-prefixed
   catalogues for older installers and downloads already in progress. During
   root rotation, provide each intermediate root missing from the previous
   publication in the signing metadata directory. Staging follows the
   verified chain from root version 1 to the new root. Supply that complete
   chain for an initial deployment if the installer already embeds a later root.
   For a host with individually uploaded files, upload immutable files first,
   verify they are reachable, and replace `timestamp.json` last. Publish from
   one writer at a time and recheck the base deployment before promotion.
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
