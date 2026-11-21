//! Resumable downloads for the optional native extras.
//!
//! Every optional capability - the writing assistant, the karaoke aligner -
//! fetches the same way: a pinned URL, a `.part` file that survives an
//! interrupted run, and a zip that unpacks into its own directory so two
//! runtimes never mix. Nothing here starts on its own; a download happens only
//! when the user asks for it.
//!
//! How much a file weighs is asked of the server serving it, never assumed -
//! see `sizes` for what assuming it cost.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// What a downloadable asset is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Model weights.
    Model,
    /// Executables and their libraries, delivered as a zip.
    Runtime,
}

#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: AssetKind,
    pub url: &'static str,
    /// Where the file lands, relative to the capability's root.
    pub relative_path: &'static str,
    /// About what it weighs, for showing a size before anything is fetched.
    /// The real one comes from the server; see `sizes`.
    pub bytes: u64,
    /// Unpacked into this sub-directory of `runtime/` when set.
    pub unzip_into: Option<&'static str>,
    /// A file name prefix that proves this asset is unpacked. Without it a
    /// companion archive would look installed as soon as its neighbour was,
    /// because both land in the same directory.
    pub marker: &'static str,
    /// When set, only these files are taken out of the archive, by name, over
    /// range requests - the rest is never downloaded. NVIDIA's libraries come
    /// in archives several times larger than the parts anyone uses.
    pub pick: &'static [&'static str],
    /// Roughly how much VRAM the asset wants; informational only.
    pub vram_gb: Option<u32>,
    pub note: &'static str,
}

impl Asset {
    /// A runtime that is a Windows build, by what it is downloaded from and
    /// what marks it installed.
    pub fn is_windows_runtime(&self) -> bool {
        let windows_file = |name: &str| name.ends_with(".exe") || name.ends_with(".dll");
        self.kind == AssetKind::Runtime && (self.url.to_ascii_lowercase().contains("windows") || self.url.contains("-win-") || windows_file(self.marker) || self.pick.iter().any(|name| windows_file(name)))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: AssetKind,
    pub bytes: u64,
    pub vram_gb: Option<u32>,
    pub note: &'static str,
    pub installed: bool,
    /// Bytes already on disk, so a resumed download reports honestly.
    pub downloaded_bytes: u64,
    /// The program (yue2, dub...) it was taken from in this run instead of being downloaded.
    pub taken_from: Option<String>,
}

/// What a whole set calls itself while it downloads. A capability is usually
/// several files - a runtime, a provider, the weights - and they arrive as one
/// thing, so they report as one thing.
pub const SET: &str = "set";

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub asset_id: String,
    /// Which panel asked for this. The karaoke recogniser and the separator
    /// share one downloader - they share the ONNX runtime and the CUDA
    /// libraries on disk, so they must - and without this the separator's panel
    /// showed the recogniser's download as its own: its button disappeared and
    /// a percentage of somebody else's gigabytes appeared in its place.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub done: bool,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_archive: Option<ManualArchive>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualArchive {
    url: &'static str,
    path: PathBuf,
}

