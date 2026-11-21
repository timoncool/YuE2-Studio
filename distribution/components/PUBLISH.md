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
2. Sign the catalogue with the project's TUF signer. For the initial Windows
   signing setup and subsequent catalogue edits, use the local procedure
   below. The reviewed public root is the input to staging.
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

   On initial deployment omit `--previous-site`. Pass `--voices config/rvc-voice-catalog.json`
   for the signed target `rvc-voice-catalog.json`. To publish engine package
   definitions, pass `--engines candidate/music-engine-catalog.json` and sign
   that file under the exact target name `music-engine-catalog.json`. Its
   package `files` entries give the HTTPS URL, byte count and SHA-256 for each
   executable, library or model included in the package. Upload those assets
   before signing the catalogue. Before promotion, staging fetches each new or
   changed package URL once per distinct byte count and SHA-256, checking the
   full response. Engine and model entries can share identical artifacts. Pins
   carried forward from the preceding published catalogue need no new transfer.
   A missing or corrupt new artifact aborts staging before the site is promoted.
   Each staged catalogue must match the reviewed
   source byte for byte, including JSON whitespace.

   For a mirror-only release, keep all already published catalogue declarations
   in the new signed targets metadata and pass their existing reviewed files
   again. Reuse the identical catalogue bytes and hash-prefixed paths so
   clients can continue reading them. Before the first engine catalogue is
   published, omit its signed target and the `--engines` argument. Studio
   starts from its packaged engine profiles, and the engine catalogue list
   reads a previously verified local cache or returns an empty extension list.
   Manual mode refreshes the catalogue on request. Notify and Auto also refresh it
   on startup, after a mode change and daily while Studio is running. Only Auto
   installs updates in the background, and only for packages already installed.
   Restoring a previous generation pauses automatic replacement of that package.

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

## Local Windows signer

Use the dedicated release Windows account and its private local profile. Install
the pinned dependencies from step 3 before invoking the signer. The provision
command generates independent Ed25519 keys for root, targets, snapshot and
timestamp, each with threshold 1. The same Windows account is the trust
boundary for these four roles. It protects the encrypted store through Windows
CurrentUser DPAPI. Keep the store outside the checkout, staging directories,
shared folders and cloud sync. Tauri update signing uses its own keys.

Run provisioning once for a new channel. An existing store path causes
provisioning and recovery to fail. Back up the keys before embedding the root in
any installer or making the first publication.

```powershell
$store = Join-Path $env:LOCALAPPDATA 'YuE2-Studio-TUF'
$sign = 'distribution/components/sign_channel.py'
python $sign --store $store provision
python $sign --store $store backup --output E:/offline-backup/studio-tuf.backup
```

The backup is portable between Windows accounts and machines. It uses
scrypt and authenticated AES-256-GCM. Supply a strong, randomly generated
passphrase at the concealed prompt, and keep it separately from the offline
archive. Retain a separate reviewed copy of the public `$store/root.json` and
record its SHA-256. Pass only that public root to staging, and install it as the
channel bootstrap after review. The store and backup are private release data.

Sign the first candidate with `initial`. For subsequent updates, fetch the
current deployed site again and use it as `--previous-site`. The signer verifies
that site's signatures and signed snapshot chain against the store's root, then
increments the three online role versions. Run staging against the same site.
Concurrent publishers or stale copies of the site can cause conflicting
metadata, so recheck the deployed timestamp before promotion as described in
step 4. Use a new output directory for each candidate.

```powershell
python $sign --store $store initial --mirrors distribution/channels/stable/mirrors.json --output signed-initial
python $sign --store $store update --mirrors distribution/channels/stable/mirrors.json --previous-site published --output signed-update
```

Pass `--voices` and `--engines` for applicable reviewed catalogues, including
each previously published target. Point staging at the resulting
`signed-*/metadata` and `signed-*/targets` directories. The signer uses
7-day timestamps, 30-day snapshots, 90-day targets and a one-year root.
Renew the timestamp with `update` before expiry even if no catalogue changed.
The signer handles the initial root and updates under that root. Plan root
renewal and rotation before root expiry through the separate TUF ceremony.

On a replacement release machine, run `restore` into a new private LocalAppData
path with the offline backup and reviewed public root:

```powershell
python $sign --store $store restore --backup E:/offline-backup/studio-tuf.backup --root reviewed-root.json
```

Restore validates the recovered keys against the reviewed root and seals them
with the new Windows profile. Use the current deployed site for the next update.
Run `python distribution/components/test_sign_channel.py` on Windows to exercise
provisioning, staging versions 1 and 2, and staging version 3 from the recovered
store with the same public root.

## Scheduled timestamp renewal

The stable GitHub Pages site builds from `main:/docs`. Once the initial channel
is live, run `renew_channel.py` under the Windows account that owns the signing
store. The account needs noninteractive Git access with permission to update
`main`. Start with the command below without `--publish` as a read-only probe.
Use the reviewed SHA-256 and the same pinned Python environment as for signing.

```powershell
& $python $renew --store $store --work-dir $publicWork --root-sha256 $reviewedRootHash
```

The command compares the timestamp served by Pages with the signed version on
`main`. With more than five days remaining it exits without signing or
requesting an update. Otherwise it renews the existing catalogue and runs
staging. With `--publish`, it pushes only the three changed metadata files.
The push requires the exact `main` revision used for staging. If another
publisher advances `main`, the task retries from the new revision. If Pages
lags or fails to build, check the deployment status before retrying.

After approving direct publication, register a daily task and an owner-logon
trigger. Set `$python` and `$renew` to absolute paths in a persistent publisher
checkout and venv. Set `$store` to the working DPAPI store and `$log` to a
private profile path. Use a short, existing `$publicWork` directory outside
the private store. Keep `$reviewedRootHash` from the release review.

```powershell
$task = 'YuE2 Studio stable component renewal'
if (Get-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue) {
    throw "Task already registered: $task"
}
$owner = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$arguments = '"{0}" --store "{1}" --work-dir "{2}" --root-sha256 {3} --log-file "{4}" --publish' -f $renew, $store, $publicWork, $reviewedRootHash, $log
$action = New-ScheduledTaskAction -Execute $python -Argument $arguments -WorkingDirectory $publicWork
$triggers = @((New-ScheduledTaskTrigger -Daily -At 09:00), (New-ScheduledTaskTrigger -AtLogOn -User $owner))
$principal = New-ScheduledTaskPrincipal -UserId $owner -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Minutes 15) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 30)
Register-ScheduledTask -TaskName $task -Action $action -Trigger $triggers -Principal $principal -Settings $settings
```

This task runs while the owner is signed in, including after an outage when the
owner next signs in. An expired timestamp is an authenticated predecessor for
renewal. Restored machines use the same task with the recovered store path and
the same root fingerprint. Monitor the Pages timestamp independently and
investigate by three days remaining. A successful Git submission is followed
by the Pages build. Check the public timestamp after the build completes.

See the [TUF repository maintenance manual](https://github.com/theupdateframework/tuf-on-ci/blob/main/docs/REPOSITORY-MAINTENANCE.md)
for renewal and rotation.