/// Owns one capability's download directory.
#[derive(Clone)]
pub struct Downloader {
    root: PathBuf,
    /// Where runtimes unpack, one folder per flavour: `root/runtime` unless
    /// several capabilities share one build of a tool.
    runtime_root: PathBuf,
    http: reqwest::Client,
    progress: Arc<Mutex<Option<DownloadProgress>>>,
    rejected: Arc<Mutex<std::collections::HashMap<String, DownloadProgress>>>,
    /// Raised to stop whatever is downloading. What has already arrived stays
    /// on disk with its manifest, so pressing this is a pause as much as a
    /// cancel: the next attempt carries on from the last finished piece.
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl Downloader {
    pub fn new(root: PathBuf) -> Self {
        let runtime_root = root.join("runtime");
        Self::with_runtimes(root, runtime_root)
    }

    /// A downloader whose runtimes unpack under `runtime_root`, where another
    /// capability's downloader finds the same build.
    pub fn with_runtimes(root: PathBuf, runtime_root: PathBuf) -> Self {
        Self {
            root,
            runtime_root,
            http: crate::net::download_client(),
            progress: Arc::new(Mutex::new(None)),
            rejected: Default::default(),
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_http(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_of(&self, asset: &Asset) -> PathBuf {
        self.root.join(asset.relative_path)
    }

    pub fn runtime_dir(&self, flavour: &str) -> PathBuf {
        self.runtime_root.join(flavour)
    }

    /// Where an asset's picked files land. Without a flavour they land in the
    /// root itself, which is how the engine's CUDA libraries end up beside
    /// `yue-server.exe` - the only place Windows looks without being told.
    pub fn picked_into(&self, asset: &Asset) -> PathBuf {
        match asset.unzip_into {
            Some(flavour) => self.runtime_dir(flavour),
            None => self.root.clone(),
        }
    }

    /// Installed when its files are there and, for a runtime, its record names
    /// today's pin (`install_record`), or its files are today's pin byte for byte
    /// whichever address put them there; a runtime from before records counts as
    /// in `take_over`. A model counts by its file: its address can move to
    /// another revision of the same weights without them being fetched again.
    pub fn is_installed(&self, asset: &Asset) -> bool {
        if !self.present(asset) {
            return false;
        }
        if asset.kind == AssetKind::Model {
            return true;
        }
        let record = self.record_of(asset);
        match crate::install_record::matches(&record, asset.url, &self.root) {
            Some(true) => true,
            Some(false) => self.holds_pinned_bytes(asset, &record),
            None => self.take_over(asset, &record) || self.holds_pinned_bytes(asset, &record),
        }
    }

    /// Checks an installation record with the pinned checksum, without adopting files by size.
    pub(crate) fn is_verified_install(&self, asset: &Asset) -> bool {
        let Some(expected) = crate::pin_sha::sha256(asset.url) else { return false };
        let record = self.record_of(asset);
        self.present(asset)
            && crate::install_record::read(&record).is_some_and(|record| record.sha256 == expected)
            && crate::install_record::matches(&record, asset.url, &self.root) == Some(true)
    }

    /// Today's pin already on disk byte for byte, from whichever address: the files a pick takes out of its
    /// package by the hashes the package lists, or a single file by the pin's own. Its record is then written for
    /// today's pin, so the next look reads the record.
    fn holds_pinned_bytes(&self, asset: &Asset, record: &Path) -> bool {
        if crate::install_record::under_way(record) {
            return false;
        }
        let libraries = crate::pin_sha::runtime_libraries(asset.url);
        let mut file_sha256 = "";
        let files: Vec<PathBuf> = if !libraries.is_empty() {
            let destination = self.picked_into(asset);
            let mut files = Vec::new();
            for &(name, sha) in libraries {
                let path = destination.join(name);
                if !crate::pin_sha::holds(&path, sha) { return false; }
                files.push(path);
            }
            files
        } else if !asset.pick.is_empty() {
            let destination = self.picked_into(asset);
            let mut files = Vec::new();
            for entry in asset.pick {
                let name = picked_file_name(entry);
                let path = destination.join(name);
                match crate::pin_sha::picked(asset.url, name) {
                    Some(sha256) if crate::pin_sha::holds(&path, sha256) => files.push(path),
                    _ => return false,
                }
            }
            files
        } else if asset.unzip_into.is_none() {
            let path = self.path_of(asset);
            match crate::pin_sha::sha256(asset.url) {
                Some(sha256) if crate::pin_sha::holds(&path, sha256) => {
                    file_sha256 = sha256;
                    vec![path]
                }
                _ => return false,
            }
        } else {
            return false;
        };
        if let Err(error) = crate::install_record::write(record, asset.url, file_sha256, &files, &self.root) {
            eprintln!("[ERROR] {}: the record of today's files already on disk was not written: {error:#}", asset.label);
        }
        true
    }

    /// What an earlier release left of `asset` for today's pin to replace: `Some(Some(url))` when its record names
    /// the URL it came from, `Some(None)` when its files lie there without a record of it; `None` when it is today's
    /// or not there. A model counts by its file, so it has no earlier release.
    pub fn earlier(&self, asset: &Asset) -> Option<Option<String>> {
        if asset.kind == AssetKind::Model || self.is_installed(asset) {
            return None;
        }
        if let Some(record) = crate::install_record::read(&self.record_of(asset)) {
            if record.url != asset.url {
                return Some(Some(record.url));
            }
        }
        self.present(asset).then_some(None)
    }

    /// Where the record of an asset's download lies: beside its file or archive.
    pub fn record_of(&self, asset: &Asset) -> PathBuf {
        crate::install_record::record_path(&self.path_of(asset))
    }

    /// A download from before records existed. A model counts by its presence;
    /// a runtime only when the file its pin names has the pinned size - a single
    /// file its own, an archive its key file - and its record is then written.
    fn take_over(&self, asset: &Asset, record: &Path) -> bool {
        if asset.kind == AssetKind::Model {
            return true;
        }
        if !crate::pin_sha::runtime_libraries(asset.url).is_empty() { return false; }
        // A pin for a complete file can prove the actual bytes. Let
        // holds_pinned_bytes do that before adopting it from an older install.
        if asset.pick.is_empty() && asset.unzip_into.is_none() && crate::pin_sha::sha256(asset.url).is_some() {
            return false;
        }
        let key = if asset.pick.is_empty() && asset.unzip_into.is_none() {
            Some((self.path_of(asset), asset.bytes))
        } else {
            crate::install_record::key_file(asset.url).map(|(file, size)| (self.picked_into(asset).join(file), size))
        };
        let Some((path, _)) = key.filter(|(path, size)| fs::metadata(path).is_ok_and(|meta| meta.len() == *size)) else {
            return false;
        };
        if let Err(error) = crate::install_record::write(record, asset.url, "", &[path], &self.root) {
            eprintln!("[ERROR] {}: the record of the download already on disk was not written: {error:#}", asset.label);
        }
        true
    }

    /// The files of the asset are on disk, whichever pin put them there.
    fn present(&self, asset: &Asset) -> bool {
        let libraries = crate::pin_sha::runtime_libraries(asset.url);
        if !libraries.is_empty() {
            let destination = self.picked_into(asset);
            return libraries.iter().all(|(name, _)| destination.join(name).is_file());
        }
        if !asset.pick.is_empty() {
            let destination = self.picked_into(asset);
            return asset.pick.iter().all(|name| destination.join(picked_file_name(name)).is_file());
        }
        if let Some(flavour) = asset.unzip_into {
            let marker = asset.marker.to_ascii_lowercase();
            return fs::read_dir(self.runtime_dir(flavour))
                .map(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with(&marker))
                })
                .unwrap_or(false);
        }
        // Presence, not size. A file only appears here after being downloaded
        // into `.part` and checked against the length the server itself
        // declared, so it being here is the proof that it arrived whole. Judging
        // it by a figure compiled into the studio meant that re-uploading a file
        // on someone else's repository - a routine thing, and none of our
        // business - made it permanently un-installable.
        fs::metadata(self.path_of(asset)).map(|meta| meta.len() > 0).unwrap_or(false)
    }

    pub fn partial_bytes(&self, asset: &Asset) -> u64 {
        let part = if asset.pick.is_empty() {
            self.path_of(asset).with_extension("part")
        } else {
            picked_archive_partial(&self.picked_into(asset), asset)
        };
        crate::chunked::on_disk(&part)
    }

    pub fn status_of(&self, assets: &[Asset]) -> Vec<AssetStatus> {
        // Ask the servers what these actually weigh. The answers land in time
        // for the next poll a second from now; nothing waits on them.
        crate::sizes::ask(&self.http, assets.iter().map(|asset| asset.url.to_string()).collect());
        self.link_from_neighbors(
            assets.iter().copied().filter(|asset| asset.kind == AssetKind::Model && asset.pick.is_empty() && asset.unzip_into.is_none() && !self.is_installed(asset)).collect(),
        );
        assets
            .iter()
            .map(|asset| AssetStatus {
                id: asset.id,
                label: asset.label,
                kind: asset.kind,
                bytes: crate::sizes::or_listed(asset.url, asset.bytes),
                vram_gb: asset.vram_gb,
                note: asset.note,
                installed: self.is_installed(asset),
                downloaded_bytes: if self.is_installed(asset) { crate::sizes::or_listed(asset.url, asset.bytes) } else { self.partial_bytes(asset) },
                taken_from: taken_from(asset.id),
            })
            .collect()
    }

    /// Links in what another program already has off the async workers: the walk of its folders, and the wait for a
    /// file another caller is taking, must not hold a status poll. What is found shows from the next poll.
    fn link_from_neighbors(&self, assets: Vec<Asset>) {
        static LINKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        struct Linking;
        impl Drop for Linking {
            fn drop(&mut self) {
                LINKING.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        if assets.is_empty() || LINKING.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        let downloader = self.clone();
        let work = move || {
            let _linking = Linking;
            for asset in &assets {
                downloader.link_from_neighbor(asset);
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(work)),
            Err(_) => work(),
        }
    }

    /// A model another program of timoncool's already has (the same name and the size the server gives) is linked
    /// in without asking: a hard link costs no disk and no download. A copy waits for the person to ask (`fetch`).
    fn link_from_neighbor(&self, asset: &Asset) {
        if self.is_installed(asset) {
            return;
        }
        let target = self.path_of(asset);
        let (Some(bytes), Some(name)) = (crate::sizes::known(asset.url), target.file_name()) else { return };
        if let Some((source, app)) = studio_hub_client::neighbors::find_file(name, bytes, target.parent().and_then(Path::file_name)) {
            match studio_hub_client::neighbors::take_file(&source, &target, false, |staged| crate::pin_sha::accepts(asset.url, staged)) {
                Ok(()) => note_taken(asset.id, &target, &app),
                Err(error) => report_take_failure(&target, &app, &error),
            }
        }
    }

    /// Deletes an installed asset, whatever shape it took on disk.
    ///
    /// A single file is a file; a runtime is a directory of unpacked libraries;
    /// a picked set is the named files inside one. Anything half-downloaded goes
    /// with it - a `.part` occupies exactly as much disk as a finished file.
    pub fn remove(&self, asset: &Asset) -> Result<u64> {
        // A free function rather than a closure: the running total is read while
        // the directory walk below is still adding to it.
        fn drop_file(path: PathBuf) -> Result<u64> {
            match fs::symlink_metadata(&path) {
                Ok(meta) => {
                    // A library picked by its loader name is a link; the
                    // versioned file it names is what takes the disk.
                    let size = match fs::read_link(&path) {
                        Ok(target) if meta.file_type().is_symlink() => match path.parent() {
                            Some(directory) => drop_file(directory.join(target))?,
                            None => 0,
                        },
                        _ => meta.len(),
                    };
                    fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
                    Ok(size)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
                Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
            }
        }

        let mut freed = 0u64;

        if !asset.pick.is_empty() {
            let destination = self.runtime_root.join(asset.unzip_into.unwrap_or("."));
            for name in asset.pick {
                freed += drop_file(destination.join(picked_file_name(name)))?;
            }
        } else if let Some(flavour) = asset.unzip_into {
            let directory = self.runtime_dir(flavour);
            if let Ok(entries) = fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata() {
                        if meta.is_file() {
                            freed += meta.len();
                        }
                    }
                }
            }
            match fs::remove_dir_all(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("remove {}", directory.display())),
            }
        } else {
            freed += drop_file(self.path_of(asset))?;
        }

        let part = self.path_of(asset).with_extension("part");
        freed += drop_file(crate::chunked::manifest_path(&part))?;
        freed += drop_file(part)?;
        drop_file(self.record_of(asset))?;
        Ok(freed)
    }

    pub async fn active(&self) -> Option<DownloadProgress> {
        self.progress.lock().await.clone()
    }
    /// Stops the running download.
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The running download, but only if this panel started it.
    pub async fn active_for(&self, scope: &str) -> Option<DownloadProgress> {
        let active = self.progress.lock().await;
        if let Some(ours) = active.as_ref().filter(|progress| progress.scope.as_deref() == Some(scope)) {
            return Some(ours.clone());
        }
        self.rejected.lock().await.get(scope).cloned()
    }

    /// Clear an earlier result before accepting a new background transfer.
    pub(crate) async fn clear_finished(&self, scope: &str) {
        let mut active = self.progress.lock().await;
        if active.as_ref().is_some_and(|progress| progress.done && progress.scope.as_deref() == Some(scope)) {
            *active = None;
        }
        self.rejected.lock().await.remove(scope);
    }

    /// Stops the running download only when the panel of `scope` started it: a downloader two panels share (audio.cpp's
    /// server carries the stems and Audio to MIDI) keeps the other panel's download going.
    pub async fn cancel_for(&self, scope: &str) -> bool {
        let active = self.progress.lock().await;
        let ours = active.as_ref().is_some_and(|progress| !progress.done && progress.scope.as_deref() == Some(scope));
        if ours {
            self.cancel();
        }
        ours
    }

    /// The panel's set failed after its files arrived: the step that completes it did not, and the panel shows the
    /// reason as it shows a failed download.
    pub async fn fail(&self, scope: &str, error: String) {
        let mut progress = self.progress.lock().await;
        let mut failed = progress.clone().filter(|active| active.scope.as_deref() == Some(scope)).unwrap_or(DownloadProgress {
            asset_id: SET.to_string(),
            scope: Some(scope.to_string()),
            downloaded_bytes: 0,
            total_bytes: 0,
            done: true,
            error: None,
            manual_archive: None,
        });
        failed.done = true;
        failed.error = Some(error);
        if let Some(running) = progress.as_ref().filter(|active| !active.done) {
            // A rejected request cannot finish the transfer which still owns the downloader.
            if running.scope.as_deref() != Some(scope) {
                self.rejected.lock().await.insert(scope.to_owned(), failed);
            }
        } else {
            *progress = Some(failed);
        }
    }

    /// Installs a whole set, several files at a time.
    ///
    /// One at a time was the reason a recogniser took as long as its slowest
    /// file and the panel showed whichever download happened to be current -
    /// the progress is one figure per downloader, so a queue of eight made it
    /// jump between them. Four at once saturates the line, and the set reports
    /// as one download: how much of everything it needs has arrived.
    ///
    /// That reporting is not decoration. The panel decides there is a download
    /// running by looking at this one figure, so a set that downloaded without
    /// touching it looked like nothing had happened at all: the request
    /// succeeded, the files arrived, and the interface redrew the same row with
    /// the same download button. That is what "the button does nothing" was.
    pub async fn install_all(&self, scope: &str, assets: &[&'static Asset]) -> Result<()> {
        self.install_with_hint(scope, assets, &mut None).await
    }

    pub(crate) async fn install_with_hint(&self, scope: &str, assets: &[&'static Asset], hint: &mut Option<ManualArchive>) -> Result<()> {
        self.install_set(scope, assets, None, hint, Self::is_installed).await
    }

    pub(crate) async fn install_local_archive(&self, scope: &str, asset: &'static Asset, source: &Path) -> Result<()> {
        anyhow::ensure!(release_archive_name(asset).is_some(), "this component has no pinned runtime archive");
        self.install_set(scope, &[asset], Some(source), &mut None, Self::is_installed).await
    }

    pub(crate) async fn install_verified(&self, scope: &str, assets: &[&'static Asset]) -> Result<()> {
        anyhow::ensure!(assets.iter().all(|asset| crate::pin_sha::sha256(asset.url).is_some()),
            "verified installation requires a checksum for each component");
        self.install_set(scope, assets, None, &mut None, Self::is_verified_install).await
    }

    async fn install_set(
        &self, scope: &str, assets: &[&'static Asset], source: Option<&Path>,
        hint: &mut Option<ManualArchive>, is_installed: fn(&Self, &Asset) -> bool,
    ) -> Result<()> {
        *hint = None;
        const AT_ONCE: usize = 4;
        let pending: Vec<&'static Asset> = assets.iter().copied().filter(|asset| !is_installed(self, asset)).collect();
        if pending.is_empty() {
            return Ok(());
        }
        let total: u64 = pending.iter().map(|asset| asset.bytes).sum();
        let mut progress = self.progress.lock().await;
        // a second press before the first set finished would write the same
        // partial files twice
        if progress.as_ref().is_some_and(|active| !active.done && active.error.is_none()) {
            bail!("another download is already running");
        }
        self.rejected.lock().await.remove(scope);
        self.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        // A set of one is that one file, as far as anyone watching is
        // concerned; only a real set needs a name of its own.
        let asset_id = if pending.len() == 1 { pending[0].id.to_string() } else { SET.to_string() };
        // the panel learns the outcome of a set from here alone
        if let Err(error) = pending.iter().try_for_each(|asset| refuse_foreign_runtime(asset)) {
            *progress = Some(DownloadProgress { asset_id, scope: Some(scope.to_string()), downloaded_bytes: 0, total_bytes: total, done: true, error: Some(error.to_string()), manual_archive: None });
            return Err(error);
        }
        *progress = Some(DownloadProgress {
            asset_id,
            scope: Some(scope.to_string()),
            downloaded_bytes: pending.iter().map(|asset| self.partial_bytes(asset)).sum(),
            total_bytes: total,
            done: false,
            error: None,
            manual_archive: None,
        });
        drop(progress);

        let mut finished = 0u64;
        let mut failure = None;
        let mut manual_archive = None;
        for batch in pending.chunks(AT_ONCE) {
            // One cell per download, summed by the reporter below: four
            // downloads writing to one counter is what made the bar jump
            // between files and reset.
            let cells: Vec<Arc<Mutex<Option<DownloadProgress>>>> = batch
                .iter()
                .map(|asset| {
                    Arc::new(Mutex::new(Some(DownloadProgress {
                        asset_id: asset.id.to_string(),
                        scope: Some(scope.to_string()),
                        downloaded_bytes: self.partial_bytes(asset),
                        total_bytes: asset.bytes,
                        done: false,
                        error: None,
                        manual_archive: None,
                    })))
                })
                .collect();
            let reporter = self.publish_sum(finished, total, cells.clone());
            let outcomes =
                futures_util::future::join_all(batch.iter().zip(&cells).map(|(asset, cell)| async move {
                    match source {
                        Some(archive) => {
                            let into = (!asset.pick.is_empty() || asset.unzip_into.is_some()).then(|| self.picked_into(asset));
                            settle_apart(asset, archive.to_owned(), self.record_of(asset), self.root.clone(), into, true).await
                        }
                        None => self.fetch_now(asset, cell).await,
                    }
                })).await;
            reporter.abort();
            finished += batch.iter().map(|asset| asset.bytes).sum::<u64>();
            if let Some((asset, error)) = batch.iter().zip(outcomes).find_map(|(asset, result)| result.err().map(|error| (*asset, error))) {
                manual_archive = manual_archive_hint(asset, &self.path_of(asset), &error);
                failure = Some(error);
                break;
            }
        }

        let outcome = match failure {
            Some(error) if self.cancel.load(std::sync::atomic::Ordering::Relaxed) => {
                let _ = error;
                Err(anyhow::anyhow!("cancelled"))
            }
            Some(error) => Err(error),
            None => match assets.iter().find(|asset| !is_installed(self, asset)) {
                Some(asset) => Err(anyhow::anyhow!("{} finished downloading but its files are not on disk", asset.label)),
                None => Ok(()),
            },
        };
        *hint = manual_archive.filter(|_| !self.cancel.load(std::sync::atomic::Ordering::Relaxed));
        if let Some(active) = self.progress.lock().await.as_mut() {
            active.done = true;
            active.error = outcome.as_ref().err().map(|error| error.to_string());
            active.manual_archive = hint.clone();
            if outcome.is_ok() {
                active.downloaded_bytes = total;
            }
        }
        outcome
    }

    /// Reports what a running batch has fetched, as one figure for the set.
    ///
    /// Aborted by the caller when the batch ends, which is why it may loop for
    /// ever: it has nothing to decide, only something to add up.
    fn publish_sum(&self, finished: u64, total: u64, cells: Vec<Arc<Mutex<Option<DownloadProgress>>>>) -> tokio::task::JoinHandle<()> {
        let progress = self.progress.clone();
        tokio::spawn(async move {
            loop {
                let mut written = finished;
                for cell in &cells {
                    if let Some(active) = cell.lock().await.as_ref() {
                        written += active.downloaded_bytes;
                    }
                }
                if let Some(active) = progress.lock().await.as_mut() {
                    active.downloaded_bytes = written.min(total);
                }
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        })
    }

    /// Downloads one asset and waits for it, without the single-slot rule that
    /// `install` keeps for the panel's own buttons. Reports into the cell it is
    /// given rather than the downloader's own, because several of these run at
    /// the same time.
    async fn fetch_now(&self, asset: &'static Asset, cell: &Arc<Mutex<Option<DownloadProgress>>>) -> Result<()> {
        let target = self.path_of(asset);
        let record = self.record_of(asset);
        if !asset.pick.is_empty() {
            // The whole archive over the resumable downloader, then the wanted
            // files extracted locally. Reading named files straight out of a
            // remote zip over range requests was clever and fragile: a separate
            // client with no resume, no retry and no rate-limit handling, so a
            // slow or interrupted connection sat at nought bytes until a ten
            // minute timeout. NVIDIA's cuBLAS archive is almost all the two
            // libraries anyway - nineteen megabytes of difference bought a
            // download that survives a bad line.
            let (sha256, files) = download_and_extract_named(&self.http, asset, &target, &self.picked_into(asset), cell, self.cancel.clone(), &record, &self.root).await?;
            make_programs_executable(asset.marker, &files)?;
            return crate::install_record::write(&record, asset.url, &sha256, &files, &self.root);
        }
        let target = fetch(&self.http, asset, &target, cell, self.cancel.clone()).await?;
        settle_apart(asset, target, record, self.root.clone(), asset.unzip_into.map(|flavour| self.runtime_dir(flavour)), false).await
    }

    /// Starts one download in the background. Only one runs at a time, and an
    /// interrupted file resumes from what is already on disk.
    pub async fn install(&self, scope: &str, asset: &'static Asset) -> Result<()> {
        refuse_foreign_runtime(asset)?;
        {
            let mut progress = self.progress.lock().await;
            if progress.as_ref().is_some_and(|active| !active.done && active.error.is_none()) {
                bail!("another download is already running");
            }
            self.rejected.lock().await.remove(scope);
            // The archive is normally discarded after unpacking. A repeated
            // setup request must also work offline after moving the data folder.
            if asset.id == "onnxruntime" && self.is_installed(asset) {
                *progress = Some(DownloadProgress {
                    asset_id: asset.id.to_owned(), scope: Some(scope.to_owned()),
                    downloaded_bytes: 0, total_bytes: 0, done: true,
                    error: None, manual_archive: None,
                });
                return Ok(());
            }
            self.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
            *progress = Some(DownloadProgress {
                asset_id: asset.id.to_string(),
                scope: Some(scope.to_owned()),
                downloaded_bytes: self.partial_bytes(asset),
                total_bytes: asset.bytes,
                done: false,
                error: None,
                manual_archive: None,
            });
        }

        let target = self.path_of(asset);
        let record = self.record_of(asset);
        let root = self.root.clone();
        let runtime_root = self.runtime_root.clone();
        let http = self.http.clone();
        let progress = self.progress.clone();
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            // A named-file asset: the whole archive over the resumable
            // downloader, then the wanted files extracted locally. The old path
            // read them straight out of a remote zip with a client that could
            // not resume or retry, so a bad connection sat at nought bytes.
            if !asset.pick.is_empty() {
                let destination = match asset.unzip_into {
                    Some(flavour) => runtime_root.join(flavour),
                    None => root.clone(),
                };
                let outcome = download_and_extract_named(&http, asset, &target, &destination, &progress, cancel, &record, &root)
                    .await
                    .and_then(|(sha256, files)| {
                        make_programs_executable(asset.marker, &files)?;
                        crate::install_record::write(&record, asset.url, &sha256, &files, &root)
                    });
                let mut guard = progress.lock().await;
                if let Some(active) = guard.as_mut() {
                    active.done = true;
                    active.manual_archive = outcome.as_ref().err().and_then(|error| manual_archive_hint(asset, &target, error));
                    active.error = outcome.err().map(|error| error.to_string());
                }
                return;
            }
            let outcome = match fetch(&http, asset, &target, &progress, cancel).await {
                Ok(archive) => settle_apart(asset, archive, record, root, asset.unzip_into.map(|flavour| runtime_root.join(flavour)), false).await,
                Err(error) => Err(error),
            };
            let mut guard = progress.lock().await;
            let manual_archive = outcome.as_ref().err().and_then(|error| manual_archive_hint(asset, &target, error));
            let error = outcome.err().map(|error| error.to_string());
            if let Some(active) = guard.as_mut() {
                active.done = true;
                active.error = error;
                active.manual_archive = manual_archive;
            }
        });
        Ok(())
    }
}

async fn fetch(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<PathBuf> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if holds_todays(asset.kind == AssetKind::Model, asset.url, target).await {
        return Ok(target.to_path_buf());
    }
    if let Some(archive) = release_archive_alias(asset, target).filter(|path| path.is_file()) {
        let (url, local) = (asset.url, archive.clone());
        tokio::task::spawn_blocking(move || check_staged_archive(url, &local)).await??;
        return Ok(archive);
    }
    // an earlier pin's file of this name waits beside it, and comes back when today's does not arrive
    let earlier = PathBuf::from(format!("{}.earlier", target.display()));
    let set_aside = target.is_file();
    if set_aside {
        fs::rename(target, &earlier).with_context(|| format!("set {} aside", target.display()))?;
    }
    let mut outcome = fetch_absent(http, asset, target, progress, cancel).await;
    if set_aside && outcome.is_ok() {
        let (url, file) = (asset.url, target.to_path_buf());
        outcome = tokio::task::spawn_blocking(move || crate::pin_sha::check(url, &file).map(|_| ()))
            .await
            .unwrap_or_else(|error| Err(anyhow::anyhow!("the check of {} failed: {error}", target.display())));
    }
    if set_aside {
        if outcome.is_ok() {
            fs::remove_file(&earlier).ok();
        } else if !target.exists() {
            if let Err(error) = fs::rename(&earlier, target) {
                eprintln!("[ERROR] {} was not put back after the download failed: {error}", target.display());
            }
        }
    }
    outcome.map(|()| target.to_path_buf())
}

pub(crate) fn release_archive_name(asset: &Asset) -> Option<&'static str> {
    if asset.kind != AssetKind::Runtime || (asset.unzip_into.is_none() && asset.pick.is_empty()) || crate::pin_sha::sha256(asset.url).is_none() {
        return None;
    }
    let filename = asset.url.rsplit('/').next()?;
    if ![".zip", ".tgz", ".tar.gz", ".tar.xz", ".whl"].iter().any(|suffix| filename.ends_with(suffix)) {
        return None;
    }
    Some(filename)
}

/// Recognize an archive saved under its release filename beside the studio's download path.
fn release_archive_alias(asset: &Asset, target: &Path) -> Option<PathBuf> {
    let alias = target.with_file_name(release_archive_name(asset)?);
    (alias != target).then_some(alias)
}

fn manual_archive_hint(asset: &Asset, target: &Path, error: &anyhow::Error) -> Option<ManualArchive> {
    if !asset.url.starts_with("https://")
        || !matches!(failure_code(error), "network" | "proxy" | "http_status" | "rate_limited")
    {
        return None;
    }
    Some(ManualArchive { url: asset.url, path: target.with_file_name(release_archive_name(asset)?) })
}

fn check_staged_archive(url: &str, archive: &Path) -> Result<String> {
    let sha256 = crate::install_record::sha256_of(archive)?;
    if crate::pin_sha::sha256(url) != Some(sha256.as_str()) {
        bail!("{} does not match the SHA-256 of the archive for this version: {url}", archive.display());
    }
    Ok(sha256)
}

/// ONNX releases may be unreachable from networks that cannot connect to GitHub at all (#276).
/// Only offer these mirrors for known, hash-pinned release files. Installation still checks the original pin.
#[cfg(test)]
fn onnx_sources(url: &str) -> Option<&'static [String]> {
    let digest = crate::pin_sha::sha256(url)?;
    crate::component_catalogue::bundled().sources(digest)
}

async fn unless_cancelled<T>(cancel: &std::sync::atomic::AtomicBool, work: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {
        biased;
        _ = async {
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        } => bail!("cancelled"),
        result = work => result,
    }
}

fn try_another_source(error: &anyhow::Error) -> bool {
    matches!(failure_code(error), "network" | "http_status" | "rate_limited" | "size_mismatch" | "hash_mismatch")
        || error.chain().any(|cause| cause.downcast_ref::<reqwest::Error>().is_some_and(|error| error.is_body() || error.is_decode() || error.is_request() || error.is_timeout()))
}

/// A neighbouring studio can have the same model under the same relative path, even
/// in folders (such as midi/models) excluded from the general file search.
fn take_model_from_installations(asset: &Asset, target: &Path, own_root: &Path, neighbors: &[studio_hub_client::neighbors::Neighbor]) -> bool {
    let Ok(relative) = target.strip_prefix(own_root) else { return false };
    if !relative.components().all(|part| matches!(part, std::path::Component::Normal(_))) {
        return false;
    }
    for neighbor in neighbors {
        let source = neighbor.data_dir.join(relative);
        if !source.is_file() { continue; }
        match studio_hub_client::neighbors::take_file(&source, target, true, |staged| crate::pin_sha::accepts(asset.url, staged)) {
            Ok(()) => {
                note_taken(asset.id, target, &neighbor.app);
                return true;
            }
            Err(error) => report_take_failure(target, &neighbor.app, &error),
        }
    }
    false
}

/// `fetch` once nothing lies at `target`: from another program that has the file, or downloaded into `.part`.
async fn fetch_absent(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    if asset.kind == AssetKind::Model && asset.pick.is_empty() && asset.unzip_into.is_none() && crate::pin_sha::sha256(asset.url).is_some() {
        if let Some(own_root) = crate::studio_data_root() {
            let (asset, target) = (*asset, target.to_owned());
            let taken = tokio::task::spawn_blocking(move || {
                let neighbors = studio_hub_client::neighbors::others(&own_root);
                take_model_from_installations(&asset, &target, &own_root, &neighbors)
            }).await?;
            if taken { return Ok(()); }
        }
    }
    // How big it is and whether it can be fetched in pieces, asked of the
    // server rather than assumed - both answers come from one ranged byte.
    let url = crate::net::model_url(asset.url);
    let Some(digest) = crate::pin_sha::sha256(asset.url) else {
        return fetch_from(http, asset, target, progress, cancel, &url, false).await;
    };
    let bundled = crate::component_catalogue::bundled().sources(digest);
    // For pins outside the bundled mirror list, a healthy primary needs no channel request.
    let primary_failure = if bundled.is_none() {
        match fetch_verified_source(http, asset, target, progress, cancel.clone(), &url).await {
            Ok(()) => return Ok(()),
            Err(error) if !try_another_source(&error) => return Err(error),
            Err(error) => Some(error),
        }
    } else {
        None
    };
    let mut candidates = unless_cancelled(&cancel, async { Ok(mirror_sources(http, digest).await) }).await?;
    if let Some(mirror) = crate::engine_runtime::archive_mirror(asset.url) {
        if !candidates.iter().any(|url| url == mirror) { candidates.push(mirror.to_owned()); }
    }
    if let Some(error) = primary_failure {
        candidates.retain(|candidate| candidate != &url);
        if candidates.is_empty() { return Err(error); }
        return fetch_mirrors(http, asset, target, progress, cancel, candidates).await
            .with_context(|| format!("primary download failed: {error:#}; component mirrors also failed"));
    }
    fetch_mirrors(http, asset, target, progress, cancel, candidates).await
}
pub(crate) async fn mirror_sources(http: &reqwest::Client, digest: &str) -> Vec<String> {
    let refresh = crate::component_catalogue::channel_sources(http, digest);
    let mut sources = match tokio::time::timeout(std::time::Duration::from_secs(12), refresh).await {
        Ok(Ok(sources)) => sources.unwrap_or_default(),
        Ok(Err(error)) => { eprintln!("Component mirror refresh failed: {error}"); Vec::new() }
        Err(_) => { eprintln!("Component mirror refresh timed out"); Vec::new() }
    };
    sources.extend_from_slice(crate::component_catalogue::bundled().sources(digest).unwrap_or_default());
    let mut candidates = Vec::new();
    for source in sources {
        let mapped = crate::net::model_url(&source);
        if !candidates.contains(&mapped) { candidates.push(mapped); }
    }
    candidates
}

fn picked_archive_partial(destination: &Path, asset: &Asset) -> PathBuf {
    destination.join(format!("{}.part-archive", asset.id))
}

fn partial_for_target(asset: &Asset, target: &Path) -> PathBuf {
    if !asset.pick.is_empty() {
        if let Some(destination) = target.parent() {
            return picked_archive_partial(destination, asset);
        }
    }
    target.with_extension("part")
}

async fn fetch_mirrors(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    sources: impl AsRef<[String]>,
) -> Result<()> {
    let mut failures = Vec::new();
    for source in sources.as_ref() {
        match fetch_verified_source(http, asset, target, progress, cancel.clone(), source).await {
            Ok(()) => return Ok(()),
            Err(_) if cancel.load(std::sync::atomic::Ordering::Relaxed) => bail!("cancelled"),
            Err(error) if !try_another_source(&error) => return Err(error),
            Err(error) => failures.push(format!("{source}: {error:#}")),
        }
    }
    bail!("{}", failures.join(" | "))
}

async fn fetch_verified_source(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    source: &str,
) -> Result<()> {
    let resumed = crate::chunked::on_disk(&partial_for_target(asset, target)) > 0;
    match fetch_from(http, asset, target, progress, cancel.clone(), source, true).await {
        Err(error) if resumed && failure_code(&error) == "hash_mismatch" && !cancel.load(std::sync::atomic::Ordering::Relaxed) => {
            fetch_from(http, asset, target, progress, cancel, source, true).await
        }
        result => result,
    }
}

async fn fetch_from(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    source: &str,
    verify_before_publish: bool,
) -> Result<()> {
    let probe = crate::chunked::probe(http, source);
    let (plan, url) = unless_cancelled(&cancel, probe).await?;
    if plan.total > 0 {
        if !verify_before_publish {
            crate::sizes::learn(asset.url, plan.total);
        }
        // another program of timoncool's has it: taken from there, by copy too when it is on another drive
        let (to, total, pinned) = (target.to_path_buf(), plan.total, asset.url);
        let taken = tokio::task::spawn_blocking(move || {
            let (source, app) = to.file_name().and_then(|name| studio_hub_client::neighbors::find_file(name, total, to.parent().and_then(Path::file_name)))?;
            Some((studio_hub_client::neighbors::take_file(&source, &to, true, |staged| crate::pin_sha::accepts(pinned, staged)), app))
        })
        .await
        .ok()
        .flatten();
        match taken {
            Some((Ok(()), app)) => {
                note_taken(asset.id, target, &app);
                return Ok(());
            }
            Some((Err(error), app)) => report_take_failure(target, &app, &error),
            None => {}
        }
    }

    let part = partial_for_target(asset, target);
    let written = Arc::new(std::sync::atomic::AtomicU64::new(0));
    // The panel reads one figure; the pieces write to another, from four tasks
    // at once. This copies the second into the first while they work.
    let reporter = {
        let (written, progress) = (written.clone(), progress.clone());
        tokio::spawn(async move {
            loop {
                let value = written.load(std::sync::atomic::Ordering::Relaxed);
                if let Some(active) = progress.lock().await.as_mut() {
                    active.downloaded_bytes = value;
                }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        })
    };
    // A broken range stops its siblings without marking the whole download as cancelled by the user.
    let attempt_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let outcome = unless_cancelled(&cancel, crate::chunked::fetch(http, &url, &part, plan, written.clone(), attempt_stop)).await;
    reporter.abort();
    outcome?;
    if let Some(active) = progress.lock().await.as_mut() {
        active.downloaded_bytes = written.load(std::sync::atomic::Ordering::Relaxed);
    }

    let size = fs::metadata(&part)?.len();
    if plan.total > 0 && size != plan.total {
        bail!("{} has {size} bytes, the server said {}", asset.label, plan.total);
    }
    if verify_before_publish {
        let (original, staged) = (asset.url, part.clone());
        tokio::task::spawn_blocking(move || crate::pin_sha::check(original, &staged)).await??;
        crate::sizes::learn(asset.url, size);
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("cancelled");
        }
    }
    fs::rename(&part, target).with_context(|| format!("publish {}", target.display()))?;
    Ok(())
}

/// Says once per file why a neighbour's copy could not be taken: the setup screen asks every second.
pub(crate) fn report_take_failure(target: &Path, app: &str, error: &io::Error) {
    // a link does not cross drives; installing takes such a file by copy
    if error.kind() == io::ErrorKind::CrossesDevices {
        return;
    }
    static SAID: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !said.iter().any(|known| known == target) {
        said.push(target.to_path_buf());
        eprintln!("[ERROR] {} was not taken from {app}: {error}", target.display());
    }
}

/// A model taken from another program instead of being downloaded: which program, and the file's name.
#[derive(Clone, Debug, Serialize)]
pub struct Taken {
    pub app: String,
    pub file: String,
}

/// What was taken from another program in this run, by asset or component id: the panels say "taken from ...".
fn taken() -> &'static std::sync::Mutex<std::collections::BTreeMap<String, Taken>> {
    static TAKEN: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<String, Taken>>> = std::sync::OnceLock::new();
    TAKEN.get_or_init(Default::default)
}

pub(crate) fn note_taken(id: &str, file: &Path, app: &str) {
    eprintln!("[OK] {id} taken from {app} instead of downloading");
    let file = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    taken().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id.to_string(), Taken { app: app.to_string(), file });
}

pub(crate) fn taken_from(id: &str) -> Option<String> {
    taken().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(id).map(|taken| taken.app.clone())
}

/// Everything taken from another program in this run, for the setup screen.
pub(crate) fn taken_all() -> std::collections::BTreeMap<String, Taken> {
    taken().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// Downloads a whole archive with the resumable downloader and extracts the
/// asset's named files, reporting progress into the shared cell. What an earlier
/// pin recorded at `record` goes only once the archive is on disk and checked.
async fn download_and_extract_named(
    http: &reqwest::Client,
    asset: &'static Asset,
    target: &Path,
    destination: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    record: &Path,
    root: &Path,
) -> Result<(String, Vec<PathBuf>)> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let alias = release_archive_alias(asset, target).filter(|path| path.is_file());
    let target = if target.is_file() { target } else { alias.as_deref().unwrap_or(target) };
    let staged = target.is_file() && crate::pin_sha::sha256(asset.url).is_some();
    let archive = if staged {
        target.to_path_buf()
    } else {
        let archive = destination.join(format!("{}.verified-archive", asset.id));
        fetch_absent(http, asset, &archive, progress, cancel).await?;
        archive
    };
    let (archive_path, wanted, destination, pinned) = (archive.clone(), asset.pick, destination.to_path_buf(), asset.url);
    let (record, root) = (record.to_path_buf(), root.to_path_buf());
    let extracted = tokio::task::spawn_blocking(move || -> Result<(String, Vec<PathBuf>)> {
        let sha256 = if staged {
            check_staged_archive(pinned, &archive_path)?
        } else {
            crate::pin_sha::check(pinned, &archive_path)?
        };
        replace_earlier(&record, &root, &archive_path)?;
        Ok((sha256, extract_named_local(&archive_path, wanted, &destination)?))
    })
    .await
    .map_err(|error| anyhow::anyhow!("extraction task failed: {error}"))?;
    if !staged {
        fs::remove_file(&archive).ok();
    }
    extracted
}

/// Today's download of an asset is on disk and checked: its install is under way from here
/// (`install_record::begin`), and what an earlier pin put down goes - the files its record lists but `kept`, the
/// file the download itself replaced, then that record. Until this point a download cut off leaves the earlier
/// pin's files where they were.
pub(crate) fn replace_earlier(record: &Path, root: &Path, kept: &Path) -> Result<()> {
    crate::install_record::begin(record)?;
    crate::install_record::remove_earlier(record, root, kept)
}

/// What follows the download of one file, off the async workers since it hashes the whole file: checked against its
/// pin, the earlier pin's files gone, unpacked into `unpack_into` when it is an archive, made executable and recorded.
async fn settle_apart(asset: &'static Asset, target: PathBuf, record: PathBuf, root: PathBuf, unpack_into: Option<PathBuf>, keep_archive: bool) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        if let Some(result) = unpack_into.as_ref().and_then(|folder| {
            crate::native_music_runtime::install_tree(asset.url, &target, folder, &record, &root)
        }) {
            result?;
            if !keep_archive { fs::remove_file(&target).ok(); }
            return Ok(());
        }
        let sha256 = crate::pin_sha::check(asset.url, &target)?;
        replace_earlier(&record, &root, &target)?;
        let files = match unpack_into {
            Some(folder) if !asset.pick.is_empty() => {
                fs::create_dir_all(&folder)?;
                extract_named_local(&target, asset.pick, &folder)?
            }
            Some(folder) if keep_archive => unpack_archive(&target, &folder)?,
            Some(folder) => extract_archive(&target, &folder)?,
            None => vec![target.clone()],
        };
        make_programs_executable(asset.marker, &files)?;
        crate::install_record::write(&record, asset.url, &sha256, &files, &root)
    })
    .await
    .map_err(|error| anyhow::anyhow!("the download's check stopped: {error}"))?
}

/// Whether the file a download of `url` lands on already holds today's pin: a model by its presence, as models
/// count; a runtime only by the pinned hash, since an earlier pin's file of the same name stays until today's has
/// arrived.
pub(crate) async fn holds_todays(model: bool, url: &str, target: &Path) -> bool {
    if !fs::metadata(target).is_ok_and(|meta| meta.len() > 0) {
        return false;
    }
    if model {
        return true;
    }
    let Some(pinned) = crate::pin_sha::sha256(url) else {
        return false;
    };
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || crate::install_record::sha256_of(&target).is_ok_and(|got| got == pinned)).await.unwrap_or(false)
}

fn extract_named_local(archive: &Path, wanted: &[&str], destination: &Path) -> Result<Vec<PathBuf>> {
    let format = archive_format(archive)?;
    let mut found: Vec<&str> = Vec::new();
    let mut written: Vec<PathBuf> = Vec::new();
    if format == ArchiveFormat::Zip {
        let file = fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
        let mut zip = zip::ZipArchive::new(file).with_context(|| format!("read {}", archive.display()))?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).context("read zip entry")?;
            let Some(path) = entry.enclosed_name().map(|path| path.to_string_lossy().replace('\\', "/")) else {
                continue;
            };
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let Some(candidate) = wanted.iter().copied().find(|candidate| {
                if candidate.contains('/') { candidate.eq_ignore_ascii_case(&path) } else { candidate.eq_ignore_ascii_case(&name) }
            }) else {
                continue;
            };
            let mode = entry.unix_mode();
            write_entry(&mut entry, &destination.join(&name), mode)?;
            written.push(destination.join(&name));
            found.push(candidate);
        }
    } else {
        // A library is asked for by the name the loader looks up,
        // `libcublas.so.12`; the archive carries it as the versioned file that
        // name links to, which lands under its own name with the link beside it.
        let mut links: Vec<(&str, String)> = Vec::new();
        for entry in open_tar(archive, format)?.entries().with_context(|| format!("read {}", archive.display()))? {
            let mut entry = entry.with_context(|| format!("read {}", archive.display()))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path().context("read tar entry")?.to_string_lossy().replace('\\', "/");
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let Some(candidate) = wanted.iter().copied().find(|candidate| name == *candidate || name.starts_with(&format!("{candidate}."))) else {
                continue;
            };
            if found.contains(&candidate) {
                continue;
            }
            let mode = entry.header().mode().ok();
            write_entry(&mut entry, &destination.join(&name), mode)?;
            written.push(destination.join(&name));
            if name != candidate {
                links.push((candidate, name));
            }
            found.push(candidate);
        }
        for (candidate, name) in links {
            link_beside(&destination.join(candidate), &name)?;
            written.push(destination.join(candidate));
        }
    }
    let missing: Vec<&str> = wanted.iter().copied().filter(|candidate| !found.contains(candidate)).collect();
    if !missing.is_empty() {
        bail!("{} did not contain {}", archive.display(), missing.join(", "));
    }
    Ok(written)
}

/// The file a picked archive entry lands as: its base name.
pub fn picked_file_name(entry: &str) -> &str {
    entry.rsplit('/').next().unwrap_or(entry)
}

/// The programs among unpacked `files`: the ones `marker` names, without an extension, as Linux and macOS name
/// programs (a library or a data file has one).
#[cfg(any(unix, test))]
fn programs_among<'a>(marker: &str, files: &'a [PathBuf]) -> Vec<&'a PathBuf> {
    if marker.is_empty() {
        return Vec::new();
    }
    files
        .iter()
        .filter(|path| path.extension().is_none() && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(marker)))
        .collect()
}

/// Makes the programs of a component executable: an archive packed on Windows carries no execute bit, and the
/// program would answer «Permission denied».
pub fn make_programs_executable(marker: &str, files: &[PathBuf]) -> Result<()> {
    #[cfg(unix)]
    for path in programs_among(marker, files) {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path).with_context(|| format!("read the permissions of {}", path.display()))?.permissions().mode();
        if mode & 0o111 != 0o111 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o755)).with_context(|| format!("make {} executable", path.display()))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (marker, files);
    Ok(())
}

/// Unpacks a release archive, zip or tar. Entries are flattened into
/// `destination` because the releases nest their binaries one or two
/// directories deep.
pub fn extract_archive(archive: &Path, destination: &Path) -> Result<Vec<PathBuf>> {
    let files = unpack_archive(archive, destination)?;
    fs::remove_file(archive).ok();
    Ok(files)
}

/// Extract directly from a user-owned archive, which stays in its library.
pub(crate) fn unpack_archive(archive: &Path, destination: &Path) -> Result<Vec<PathBuf>> {
    // Packages ship every architecture they support; only this one belongs
    // here, and flattening the rest would overwrite it with an ARM build.
    const FOREIGN: [&str; 4] = ["arm64", "win-x86", if cfg!(windows) { "linux" } else { "win-x64" }, "osx"];
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let mut written: Vec<PathBuf> = Vec::new();
    match archive_format(archive)? {
        ArchiveFormat::Zip => {
            let file = fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
            let mut zip = zip::ZipArchive::new(file).with_context(|| format!("read {}", archive.display()))?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index).context("read zip entry")?;
                if entry.is_dir() {
                    continue;
                }
                let Some(path) = entry.enclosed_name() else { continue };
                let inside = path.to_string_lossy().to_ascii_lowercase();
                if FOREIGN.iter().any(|other| inside.contains(other)) {
                    continue;
                }
                let Some(name) = path.file_name().map(|name| name.to_owned()) else {
                    continue;
                };
                let mode = entry.unix_mode();
                write_entry(&mut entry, &destination.join(&name), mode)?;
                written.push(destination.join(name));
            }
        }
        format => {
            // A tar carries the libraries and programs among headers and build
            // files; the loader's short names are links to the versioned file.
            let mut links: Vec<(String, String)> = Vec::new();
            for entry in open_tar(archive, format)?.entries().with_context(|| format!("read {}", archive.display()))? {
                let mut entry = entry.with_context(|| format!("read {}", archive.display()))?;
                let path = entry.path().context("read tar entry")?.to_string_lossy().replace('\\', "/");
                // a macOS package's debug symbols carry the library's own file name
                if path.contains(".dSYM/") {
                    continue;
                }
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                let kind = entry.header().entry_type();
                let mode = entry.header().mode().ok();
                // a program packed on Windows has lost its execute bit, so a name without an extension counts too
                let executable = mode.is_some_and(|mode| mode & 0o111 != 0) || Path::new(&name).extension().is_none();
                if kind.is_symlink() && is_shared_library(&name) {
                    let target = entry.link_name().ok().flatten().map(|target| target.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                    links.push((name, target.rsplit('/').next().unwrap_or(&target).to_string()));
                } else if kind.is_file() && (is_shared_library(&name) || executable) {
                    write_entry(&mut entry, &destination.join(&name), mode)?;
                    written.push(destination.join(&name));
                }
            }
            // A link may name another link, in whatever order the archive lists them.
            while !links.is_empty() {
                let (ready, waiting): (Vec<_>, Vec<_>) = links.into_iter().partition(|(_, target)| destination.join(target).is_file());
                if ready.is_empty() {
                    bail!("{} links {} to files it does not contain", archive.display(), waiting.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", "));
                }
                for (name, target) in ready {
                    link_beside(&destination.join(&name), &target)?;
                    written.push(destination.join(&name));
                }
                links = waiting;
            }
            // a library that comes by its full version alone (BSRoformer.cpp's processor and macOS builds) gets the
            // name the loader asks for beside it
            let unpacked: Vec<String> = written.iter().filter_map(|path| path.file_name()?.to_str().map(str::to_owned)).collect();
            for name in unpacked {
                let Some(short) = soname_of(&name) else { continue };
                let link = destination.join(&short);
                if !link.exists() {
                    link_beside(&link, &name)?;
                    written.push(link);
                }
            }
        }
    }
    written.sort();
    written.dedup();
    Ok(written)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFormat {
    Zip,
    TarGz,
    TarXz,
}

/// An archive's format by its first bytes: the file on disk is named after the
/// download, not after what it is.
fn archive_format(archive: &Path) -> Result<ArchiveFormat> {
    let mut head = [0u8; 6];
    let read = io::Read::read(&mut fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?, &mut head)?;
    Ok(match &head[..read] {
        [0x1f, 0x8b, ..] => ArchiveFormat::TarGz,
        [0xfd, b'7', b'z', b'X', b'Z', 0x00] => ArchiveFormat::TarXz,
        _ => ArchiveFormat::Zip,
    })
}

fn open_tar(archive: &Path, format: ArchiveFormat) -> Result<tar::Archive<Box<dyn io::Read>>> {
    let file = io::BufReader::new(fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?);
    let reader: Box<dyn io::Read> = match format {
        ArchiveFormat::TarGz => Box::new(flate2::read::GzDecoder::new(file)),
        _ => Box::new(lzma_rust2::XzReader::new(file, true)),
    };
    Ok(tar::Archive::new(reader))
}

/// `libcublas.so.12`, `libcublas.so.12.9.2.10`, `libonnxruntime.so`.
fn is_shared_library(name: &str) -> bool {
    name.ends_with(".so") || name.contains(".so.") || name.ends_with(".dylib")
}

/// The name the loader asks for of a library named by its full version: `libX.so.0.15.1` → `libX.so.0`,
/// `libX.0.15.1.dylib` → `libX.0.dylib`; None for a name without one (as Speech Studio's setup gives it).
fn soname_of(name: &str) -> Option<String> {
    let numeric = |parts: &[&str]| parts.len() >= 2 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if let Some(at) = name.find(".so.") {
        let version: Vec<&str> = name[at + 4..].split('.').collect();
        return numeric(&version).then(|| format!("{}.so.{}", &name[..at], version[0]));
    }
    let stem = name.strip_suffix(".dylib")?;
    let mut parts: Vec<&str> = stem.split('.').collect();
    let base = parts.remove(0);
    let first = parts.iter().position(|part| part.bytes().all(|b| b.is_ascii_digit()))?;
    let (named, version) = parts.split_at(first);
    numeric(version).then(|| format!("{}.{}.dylib", std::iter::once(base).chain(named.iter().copied()).collect::<Vec<_>>().join("."), version[0]))
}

/// Writes an archive entry through a partial file, keeping its permission
/// bits where the system has them: a program unpacked without them does not run.
fn write_entry(entry: &mut impl io::Read, target: &Path, mode: Option<u32>) -> Result<()> {
    let partial = target.with_file_name(format!("{}.part", target.file_name().unwrap_or_default().to_string_lossy()));
    let mut out = fs::File::create(&partial).with_context(|| format!("create {}", partial.display()))?;
    io::copy(entry, &mut out).with_context(|| format!("extract {}", target.display()))?;
    drop(out);
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&partial, fs::Permissions::from_mode(mode & 0o777)).with_context(|| format!("set permissions of {}", partial.display()))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    if let Err(error) = fs::rename(&partial, target) {
        // a file of the same name a running program holds cannot be replaced, only moved aside
        if !target.is_file() {
            return Err(error).with_context(|| format!("publish {}", target.display()));
        }
        let aside = crate::install_record::set_aside(target).with_context(|| format!("publish {}: {error}", target.display()))?;
        if let Err(error) = fs::rename(&partial, target) {
            // the earlier file goes back, so a failed replace leaves the program as it was
            let restored = fs::rename(&aside, target);
            return Err(error).with_context(|| match restored {
                Ok(()) => format!("publish {}", target.display()),
                Err(back) => format!("publish {} (the earlier file stays at {}: {back})", target.display(), aside.display()),
            });
        }
    }
    Ok(())
}

/// Gives the file `target` beside `link` the name `link` as well; a copy where
/// there are no symbolic links.
fn link_beside(link: &Path, target: &str) -> Result<()> {
    match fs::remove_file(link) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("replace {}", link.display())),
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).with_context(|| format!("link {} to {target}", link.display()))?;
    #[cfg(not(unix))]
    fs::copy(link.with_file_name(target), link).with_context(|| format!("copy {target} to {}", link.display()))?;
    Ok(())
}

/// Runtimes are delivered as Windows builds (.exe, .dll, CUDA); anywhere else
/// they cannot run, so downloading one would only end in "Permission denied".
fn refuse_foreign_runtime(asset: &Asset) -> Result<()> {
    if cfg!(windows) || !asset.is_windows_runtime() {
        return Ok(());
    }
    bail!("{} is a Windows build and is not available on this platform yet", asset.label)
}

/// The cause of a failed download as one code for the hub's counters (CONTRACT, Counters: one cause, one row):
/// disk_space, io, hash_mismatch, size_mismatch, rate_limited, http_status, network, proxy, extract or other. The
/// detail stays in the window and the log; it names hosts and sizes and would split one cause into many rows.
pub fn failure_code(error: &anyhow::Error) -> &'static str {
    for cause in error.chain() {
        if let Some(io) = cause.downcast_ref::<io::Error>() {
            // 112 is Windows' ERROR_DISK_FULL, 28 is ENOSPC
            return if io.kind() == io::ErrorKind::StorageFull || matches!(io.raw_os_error(), Some(112) | Some(28)) { "disk_space" } else { "io" };
        }
    }
    let text = format!("{error:#}");
    if text.contains("differs from the pinned") {
        "hash_mismatch"
    } else if text.contains("bytes, the server said") {
        "size_mismatch"
    } else if text.contains(" minutes") && text.contains(" answered ") {
        "rate_limited"
    } else if text.contains("proxy") {
        "proxy"
    } else if text.contains("could not connect to") || text.contains("could not start") || text.contains("failed after") {
        "network"
    } else if text.contains(" answered ") {
        "http_status"
    } else if text.contains("did not contain") || text.contains("to files it does not contain") || text.contains("extraction") {
        "extract"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONNX_FIXTURE_PIN: &str = "https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip";

    async fn onnx_fixture(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, server)
    }

    #[tokio::test]
    async fn onnx_switches_source_when_the_body_breaks_after_a_successful_probe() {
        use axum::{body::{Body, Bytes}, http::{HeaderMap, Response}};
        let app = axum::Router::new()
            .route("/broken", axum::routing::get(|headers: HeaderMap| async move {
                if headers.contains_key("range") {
                    return Response::new(Body::from("archive"));
                }
                let body = futures_util::stream::unfold(false, |first_sent| async move {
                    if first_sent {
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                        Some((Err(io::Error::from(io::ErrorKind::ConnectionReset)), true))
                    } else {
                        Some((Ok(Bytes::from_static(b"arch")), true))
                    }
                });
                Response::builder().header("content-length", 7).body(Body::from_stream(body)).unwrap()
            }))
            .route("/copy", axum::routing::get(|| async { "archive" }));
        let (base, server) = onnx_fixture(app).await;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("test.zip");
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        fetch_mirrors(&http, &asset(), &target, &Arc::new(Mutex::new(None)), cancel.clone(),
            [format!("{base}/broken"), format!("{base}/copy"), format!("{base}/unused")]).await.unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"archive");
        assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!target.with_extension("part").exists());
        server.abort();
    }

    #[tokio::test]
    async fn an_optional_model_stalled_at_zero_can_be_cancelled_and_retried() {
        use axum::{body::{Body, Bytes}, http::{HeaderMap, Response}};
        use std::sync::atomic::{AtomicBool, Ordering};
        for stall_probe in [true, false] {
            let stalled = Arc::new(tokio::sync::Notify::new());
            let app = axum::Router::new()
                .route("/stall", axum::routing::get({
                    let stalled = stalled.clone();
                    move |headers: HeaderMap| async move {
                        if stall_probe {
                            stalled.notify_one();
                            return std::future::pending().await;
                        }
                        if headers.contains_key("range") { return Response::new(Body::from("archive")); }
                        stalled.notify_one();
                        let body = futures_util::stream::pending::<std::result::Result<Bytes, io::Error>>();
                        Response::builder().header("content-length", 7).body(Body::from_stream(body)).unwrap()
                    }
                }))
                .route("/ready", axum::routing::get(|| async { "archive" }));
            let (base, server) = onnx_fixture(app).await;
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("model.bin");
            let cancel = Arc::new(AtomicBool::new(false));
            let http = reqwest::Client::builder().no_proxy().build().unwrap();
            let progress = Arc::new(Mutex::new(None));
            let model = Asset { url: "https://example.invalid/stall-retry.fixture", ..asset() };
            let url = format!("{base}/stall");
            let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::join!(fetch_from(&http, &model, &target, &progress, cancel.clone(), &url, false), async {
                    stalled.notified().await;
                    cancel.store(true, Ordering::Relaxed);
                })
            }).await.expect("cancel should interrupt a pending HTTP operation");
            assert_eq!(result.unwrap_err().to_string(), "cancelled");
            cancel.store(false, Ordering::Relaxed);
            fetch_from(&http, &model, &target, &progress, cancel, &format!("{base}/ready"), false).await.unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), b"archive");
            server.abort();
        }
    }

    #[tokio::test]
    async fn onnx_cancelled_after_probe_never_starts_a_mirror() {
        use axum::{body::{Body, Bytes}, http::{HeaderMap, Response}};
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let body_started = Arc::new(tokio::sync::Notify::new());
        let mirror_hits = Arc::new(AtomicUsize::new(0));
        let app = axum::Router::new()
            .route("/stall", axum::routing::get({
                let body_started = body_started.clone();
                move |headers: HeaderMap| async move {
                    if headers.contains_key("range") {
                        return Response::new(Body::from("archive"));
                    }
                    body_started.notify_one();
                    let body = futures_util::stream::pending::<std::result::Result<Bytes, io::Error>>();
                    Response::builder().header("content-length", 7).body(Body::from_stream(body)).unwrap()
                }
            }))
            .route("/mirror", axum::routing::get({
                let mirror_hits = mirror_hits.clone();
                move || async move {
                    mirror_hits.fetch_add(1, Ordering::Relaxed);
                    "archive"
                }
            }));
        let (base, server) = onnx_fixture(app).await;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("test.zip");
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(None));
        let asset = asset();
        let download = fetch_mirrors(&http, &asset, &target, &progress, cancel.clone(),
            [format!("{base}/stall"), format!("{base}/mirror"), format!("{base}/mirror")]);
        let (outcome, ()) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(download, async {
                body_started.notified().await;
                cancel.store(true, Ordering::Relaxed);
            })
        }).await.unwrap();
        assert_eq!(outcome.unwrap_err().to_string(), "cancelled");
        assert_eq!(mirror_hits.load(Ordering::Relaxed), 0);
        assert!(!target.exists());
        server.abort();
    }

    #[tokio::test]
    async fn onnx_rejects_wrong_sha_before_publishing_and_uses_only_pinned_mirrors() {
        assert!(onnx_sources(ONNX_FIXTURE_PIN).is_some());
        assert!(onnx_sources(&ONNX_FIXTURE_PIN.replace("win-x64", "unlisted")).is_none());
        let app = axum::Router::new().route("/copy", axum::routing::get(|| async { "archive" }));
        let (base, server) = onnx_fixture(app).await;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("test.zip");
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let entry = Asset { url: ONNX_FIXTURE_PIN, ..asset() };
        let error = fetch_mirrors(&http, &entry, &target, &Arc::new(Mutex::new(None)),
            Arc::new(std::sync::atomic::AtomicBool::new(false)), vec![format!("{base}/copy"); 3]).await.unwrap_err();
        assert_eq!(failure_code(&error), "hash_mismatch");
        assert!(!target.exists());
        assert!(!target.with_extension("part").exists());
        server.abort();
    }

    #[tokio::test]
    async fn a_healthy_mirror_repairs_corrupt_retained_archive_ranges() {
        use axum::http::{header, HeaderMap, StatusCode};
        // Exact bytes of the small, production-pinned CrisperWhisper config.
        const BODY: &[u8] = b"{\n  \"chunk_length\": 30,\n  \"dither\": 0.0,\n  \"feature_extractor_type\": \"WhisperFeatureExtractor\",\n  \"feature_size\": 128,\n  \"hop_length\": 160,\n  \"n_fft\": 400,\n  \"n_samples\": 480000,\n  \"nb_max_frames\": 3000,\n  \"padding_side\": \"right\",\n  \"padding_value\": 0.0,\n  \"processor_class\": \"WhisperProcessor\",\n  \"return_attention_mask\": false,\n  \"sampling_rate\": 16000\n}\n";
        let app = axum::Router::new().route("/copy", axum::routing::get(|headers: HeaderMap| async move {
            let range = headers[header::RANGE].to_str().unwrap().trim_start_matches("bytes=");
            let (start, end) = range.split_once('-').unwrap();
            let start: usize = start.parse().unwrap();
            let end = end.parse::<usize>().unwrap_or(BODY.len() - 1);
            (StatusCode::PARTIAL_CONTENT, [(header::CONTENT_RANGE, format!("bytes {start}-{end}/{}", BODY.len()))], BODY[start..=end].to_vec())
        }));
        let (base, server) = onnx_fixture(app).await;
        let root = tempfile::tempdir().unwrap();
        let entry = Asset { url: "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/preprocessor_config.json", pick: &["test.dll"], ..asset() };
        let target = root.path().join("test.verified-archive");
        let part = partial_for_target(&entry, &target);
        fs::write(&part, vec![0u8; BODY.len()]).unwrap();
        fs::write(crate::chunked::manifest_path(&part), crate::chunked::manifest_of(BODY.len() as u64, &[(0, 10)])).unwrap();
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        fetch_mirrors(&http, &entry, &target, &Arc::new(Mutex::new(None)), Default::default(), [format!("{base}/copy")]).await.unwrap();
        assert_eq!(fs::read(target).unwrap(), BODY);
        server.abort();
    }

    fn asset() -> Asset {
        Asset {
            id: "test-model",
            label: "Test model",
            kind: AssetKind::Model,
            url: "https://example.invalid/model.bin",
            relative_path: "models/model.bin",
            bytes: 64,
            unzip_into: None,
            marker: "",
            pick: &[],
            vram_gb: None,
            note: "",
        }
    }

    #[test]
    fn picked_archive_progress_uses_the_resumable_file_in_the_runtime_folder() {
        let root = tempfile::tempdir().unwrap();
        let downloader = Downloader::new(root.path().to_owned());
        let entry = Asset { kind: AssetKind::Runtime, pick: &["onnxruntime.dll"], unzip_into: Some("cuda"), ..asset() };
        let into = downloader.picked_into(&entry);
        fs::create_dir_all(&into).unwrap();
        let partial = into.join("test-model.part-archive");
        fs::write(&partial, [0u8; 20]).unwrap();
        assert_eq!(downloader.partial_bytes(&entry), 20);
        assert_eq!(partial_for_target(&entry, &into.join("test-model.verified-archive")), partial);
    }

    #[test]
    fn a_partial_file_is_reported_but_never_counts_as_installed() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let entry = asset();
        fs::create_dir_all(downloader.path_of(&entry).parent().unwrap()).unwrap();
        fs::write(downloader.path_of(&entry).with_extension("part"), vec![0u8; 20]).unwrap();

        assert!(!downloader.is_installed(&entry));
        assert_eq!(downloader.partial_bytes(&entry), 20);

        fs::write(downloader.path_of(&entry), vec![0u8; 64]).unwrap();
        assert!(downloader.is_installed(&entry));
        fs::remove_dir_all(&root).ok();
    }

    /// A stopped ranged download leaves its part file at the whole size and the record of its pieces beside it: the
    /// pieces are what is reported, and removing the asset takes both, so the next download starts from nothing.
    #[test]
    fn a_stopped_download_is_reported_by_its_pieces_and_removed_with_their_record() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let entry = asset();
        let part = downloader.path_of(&entry).with_extension("part");
        fs::create_dir_all(part.parent().unwrap()).unwrap();
        fs::File::create(&part).unwrap().set_len(64).unwrap();
        fs::write(crate::chunked::manifest_path(&part), crate::chunked::manifest_of(64, &[(0, 40)])).unwrap();

        assert!(!downloader.is_installed(&entry));
        assert_eq!(downloader.partial_bytes(&entry), 40, "what its one piece has come to");
        downloader.remove(&entry).unwrap();
        assert!(!part.exists() && !crate::chunked::manifest_path(&part).exists());
        assert_eq!(downloader.partial_bytes(&entry), 0);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn windows_runtimes_are_recognised_and_models_never_are() {
        let runtime = Asset { kind: AssetKind::Runtime, url: "https://example.com/tool-windows-x64.zip", marker: "tool.exe", ..asset() };
        assert!(runtime.is_windows_runtime());
        let model = Asset { kind: AssetKind::Model, url: "https://example.com/windows-model.gguf", marker: "", ..asset() };
        assert!(!model.is_windows_runtime());
    }

    #[test]
    fn a_runtime_is_installed_only_when_its_own_marker_is_present() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset {
            id: "test-runtime",
            kind: AssetKind::Runtime,
            relative_path: "runtime/test.zip",
            unzip_into: Some("cuda"),
            marker: "whisper-cli",
            ..asset()
        };
        fs::create_dir_all(downloader.runtime_dir("cuda")).unwrap();
        fs::write(downloader.runtime_dir("cuda").join("ggml.dll"), b"x").unwrap();
        assert!(!downloader.is_installed(&runtime));

        let program = downloader.runtime_dir("cuda").join("whisper-cli.exe");
        fs::write(&program, b"x").unwrap();
        assert!(!downloader.is_installed(&runtime), "no record and no key file in the pin: fetched again");
        crate::install_record::write(&downloader.record_of(&runtime), runtime.url, "", &[program], &root).unwrap();
        assert!(downloader.is_installed(&runtime));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_runtime_of_an_earlier_pin_is_not_installed() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, url: "https://example.invalid/engine-0.1.3.dll", relative_path: "models/engine/engine.dll", bytes: 5, ..asset() };
        let file = downloader.path_of(&runtime);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"0.1.1-build").unwrap();
        assert!(!downloader.is_installed(&runtime), "a file of another size than the pin's is an earlier build");
        let beside = file.with_file_name("engine-helper.dll");
        fs::write(&beside, b"helper").unwrap();
        let record = downloader.record_of(&runtime);
        crate::install_record::write(&record, "https://example.invalid/engine-0.1.1.dll", "", &[file.clone(), beside.clone()], &root).unwrap();
        assert!(!downloader.is_installed(&runtime), "recorded from the earlier pin");
        // today's download has replaced the file in place: the rest of the earlier pin and its record go
        replace_earlier(&record, &root, &file).unwrap();
        assert!(file.exists() && !beside.exists() && !record.exists());
        assert!(!downloader.is_installed(&runtime), "until its record is written the install is under way");
        fs::write(&file, b"0.1.3").unwrap();
        crate::install_record::write(&record, runtime.url, "", &[file.clone()], &root).unwrap();
        assert!(downloader.is_installed(&runtime));
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(windows)]
    #[test]
    fn only_network_failures_of_supported_zip_assets_offer_manual_install() {
        let root = tempfile::tempdir().unwrap();
        let sync = crate::lyrics_sync::LyricsSync::new(root.path());
        let transcribe = crate::lyrics_sync::asset("transcribe-engine").unwrap();
        let target = sync.downloader().path_of(transcribe);
        let certificate = anyhow::anyhow!("could not start {}: invalid certificate", transcribe.url);
        let hint = manual_archive_hint(transcribe, &target, &certificate).unwrap();
        assert_eq!(hint.url, transcribe.url);
        assert_eq!(hint.path, root.path().join("karaoke/runtime/transcribe-windows-cuda13-x64.zip"));
        assert!(manual_archive_hint(transcribe, &target, &anyhow::anyhow!("extraction failed")).is_none());
        assert!(manual_archive_hint(&asset(), &target, &certificate).is_none());
    }

    /// A download that does not arrive leaves an earlier pin's file where it was, and one that arrives takes its
    /// place; a runtime whose file already holds today's pin by its hash is not fetched again.
    #[tokio::test]
    async fn an_earlier_pin_stays_until_todays_download_arrives() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, url: "http://127.0.0.1:9/engine-0.1.3.dll", relative_path: "models/engine/engine.dll", bytes: 5, ..asset() };
        let file = downloader.path_of(&runtime);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"0.1.1-build").unwrap();
        let cell = Arc::new(Mutex::new(None));
        assert!(fetch(&downloader.http, &runtime, &file, &cell, downloader.cancel.clone()).await.is_err(), "nothing answers on port 9");
        assert_eq!(fs::read(&file).unwrap(), b"0.1.1-build", "the earlier file is back in its place");
        assert!(!file.with_file_name("engine.dll.earlier").exists());
        assert!(!holds_todays(false, runtime.url, &file).await, "an unpinned runtime is never taken as today's by its name");
        assert!(holds_todays(true, runtime.url, &file).await, "a model counts by its file");
        fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn a_runtime_with_selected_libraries_can_cancel_a_stalled_probe() {
        let stalled = Arc::new(tokio::sync::Notify::new());
        let app = axum::Router::new().route("/libraries.zip", axum::routing::get({
            let stalled = stalled.clone();
            move || async move { stalled.notify_one(); std::future::pending::<String>().await }
        }));
        let (base, server) = onnx_fixture(app).await;
        let pack = Box::leak(Box::new(Asset {
            url: Box::leak(format!("{base}/libraries.zip").into_boxed_str()),
            pick: &["library.dll"],
            ..asset()
        }));
        let temp = tempfile::tempdir().unwrap();
        let mut downloader = Downloader::new(temp.path().to_owned());
        downloader.http = reqwest::Client::builder().no_proxy().build().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            downloader.install("training", pack).await.unwrap();
            stalled.notified().await;
            assert!(downloader.cancel_for("training").await);
            loop {
                let progress = downloader.active_for("training").await.unwrap();
                if progress.done {
                    assert_eq!(progress.error.as_deref(), Some("cancelled"));
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("cancel should interrupt the archive probe");
        server.abort();
    }

    #[tokio::test]
    async fn a_single_karaoke_file_is_visible_and_retriable_after_a_stalled_probe() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stalled = Arc::new(tokio::sync::Notify::new());
        let ready = Arc::new(AtomicBool::new(false));
        let app = axum::Router::new().route("/model", axum::routing::get({
            let (stalled, ready) = (stalled.clone(), ready.clone());
            move || async move {
                if !ready.load(Ordering::Relaxed) {
                    stalled.notify_one();
                    return std::future::pending().await;
                }
                "archive"
            }
        }));
        let (base, server) = onnx_fixture(app).await;
        let model = Box::leak(Box::new(Asset { url: Box::leak(format!("{base}/model").into_boxed_str()), bytes: 7, ..asset() }));
        let temp = tempfile::tempdir().unwrap();
        let mut downloader = Downloader::new(temp.path().to_owned());
        downloader.http = reqwest::Client::builder().no_proxy().build().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            downloader.install("karaoke", model).await.unwrap();
            stalled.notified().await;
            assert_eq!(downloader.active_for("karaoke").await.unwrap().asset_id, model.id);
            assert!(!downloader.cancel_for("listen").await);
            assert!(downloader.cancel_for("karaoke").await);
            while !downloader.active_for("karaoke").await.unwrap().done { tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
            ready.store(true, Ordering::Relaxed);
            downloader.install("karaoke", model).await.unwrap();
            while !downloader.active_for("karaoke").await.unwrap().done { tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
            assert_eq!(std::fs::read(downloader.path_of(model)).unwrap(), b"archive");
        }).await.expect("single file did not finish after retry");
        server.abort();
    }

    #[tokio::test]
    async fn rejecting_a_second_panel_keeps_the_first_transfer_cancellable() {
        let temp = tempfile::tempdir().unwrap();
        let downloader = Downloader::new(temp.path().to_owned());
        *downloader.progress.lock().await = Some(DownloadProgress {
            asset_id: "onnxruntime".into(), scope: Some("karaoke".into()), downloaded_bytes: 17,
            total_bytes: 100, done: false, error: None, manual_archive: None,
        });
        for scope in ["listen", "karaoke"] {
            downloader.fail(scope, "another download is already running".into()).await;
        }
        let first = downloader.active_for("karaoke").await.unwrap();
        assert!(!first.done && first.error.is_none());
        assert_eq!(first.downloaded_bytes, 17);
        assert!(downloader.active_for("listen").await.unwrap().error.is_some());
        assert!(!downloader.cancel_for("listen").await);
        assert!(downloader.cancel_for("karaoke").await);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_wrong_manually_placed_runtime_archive_is_rejected_before_download() {
        let root = tempfile::tempdir().unwrap();
        let sync = crate::lyrics_sync::LyricsSync::new(root.path());
        let transcribe = crate::lyrics_sync::asset("transcribe-engine").unwrap();
        let target = sync.downloader().path_of(transcribe);
        let placed = target.with_file_name("transcribe-windows-cuda13-x64.zip");
        fs::create_dir_all(placed.parent().unwrap()).unwrap();
        fs::write(&placed, b"archive from another release").unwrap();
        let error = sync.downloader().install_all("karaoke", &[transcribe]).await.unwrap_err();
        assert!(format!("{error:#}").contains("does not match the SHA-256"));
        assert!(placed.is_file());
        assert!(!sync.downloader().is_installed(transcribe));

        let training = crate::training::Training::new(root.path(), "yue2");
        let caption = crate::training::listen_pack().iter().find(|asset| asset.id == "ace-caption").unwrap();
        let placed = training.downloader().path_of(caption);
        fs::create_dir_all(placed.parent().unwrap()).unwrap();
        fs::write(&placed, b"archive from another release").unwrap();
        let error = training.downloader().install_all("listen", &[caption]).await.unwrap_err();
        assert!(format!("{error:#}").contains("does not match the SHA-256"));
        assert!(placed.is_file());
        assert!(!training.downloader().is_installed(caption));
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "needs the three pinned Windows ZIPs under YUE2_MANUAL_ARCHIVES"]
    async fn the_three_published_windows_archives_install_from_disk() {
        let fixtures = PathBuf::from(std::env::var_os("YUE2_MANUAL_ARCHIVES").expect("archive fixture folder"));
        let root = tempfile::tempdir().unwrap();
        let sync = crate::lyrics_sync::LyricsSync::new(root.path());
        let training = crate::training::Training::new(root.path(), "yue2");
        let caption = crate::training::listen_pack().iter().find(|asset| asset.id == "ace-caption").unwrap();
        let trainer = crate::training::pack().iter().find(|asset| asset.id == "music-train").unwrap();
        let transcribe = crate::lyrics_sync::asset("transcribe-engine").unwrap();
        for (downloader, asset) in [(sync.downloader(), transcribe), (training.downloader(), caption), (training.downloader(), trainer)] {
            let name = asset.url.rsplit('/').next().unwrap();
            let placed = downloader.path_of(asset).with_file_name(name);
            fs::create_dir_all(placed.parent().unwrap()).unwrap();
            fs::copy(fixtures.join(name), placed).unwrap();
            assert!(!downloader.is_installed(asset));
            downloader.install_all("manual-archive", &[asset]).await.unwrap();
            assert!(downloader.is_installed(asset));
            let record = crate::install_record::read(&downloader.record_of(asset)).unwrap();
            assert_eq!(record.url, asset.url);
            assert_eq!(Some(record.sha256.as_str()), crate::pin_sha::sha256(asset.url));
        }
    }

    #[test]
    fn onnx_network_failure_shows_the_same_archive_path_the_installer_reads() {
        let asset = crate::lyrics_sync::asset("onnxruntime").unwrap();
        let target = Path::new("studio-data").join(asset.relative_path);
        let hint = manual_archive_hint(asset, &target, &anyhow::anyhow!("could not start download: invalid certificate")).unwrap();
        assert_eq!(Some(hint.path), release_archive_alias(asset, &target));
        assert_eq!(hint.url, asset.url);
        assert!(manual_archive_hint(asset, &target, &anyhow::anyhow!("invalid archive")).is_none());
        if cfg!(target_os = "macos") {
            assert_eq!(onnx_sources(asset.url).unwrap(), &[asset.url]);
        } else {
            let mirrors = onnx_sources(asset.url).unwrap();
            assert_eq!(mirrors.len(), 3);
            assert!(mirrors.iter().all(|url| url.ends_with(asset.url.split("/download/").nth(1).unwrap())));
        }
    }

    #[tokio::test]
    #[ignore = "requires the released and candidate archives under YUE2_ORT_ARCHIVES"]
    async fn upgrade_onnx_from_the_previous_studio_release() {
        let fixtures = PathBuf::from(std::env::var_os("YUE2_ORT_ARCHIVES").expect("archive fixture folder"));
        let root = tempfile::tempdir().unwrap();
        let sync = crate::lyrics_sync::LyricsSync::new(root.path());
        let downloader = sync.downloader();
        let current = crate::lyrics_sync::asset("onnxruntime").unwrap();
        #[cfg(windows)]
        let (previous_url, previous_bytes) = ("https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip", 82_929_944);
        #[cfg(target_os = "linux")]
        let (previous_url, previous_bytes) = ("https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-1.31.1.tgz", 11_413_655);
        #[cfg(target_os = "macos")]
        let (previous_url, previous_bytes) = ("https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz", 36_936_435);
        let previous: &'static Asset = Box::leak(Box::new(Asset { url: previous_url, bytes: previous_bytes, ..*current }));
        let place = |asset: &Asset| {
            let name = asset.url.rsplit('/').next().unwrap();
            let path = downloader.path_of(asset).with_file_name(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::copy(fixtures.join(name), &path).unwrap();
            path
        };
        place(previous);
        downloader.install_all("upgrade", &[previous]).await.unwrap();
        assert!(downloader.is_installed(previous));
        assert_eq!(downloader.earlier(current), Some(Some(previous_url.to_string())));
        let wrong = downloader.path_of(current).with_file_name(current.url.rsplit('/').next().unwrap());
        fs::write(&wrong, b"incomplete new archive").unwrap();
        assert!(downloader.install_all("upgrade", &[current]).await.is_err());
        assert!(downloader.is_installed(previous));
        place(current);
        downloader.install_all("upgrade", &[current]).await.unwrap();
        assert!(downloader.is_installed(current));
        assert!(!downloader.is_installed(previous));
        assert!(sync.onnxruntime_library().unwrap().is_file());
        let record = crate::install_record::read(&downloader.record_of(current)).unwrap();
        assert_eq!(record.url, current.url);
        assert_eq!(Some(record.sha256.as_str()), crate::pin_sha::sha256(current.url));
        downloader.install_all("upgrade", &[current]).await.unwrap();
    }

    /// An install cut off after the archive's key file was unpacked and before
    /// its record was written: the status poll in between must not take the
    /// half-unpacked folder over as a download from before records.
    #[test]
    fn an_install_cut_off_before_its_record_is_not_installed() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let url = "https://github.com/timoncool/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip";
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, url, relative_path: "runtime/onnxruntime.zip", unzip_into: Some("onnx"), marker: "onnxruntime.dll", ..asset() };
        let (key, size) = crate::install_record::key_file(url).unwrap();
        let record = downloader.record_of(&runtime);
        replace_earlier(&record, &root, &downloader.path_of(&runtime)).unwrap();
        fs::create_dir_all(downloader.runtime_dir("onnx")).unwrap();
        let unpacked = downloader.runtime_dir("onnx").join(key);
        fs::File::create(&unpacked).unwrap().set_len(size).unwrap();
        assert!(!downloader.is_installed(&runtime), "half unpacked, counted as installed");
        assert!(!record.exists(), "half unpacked, recorded as installed");

        // the studio closed here; at the next start the folder is still not taken over
        let reopened = Downloader::new(root.clone());
        assert!(!reopened.is_installed(&runtime) && !record.exists());

        // the next install, its download on disk, marks itself again and ends with the record alone
        replace_earlier(&record, &root, &reopened.path_of(&runtime)).unwrap();
        fs::File::create(&unpacked).unwrap().set_len(size).unwrap();
        crate::install_record::write(&record, url, "abc", &[unpacked], &root).unwrap();
        assert!(reopened.is_installed(&runtime));
        let marks: Vec<_> = fs::read_dir(record.parent().unwrap()).unwrap().flatten().filter(|entry| entry.path().is_file()).map(|entry| entry.file_name()).collect();
        assert_eq!(marks, vec![record.file_name().unwrap().to_owned()], "only the record is left beside the archive");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn capabilities_sharing_a_runtime_root_find_one_unpacked_build() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let shared = root.join("hotstep");
        let midi = Downloader::with_runtimes(root.join("midi"), shared.clone());
        let training = Downloader::with_runtimes(root.join("training"), shared.clone());
        let tool = |id: &'static str, marker: &'static str| Asset { id, kind: AssetKind::Runtime, relative_path: "tools.zip", unzip_into: Some("hotstep-tools-1"), marker, ..asset() };
        let transcriber = tool("music-midi", "music-midi.exe");
        let trainer = tool("music-train", "music-train.exe");
        assert_eq!(midi.runtime_dir("hotstep-tools-1"), training.runtime_dir("hotstep-tools-1"));
        fs::create_dir_all(midi.runtime_dir("hotstep-tools-1")).unwrap();
        fs::create_dir_all(midi.root()).unwrap();
        fs::create_dir_all(training.root()).unwrap();
        for name in ["music-midi.exe", "music-train.exe", "ggml-cuda.dll"] {
            fs::write(shared.join("hotstep-tools-1").join(name), b"x").unwrap();
        }
        let unpacked = vec![shared.join("hotstep-tools-1").join("music-midi.exe"), shared.join("hotstep-tools-1").join("music-train.exe")];
        crate::install_record::write(&midi.record_of(&transcriber), transcriber.url, "", &unpacked, midi.root()).unwrap();
        crate::install_record::write(&training.record_of(&trainer), trainer.url, "", &unpacked, training.root()).unwrap();
        assert!(midi.is_installed(&transcriber) && training.is_installed(&trainer));
        assert_eq!(midi.path_of(&transcriber), root.join("midi").join("tools.zip"), "the archive downloads into the capability's own folder");
        fs::remove_dir_all(&root).ok();
    }


    /// A package carries one library for several platforms under the same
    /// name: a pick by path takes the entry it names, a bare name any entry.
    #[test]
    fn a_pick_by_path_takes_that_entry_and_not_its_namesakes() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(root.join("out")).unwrap();
        let archive = root.join("package.nupkg");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for (path, body) in [("bin/x64-xbox/DirectML.dll", "xbox"), ("bin/x64-win/DirectML.dll", "win"), ("lib/readme.txt", "notes")] {
            zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();

        extract_named_local(&archive, &["bin/x64-win/DirectML.dll", "readme.txt"], &root.join("out")).unwrap();
        assert_eq!(fs::read_to_string(root.join("out").join("DirectML.dll")).unwrap(), "win");
        assert_eq!(fs::read_to_string(root.join("out").join("readme.txt")).unwrap(), "notes");
        assert!(extract_named_local(&archive, &["bin/arm64-win/DirectML.dll"], &root.join("out")).is_err());
        fs::remove_dir_all(&root).ok();
    }

    /// A Linux release archive as NVIDIA and Microsoft ship them: one versioned
    /// library, the loader's names as links to it, a program, and headers.
    fn linux_archive(root: &Path) -> PathBuf {
        let archive = root.join("package.part-archive");
        let encoder = flate2::write::GzEncoder::new(fs::File::create(&archive).unwrap(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        let mut add = |path: &str, mode: u32, body: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            tar.append_data(&mut header, path, body).unwrap();
        };
        add("pkg/lib/libfoo.so.1.2.3", 0o755, b"library");
        add("pkg/bin/tool", 0o755, b"program");
        add("pkg/include/foo.h", 0o644, b"header");
        for (link, target) in [("pkg/lib/libfoo.so", "libfoo.so.1"), ("pkg/lib/libfoo.so.1", "libfoo.so.1.2.3")] {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            tar.append_link(&mut header, link, target).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        archive
    }

    /// A program packed without its execute bit - a zip or a tar made on Windows - is made executable after unpacking;
    /// its libraries and data files are left as they came.
    #[test]
    fn a_program_unpacked_without_its_execute_bit_is_made_executable() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let zip_archive = root.join("tool.part-archive");
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(&zip_archive).unwrap());
            for name in ["audiocpp_server", "libggml.so", "README.txt"] {
                zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                io::Write::write_all(&mut zip, name.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        let tar_archive = root.join("llama.part-archive");
        {
            let encoder = flate2::write::GzEncoder::new(fs::File::create(&tar_archive).unwrap(), flate2::Compression::fast());
            let mut tar = tar::Builder::new(encoder);
            for (path, mode) in [("llama-server", 0o644), ("libllama.so", 0o644)] {
                let mut header = tar::Header::new_gnu();
                header.set_size(path.len() as u64);
                header.set_mode(mode);
                header.set_cksum();
                tar.append_data(&mut header, path, path.as_bytes()).unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap();
        }
        for (archive, marker, program) in [(&zip_archive, "audiocpp_server", "audiocpp_server"), (&tar_archive, "llama-server", "llama-server")] {
            let out = root.join(format!("out-{marker}"));
            let files = extract_archive(archive, &out).unwrap();
            let programs: Vec<String> = programs_among(marker, &files).iter().map(|path| path.file_name().unwrap().to_string_lossy().into_owned()).collect();
            assert_eq!(programs, vec![program.to_string()], "{marker}");
            make_programs_executable(marker, &files).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(fs::metadata(out.join(program)).unwrap().permissions().mode() & 0o111, 0o111, "{program} is executable");
            }
        }
        assert!(programs_among("", &[root.join("anything")]).is_empty(), "an asset without a marker names no program");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_tar_unpacks_its_libraries_and_programs_with_the_loader_names() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let archive = linux_archive(&root);
        extract_archive(&archive, &root.join("out")).unwrap();
        for name in ["libfoo.so.1.2.3", "libfoo.so.1", "libfoo.so"] {
            assert_eq!(fs::read_to_string(root.join("out").join(name)).unwrap(), "library", "{name}");
        }
        assert_eq!(fs::read_to_string(root.join("out").join("tool")).unwrap(), "program");
        assert!(!root.join("out").join("foo.h").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(fs::metadata(root.join("out").join("tool")).unwrap().permissions().mode() & 0o111, 0);
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_library_picked_from_a_tar_by_its_loader_name_lands_with_that_name() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(root.join("out")).unwrap();
        let archive = linux_archive(&root);
        extract_named_local(&archive, &["libfoo.so.1"], &root.join("out")).unwrap();
        assert_eq!(fs::read_to_string(root.join("out").join("libfoo.so.1")).unwrap(), "library");
        assert!(root.join("out").join("libfoo.so.1.2.3").is_file());
        assert!(!root.join("out").join("tool").exists());
        assert!(extract_named_local(&archive, &["libbar.so.1"], &root.join("out")).is_err());
        fs::remove_dir_all(&root).ok();
    }

    /// A library that comes by its full version gets the name the loader asks for, on Linux and on macOS alike
    /// (the same rule as Speech Studio's setup).
    #[test]
    fn a_library_by_its_full_version_gets_the_name_the_loader_asks_for() {
        assert_eq!(soname_of("libggml-base.so.0.15.1").as_deref(), Some("libggml-base.so.0"));
        assert_eq!(soname_of("libggml-base.0.15.1.dylib").as_deref(), Some("libggml-base.0.dylib"));
        assert_eq!(soname_of("libllama-common.0.7.0.dylib").as_deref(), Some("libllama-common.0.dylib"));
        for name in ["libggml.0.dylib", "libqwen.dylib", "libggml.so.0", "libfoo.so", "bs_roformer-cli", "libonnxruntime.dylib"] {
            assert_eq!(soname_of(name), None, "{name}");
        }
    }

    /// BSRoformer.cpp's processor and macOS archives as v0.1.0 ships them: the program and the ggml
    /// libraries by their full versions, no links; the program asks for libggml.so.0 / @rpath/libggml.0.dylib.
    #[test]
    fn an_archive_of_full_version_libraries_unpacks_with_the_loader_names() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("bsroformer.part-archive");
        let encoder = flate2::write::GzEncoder::new(fs::File::create(&archive).unwrap(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        for (path, mode) in [("bs_roformer-cli", 0o755), ("libggml.0.15.1.dylib", 0o644), ("libggml-metal.0.15.1.dylib", 0o644), ("libggml.so.0.15.1", 0o644)] {
            let mut header = tar::Header::new_gnu();
            header.set_size(path.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            tar.append_data(&mut header, path, path.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        let files = extract_archive(&archive, &root.join("out")).unwrap();
        for (short, full) in [("libggml.0.dylib", "libggml.0.15.1.dylib"), ("libggml-metal.0.dylib", "libggml-metal.0.15.1.dylib"), ("libggml.so.0", "libggml.so.0.15.1")] {
            assert_eq!(fs::read_to_string(root.join("out").join(short)).unwrap(), full, "{short}");
            assert!(files.contains(&root.join("out").join(short)), "{short} is recorded with the install");
        }
        assert!(root.join("out").join("bs_roformer-cli").is_file());
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_picked_library_takes_the_file_its_name_links_to() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, unzip_into: Some("cuda"), pick: &["libfoo.so.1"], ..asset() };
        fs::create_dir_all(downloader.picked_into(&runtime)).unwrap();
        let archive = linux_archive(&root);
        let files = extract_named_local(&archive, runtime.pick, &downloader.picked_into(&runtime)).unwrap();
        // the record sits where the download would have, a folder a real download makes
        fs::create_dir_all(downloader.record_of(&runtime).parent().unwrap()).unwrap();
        crate::install_record::write(&downloader.record_of(&runtime), runtime.url, "", &files, &root).unwrap();
        assert!(downloader.is_installed(&runtime));
        assert_eq!(downloader.remove(&runtime).unwrap(), "library".len() as u64);
        assert!(!downloader.runtime_dir("cuda").join("libfoo.so.1.2.3").exists());
        assert!(!downloader.is_installed(&runtime));
        fs::remove_dir_all(&root).ok();
    }

    /// A failed download reaches the hub as one code for its cause, never the text that names a host or a size.
    #[test]
    fn a_failed_download_is_one_code_for_its_cause() {
        let code = |error: anyhow::Error| failure_code(&error);
        assert_eq!(code(anyhow::Error::new(io::Error::from_raw_os_error(112)).context("write the part")), "disk_space");
        assert_eq!(code(anyhow::Error::new(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))), "io");
        assert_eq!(code(anyhow::anyhow!("model.gguf: its SHA-256 ab differs from the pinned cd; the file is removed, download it again")), "hash_mismatch");
        assert_eq!(code(anyhow::anyhow!("Model has 10 bytes, the server said 20")), "size_mismatch");
        assert_eq!(code(anyhow::anyhow!("https://h/x answered 429 Too Many Requests for 5 minutes")), "rate_limited");
        assert_eq!(code(anyhow::anyhow!("could not connect to huggingface.co (3 attempts) for https://huggingface.co/x: timed out")), "network");
        assert_eq!(code(anyhow::anyhow!("range 0-9 failed after 8 attempts: reset")), "network");
        assert_eq!(code(anyhow::anyhow!("https://h/x answered 404 Not Found")), "http_status");
        assert_eq!(code(anyhow::anyhow!("a.zip did not contain b.dll")), "extract");
        assert_eq!(code(anyhow::anyhow!("something new")), "other");
    }
}
