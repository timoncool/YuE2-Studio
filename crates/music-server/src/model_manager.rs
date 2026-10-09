use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

pub const ENGINE_ID: &str = "yue2-cpp";
const REPOSITORY: &str = "Serveurperso/YuE2-GGUF";
const REVISION: &str = "e630f2b8f6aedf8254c3186438633c1b5eeaf80a";
/// Part of every set, picked or not: the engine merges it under every render
/// and the trainer keeps it under every LoRA.
const COMPANION: &str = "companion-v9";
pub const COMPANION_REPOSITORY: &str = "Mothersuperior/yue2-mothersuperior-realaudio-tokenizer-v4";

/// The companion in any of the precisions and layouts its repository
/// publishes: one more copy on top of the built-in one would merge it twice.
pub fn is_companion_file(repo: &str, path: &str) -> bool {
    repo.eq_ignore_ascii_case(COMPANION_REPOSITORY) && path.starts_with("nar_lora_joint_v9")
}

/// The recommendation is a property of the machine, not of the catalog.
fn recommended_profile() -> &'static str {
    crate::hardware::recommended_local_profile()
}

#[derive(Clone)]
pub struct ModelManager {
    root: PathBuf,
    state_path: PathBuf,
    http: reqwest::Client,
    state: Arc<RwLock<PersistentState>>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    pub engine_id: &'static str,
    pub repository: &'static str,
    pub revision: &'static str,
    pub recommended_profile_id: &'static str,
    pub profiles: Vec<Profile>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub id: &'static str,
    pub label: &'static str,
    pub backend: &'static str,
    pub installable: bool,
    pub recommended: bool,
    pub components: Vec<&'static str>,
    pub total_bytes: u64,
    /// Memory this machine needs for the set: less when its card holds the weights.
    pub ram_needed_gb: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub id: &'static str,
    pub kind: &'static str,
    pub filename: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    /// Where this file comes from. The lighter quantisations are published by
    /// someone else, and a single hard-coded repository is what kept them out.
    pub repository: &'static str,
    pub revision: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallRequest {
    pub profile_id: Option<String>,
    #[serde(default)]
    pub component_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagerStatus {
    pub engine_id: &'static str,
    pub model_root: String,
    pub first_run: bool,
    pub ready: bool,
    pub download_pending: u64,
    pub recommended_profile_id: String,
    pub active: Option<DownloadJob>,
    pub components: Vec<ComponentStatus>,
    pub installed_components: Vec<String>,
    /// The files the selected set resolves to, named rather than implied.
    pub profile_files: Option<ProfileModelFiles>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentStatus {
    pub id: &'static str,
    pub installed: bool,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: String,
    pub profile_id: Option<String>,
    pub component_ids: Vec<String>,
    pub status: DownloadStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ProfileModelFiles {
    pub backbone: String,
    pub vae: String,
    /// SheetSage2 is optional: without it the studio generates but cannot
    /// read a recording into a score.
    pub transcriber: Option<String>,
    /// The decoder adapter paired with the tokenizer head of the training codes.
    pub companion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Downloading,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistentState {
    active: Option<DownloadJob>,
}

impl ModelManager {
    pub fn from_environment() -> Result<Self> {
        let root = env::var_os("YUE_MODELS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(default_model_root);
        validate_model_root(&root)?;
        let state_path = root.join(".studio-download-state.json");
        let mut state = fs::read_to_string(&state_path)
            .ok()
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default();
        if recover_interrupted_download(&mut state) {
            persist_state_file(&state_path, &state)?;
        }
        remove_retired_files(&root);
        Ok(Self {
            root,
            state_path,
            http: crate::net::client(),
            state: Arc::new(RwLock::new(state)),
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Where the weights live, for anything that needs to check a file exists.
    pub fn models_directory(&self) -> &Path {
        &self.root
    }

    pub fn catalog(&self) -> Catalog {
        Catalog {
            engine_id: ENGINE_ID,
            repository: REPOSITORY,
            revision: REVISION,
            recommended_profile_id: recommended_profile(),
            profiles: profiles(),
            components: components(),
        }
    }

    /// `target` is the set the user actually selected. Progress and readiness
    /// are reported against it, so a machine that deliberately runs the Light
    /// set is never told it is missing the hardware-recommended download.
    pub async fn status(&self, target: Option<InstallRequest>) -> ManagerStatus {
        let active = self.state.read().await.active.clone();
        let root = self.root.clone();
        // SHA-256 over a GGUF can take seconds. Keep it off the Tokio request
        // workers so setup polling and cancellation remain available.
        tokio::task::spawn_blocking(move || status_snapshot(root, active, target))
            .await
            .unwrap_or_else(|_| status_snapshot(PathBuf::from("."), None, None))
    }

    /// Deletes the files of the named components, freeing the disk they take.
    ///
    /// Downloading is undoable only if the user can also undo it. Ten gigabytes
    /// of weights with no way to remove them from inside the studio is how
    /// people end up hunting through their profile folder by hand.
    pub async fn remove(&self, component_ids: &[String]) -> Result<RemovalReport> {
        if self.state.read().await.active.as_ref().is_some_and(|job| matches!(job.status, DownloadStatus::Downloading)) {
            bail!("a model download is running; cancel it before removing files");
        }
        let catalog = components();
        let mut ids: Vec<String> = component_ids.to_vec();
        // a transcriber head goes with the MERT file of its precision, which the set lists for it
        for head in catalog.iter().filter(|component| component.kind == "transcriber" && component_ids.iter().any(|id| id == component.id)) {
            if let Some(base) = catalog.iter().find(|base| base.kind == "transcriber-base" && precision(base.filename) == precision(head.filename)) {
                if !ids.iter().any(|id| id == base.id) {
                    ids.push(base.id.to_string());
                }
            }
        }
        let mut removed = Vec::new();
        let mut freed_bytes = 0u64;
        for id in &ids {
            let component = catalog
                .iter()
                .find(|component| component.id == *id)
                .with_context(|| format!("unknown component '{id}'"))?;
            let path = self.root.join(&component.filename);
            let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            match fs::remove_file(&path) {
                Ok(()) => {
                    freed_bytes += size;
                    removed.push(component.id.to_string());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("remove {}", path.display())),
            }
            // A half-finished download of the same component is just as much
            // disk as the finished one.
            let _ = fs::remove_file(self.root.join(format!("{}.part", component.filename)));
        }
        // A remembered download resumes on the next start. Deleting the files
        // without forgetting the job is how twenty-six gigabytes came back by
        // themselves after being removed.
        {
            let mut state = self.state.write().await;
            let resumes_removed = state
                .active
                .as_ref()
                .is_some_and(|job| job.component_ids.iter().any(|id| component_ids.contains(id)));
            if resumes_removed {
                state.active = None;
                let _ = persist_state_file(&self.state_path, &state);
            }
        }
        Ok(RemovalReport { removed, freed_bytes })
    }

    pub async fn install(&self, request: InstallRequest) -> Result<DownloadJob> {
        let mut selection = resolve_install(request)?;
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create model root {}", self.root.display()))?;
        preflight_space(&self.root, &selection)?;
        // Progress starts from what is already published on disk. This uses the
        // cheap size check rather than a SHA-256 sweep: hashing a resumed 10 GB
        // set here would block this request — and every setup/status poll behind
        // it — for tens of seconds. Each component is still hash-verified in
        // `download_selection` before it is skipped or published.
        selection.already_present_bytes = selection
            .components
            .iter()
            .filter(|component| published_component(&self.root.join(component.filename), component))
            .map(|component| component.bytes)
            .sum();
        let mut state = self.state.write().await;
        if state.active.as_ref().is_some_and(|job| matches!(job.status, DownloadStatus::Downloading)) {
            bail!("a model download is already running");
        }
        self.cancelled.store(false, Ordering::SeqCst);
        let job = DownloadJob {
            id: uuid::Uuid::now_v7().to_string(),
            profile_id: selection.profile_id.clone(),
            component_ids: selection.components.iter().map(|component| component.id.into()).collect(),
            status: DownloadStatus::Downloading,
            downloaded_bytes: 0,
            total_bytes: selection.total_bytes.saturating_sub(selection.already_present_bytes),
            error: None,
        };
        state.active = Some(job.clone());
        self.persist_locked(&state)?;
        drop(state);
        let manager = self.clone();
        tokio::spawn(async move { manager.download_selection(selection).await });
        Ok(job)
    }

    pub async fn cancel(&self, target: Option<InstallRequest>) -> Result<ManagerStatus> {
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(self.status(target).await)
    }

    pub async fn download_job(&self, id: &str) -> Option<DownloadJob> {
        self.state.read().await.active.as_ref().filter(|job| job.id == id).cloned()
    }

    pub fn installed_profile_files(&self, profile_id: &str) -> Result<ProfileModelFiles> {
        let selection = resolve_install(InstallRequest { profile_id: Some(profile_id.into()), component_ids: vec![] })?;
        self.installed_files_from_selection(selection, &format!("selected profile '{profile_id}'"))
    }

    /// Resolves an explicitly selected complete set. Component ids, never
    /// filenames: callers cannot hand arbitrary paths to the native engine.
    pub fn installed_component_files(&self, component_ids: &[String]) -> Result<ProfileModelFiles> {
        let selection = resolve_install(InstallRequest { profile_id: None, component_ids: component_ids.to_vec() })?;
        self.installed_files_from_selection(selection, "selected custom component set")
    }

    /// A saved hand-picked set stays chosen while the files picked for it are
    /// there. The companion is not the user's pick: missing, it is fetched
    /// with the set rather than dropping the set for the recommended one.
    pub fn picked_components_installed(&self, component_ids: &[String]) -> bool {
        resolve_install(InstallRequest { profile_id: None, component_ids: component_ids.to_vec() }).is_ok_and(|selection| {
            selection
                .components
                .iter()
                .filter(|component| component.id != COMPANION && !optional_kind(component.kind))
                .all(|component| published_component(&self.root.join(component.filename), component))
        })
    }

    /// The files of a set to start the engine on. A transcriber not on disk
    /// yet leaves the set without one rather than keeping the engine down.
    fn installed_files_from_selection(&self, selection: ResolvedInstall, label: &str) -> Result<ProfileModelFiles> {
        let mut transcriber_complete = true;
        for component in &selection.components {
            let path = self.root.join(component.filename);
            if !published_component(&path, component) {
                if optional_kind(component.kind) {
                    transcriber_complete = false;
                    continue;
                }
                bail!("{label} is incomplete: missing or truncated {}", component.filename);
            }
        }
        let mut files = profile_files_from_components(&selection.components);
        if !transcriber_complete {
            files.transcriber = None;
        }
        Ok(files)
    }

    async fn download_selection(&self, selection: ResolvedInstall) {
        let result = async {
            for component in &selection.components {
                if self.cancelled.load(Ordering::SeqCst) {
                    bail!("cancelled");
                }
                if verified_file_async(self.root.join(component.filename), component.clone()).await? {
                    self.set_published_progress(&selection).await?;
                    continue;
                }
                self.download_component(component).await?;
                // Streaming only counts the bytes that crossed the network. A
                // component resumed from a complete `.part`, or already present
                // from an earlier attempt, would otherwise leave the bar short
                // of 100% on a successful install.
                self.set_published_progress(&selection).await?;
            }
            Ok(())
        }
        .await;
        let mut state = self.state.write().await;
        if let Some(job) = &mut state.active {
            match result {
                Ok(()) => job.status = DownloadStatus::Completed,
                Err(error) if self.cancelled.load(Ordering::SeqCst) => {
                    job.status = DownloadStatus::Cancelled;
                    job.error = Some(error.to_string());
                }
                Err(error) => {
                    job.status = DownloadStatus::Failed;
                    job.error = Some(error.to_string());
                }
            }
            let _ = self.persist_locked(&state);
        }
    }

    /// Fetches one component over four connections at once.
    ///
    /// Eleven gigabytes down a single TCP stream was the studio waiting on a
    /// fraction of the line for no reason. `chunked` cuts the file into pieces,
    /// retries them one by one, and remembers which of them landed - so an
    /// interrupted install resumes to within sixteen megabytes instead of
    /// starting the file again.
    ///
    /// Components are still fetched one after another, because four connections
    /// is the whole budget: Hugging Face's Xet storage drops them above that,
    /// and a dropped range leaves a hole in a file of exactly the right size.
    async fn download_component(&self, component: &Component) -> Result<()> {
        let target = self.root.join(component.filename);
        let part = part_path(&target);
        let url = crate::net::model_url(&format!(
            "https://huggingface.co/{}/resolve/{}/{}?download=true",
            component.repository, component.revision, component.filename
        ));

        let plan = crate::chunked::probe(&self.http, &url).await?;
        let written = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

        // The job's bar counts bytes as they arrive; the pieces count their own.
        // This turns the second into the first without double counting, which is
        // what a shared counter written by four tasks would do.
        let reporter = {
            let (written, manager) = (written.clone(), self.clone());
            tokio::spawn(async move {
                let mut reported = 0u64;
                loop {
                    let value = written.load(Ordering::Relaxed);
                    if value > reported {
                        let _ = manager.add_progress(value - reported).await;
                        reported = value;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            })
        };
        let outcome = crate::chunked::fetch(&self.http, &url, &part, plan, written, self.cancelled.clone()).await;
        reporter.abort();
        if self.cancelled.load(Ordering::SeqCst) {
            bail!("cancelled");
        }
        outcome?;

        self.publish_verified_part(component, &part, &target).await
    }

    /// Publishes a completed `.part` only after its SHA-256 matches the pinned
    /// Hugging Face LFS oid, so a truncated or corrupted transfer can never be
    /// presented to the engine as an installed component.
    async fn publish_verified_part(&self, component: &Component, part: &Path, target: &Path) -> Result<()> {
        // No size comparison here: the SHA-256 below is the check, and it is a
        // real one. A byte count compiled into the studio can only ever add a
        // way to reject a perfectly good file.
        if !verified_file_async(part.to_path_buf(), component.clone()).await? {
            fs::remove_file(part).ok();
            bail!("{} SHA-256 does not match the pinned Hugging Face LFS oid; the partial file was discarded so the next attempt starts clean", component.filename);
        }
        fs::rename(part, target).with_context(|| format!("publish {}", target.display()))?;
        Ok(())
    }

    /// Re-bases progress on what is actually published on disk.
    async fn set_published_progress(&self, selection: &ResolvedInstall) -> Result<()> {
        let published: u64 = selection
            .components
            .iter()
            .filter(|component| published_component(&self.root.join(component.filename), component))
            .map(|component| component.bytes)
            .sum::<u64>()
            .saturating_sub(selection.already_present_bytes);
        let mut state = self.state.write().await;
        if let Some(job) = &mut state.active {
            job.downloaded_bytes = published.min(job.total_bytes);
            self.persist_locked(&state)?;
        }
        Ok(())
    }

    async fn add_progress(&self, bytes: u64) -> Result<()> {
        let mut state = self.state.write().await;
        if let Some(job) = &mut state.active {
            job.downloaded_bytes = (job.downloaded_bytes + bytes).min(job.total_bytes);
            self.persist_locked(&state)?;
        }
        Ok(())
    }

    fn persist_locked(&self, state: &PersistentState) -> Result<()> {
        persist_state_file(&self.state_path, state)
    }
}

fn recover_interrupted_download(state: &mut PersistentState) -> bool {
    let Some(job) = &mut state.active else { return false; };
    if !matches!(job.status, DownloadStatus::Downloading) { return false; }
    job.status = DownloadStatus::Cancelled;
    job.error = Some("Download was interrupted by an application restart. Partial .part files were preserved and the next download resumes them with HTTP Range.".into());
    true
}

fn persist_state_file(path: &Path, state: &PersistentState) -> Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let temporary = part_path(path);
    fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

/// Direct `music-server` launches use the same data location as the shell;
/// the environment variable stays the explicit override.
fn default_model_root() -> PathBuf {
    // Models are the largest thing the studio owns, so they follow its data
    // root: beside the executable when it is portable.
    if let Some(root) = crate::studio_data_root() {
        return root.join("models").join(ENGINE_ID);
    }

    #[cfg(windows)]
    {
        if let Some(root) = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA")) {
            return PathBuf::from(root).join("YuE2 Studio").join("models").join(ENGINE_ID);
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(root) = env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(root).join("yue2-studio/models").join(ENGINE_ID);
        }
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/yue2-studio/models").join(ENGINE_ID);
        }
    }

    env::temp_dir().join("yue2-studio/models").join(ENGINE_ID)
}

/// What a removal actually did.
#[derive(Debug, Clone, Serialize)]
pub struct RemovalReport {
    pub removed: Vec<String>,
    pub freed_bytes: u64,
}

#[derive(Debug, Clone)]
struct ResolvedInstall {
    profile_id: Option<String>,
    components: Vec<Component>,
    total_bytes: u64,
    already_present_bytes: u64,
}

fn resolve_install(request: InstallRequest) -> Result<ResolvedInstall> {
    if request.profile_id.is_some() && !request.component_ids.is_empty() {
        bail!("select either a complete profile or an advanced component set, not both");
    }
    // An empty request is a mistake, not an instruction to download the default
    // set. Silently substituting a profile turned a field-name mismatch into
    // twenty-six gigabytes nobody asked for.
    if request.profile_id.is_none() && request.component_ids.is_empty() {
        bail!("nothing was selected to download: name a profile or the components");
    }
    let profiles = profiles();
    let profile = request.profile_id.unwrap_or_else(|| recommended_profile().into());
    let (profile_id, mut ids) = if request.component_ids.is_empty() {
        let selected = profiles.iter().find(|candidate| candidate.id == profile)
            .with_context(|| format!("unknown profile '{profile}'"))?;
        if !selected.installable || selected.backend != ENGINE_ID {
            bail!("profile '{}' requires the '{}' backend and is not installable by the native GGUF manager", selected.id, selected.backend);
        }
        (Some(selected.id.into()), selected.components.clone())
    } else {
        (None, request.component_ids.iter().map(String::as_str).collect::<Vec<_>>())
    };
    for id in ids.iter_mut() {
        if let Some((_, now)) = RETIRED_TRANSCRIBERS.iter().find(|(old, _)| old == id) {
            *id = now;
        }
    }
    if !ids.contains(&COMPANION) {
        ids.push(COMPANION);
    }
    let catalog = components();
    // the transcriber head reads MERT from the file of its own precision beside it
    let bases: Vec<&'static str> = ids
        .iter()
        .filter_map(|id| catalog.iter().find(|candidate| candidate.id == *id && candidate.kind == "transcriber"))
        .filter_map(|head| catalog.iter().find(|base| base.kind == "transcriber-base" && precision(base.filename) == precision(head.filename)))
        .map(|base| base.id)
        .collect();
    for base in bases {
        if !ids.contains(&base) {
            ids.push(base);
        }
    }
    let selected: Vec<Component> = ids
        .iter()
        .map(|id| catalog.iter().find(|candidate| candidate.id == *id).cloned().with_context(|| format!("unknown component '{id}'")))
        .collect::<Result<_>>()?;
    validate_complete_set(&selected)?;
    let total_bytes = selected.iter().map(|component| component.bytes).sum();
    Ok(ResolvedInstall { profile_id, components: selected, total_bytes, already_present_bytes: 0 })
}

fn validate_complete_set(selected: &[Component]) -> Result<()> {
    for kind in ["backbone", "vae", "companion"] {
        if selected.iter().filter(|component| component.kind == kind).count() != 1 {
            bail!("a runnable YuE2 installation requires exactly one {kind} component");
        }
    }
    let heads: Vec<&Component> = selected.iter().filter(|component| component.kind == "transcriber").collect();
    if heads.len() > 1 {
        bail!("a YuE2 installation takes at most one transcriber");
    }
    let bases: Vec<&Component> = selected.iter().filter(|component| component.kind == "transcriber-base").collect();
    if bases.len() != heads.len() || heads.iter().zip(&bases).any(|(head, base)| precision(head.filename) != precision(base.filename)) {
        bail!("the transcriber needs MERT in its own precision beside it");
    }
    Ok(())
}

fn profile_files_from_components(components: &[Component]) -> ProfileModelFiles {
    let filename = |kind| components.iter().find(|component| component.kind == kind).map(|component| component.filename.to_owned());
    ProfileModelFiles {
        backbone: filename("backbone").expect("complete set"),
        vae: filename("vae").expect("complete set"),
        transcriber: filename("transcriber"),
        companion: filename("companion").expect("complete set"),
    }
}

fn preflight_space(root: &Path, selection: &ResolvedInstall) -> Result<()> {
    let available = fs2::available_space(root)?;
    // This preflight runs on the HTTP request path. A final GGUF is only
    // published after its SHA-256 has been checked and atomically renamed, so
    // checking its expected final size here is sufficient. Re-hashing a 6 GB
    // language model merely to calculate free space made the first-run UI look
    // frozen.
    let missing = selection.components.iter().filter(|component| !published_component(&root.join(component.filename), component)).map(|component| component.bytes).sum::<u64>();
    if available < missing {
        bail!("not enough disk space: need {missing} bytes, only {available} bytes available");
    }
    Ok(())
}

fn status_snapshot(root: PathBuf, active: Option<DownloadJob>, target: Option<InstallRequest>) -> ManagerStatus {
    let component_statuses: Vec<_> = components()
        .into_iter()
        .map(|component| ComponentStatus {
            id: component.id,
            // Final files are atomically published only after a full SHA-256
            // verification in `download_component`. Status is polled often,
            // therefore it must never hash multi-gigabyte weights again.
            installed: published_component(&root.join(component.filename), &component),
            bytes: component.bytes,
        })
        .collect();
    let target = target
        .and_then(|request| resolve_install(request).ok())
        .or_else(|| resolve_install(InstallRequest { profile_id: Some(recommended_profile().into()), component_ids: vec![] }).ok());
    let installed = |id: &str| component_statuses.iter().find(|component| component.id == id).is_some_and(|component| component.installed);
    let ready = target.as_ref().is_some_and(|selection| selection.components.iter().filter(|component| !optional_kind(component.kind)).all(|component| installed(component.id)));
    let download_pending = target.as_ref().map(|selection| selection.components.iter()
        .filter(|component| !installed(component.id)).map(|component| component.bytes).sum()).unwrap_or_default();
    let profile_files = target
        .as_ref()
        .filter(|selection| validate_complete_set(&selection.components).is_ok())
        .map(|selection| profile_files_from_components(&selection.components));
    ManagerStatus {
        engine_id: ENGINE_ID, model_root: root.display().to_string(), first_run: !ready, ready, download_pending,
        recommended_profile_id: recommended_profile().into(), active, profile_files,
        installed_components: component_statuses.iter().filter(|component| component.installed).map(|component| component.id.into()).collect(),
        components: component_statuses,
    }
}

/// A component counts as installed when its file is there.
///
/// It gets there by one route: downloaded to `.part`, hashed against the
/// pinned Hugging Face LFS oid, and only then renamed. So presence is the
/// proof, and the alternative - re-hashing eleven gigabytes on every status
/// poll - is not one.
fn published_component(path: &Path, component: &Component) -> bool {
    // a file of another size is another release under the same name
    fs::metadata(path).map(|metadata| metadata.is_file() && metadata.len() == component.bytes).unwrap_or(false)
}

async fn verified_file_async(path: PathBuf, component: Component) -> Result<bool> {
    tokio::task::spawn_blocking(move || verified_file(&path, &component))
        .await
        .context("join GGUF SHA-256 verification task")?
}

fn verified_file(path: &Path, component: &Component) -> Result<bool> {
    if fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0) == 0 {
        return Ok(false);
    }
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    let hex: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(hex == component.sha256)
}

fn validate_model_root(root: &Path) -> Result<()> {
    if root.as_os_str().is_empty() || root.parent().is_none() || root.file_name().is_none() {
        bail!("YUE_MODELS_ROOT must be a specific non-root directory");
    }
    Ok(())
}

fn part_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

/// The declared set whose components are exactly these, whatever order they
/// arrive in: picking every component of a set by hand is choosing that set.
pub fn profile_matching(component_ids: &[String]) -> Option<&'static str> {
    let mut wanted: Vec<&str> = component_ids.iter().map(String::as_str).chain([COMPANION]).collect();
    wanted.sort_unstable();
    wanted.dedup();
    profiles().into_iter().find_map(|profile| {
        let mut declared = profile.components.clone();
        declared.sort_unstable();
        (declared == wanted).then_some(profile.id)
    })
}

const PROFILE_SETS: [(&str, &str, [&str; 5]); 4] = [
    ("light", "Light - Q5_K_M backbone (6 GB cards)", ["backbone-q5", "vae-f32", "transcriber-q8", "mert-q8", COMPANION]),
    ("balanced", "Balanced - Q6_K backbone", ["backbone-q6", "vae-f32", "transcriber-q8", "mert-q8", COMPANION]),
    ("quality-q8", "Quality - Q8_0 backbone, near lossless", ["backbone-q8", "vae-f32", "transcriber-q8", "mert-q8", COMPANION]),
    ("native", "Full native - BF16 backbone, original weights", ["backbone-bf16", "vae-f32", "transcriber-f32", "mert-f32", COMPANION]),
];

fn profiles() -> Vec<Profile> {
    PROFILE_SETS.iter().map(|(id, label, ids)| profile(id, label, ids)).collect()
}

/// The bytes of a set's backbone and VAE, the weights that sit in memory while a song is made.
pub fn profile_weights_bytes(id: &str) -> u64 {
    let all = components();
    PROFILE_SETS
        .iter()
        .find(|(profile, _, _)| *profile == id)
        .map(|(_, _, ids)| {
            ids.iter()
                .filter_map(|id| all.iter().find(|component| component.id == *id))
                .filter(|component| matches!(component.kind, "backbone" | "vae"))
                .map(|component| component.bytes)
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(test)]
pub fn profile_exists(id: &str) -> bool {
    profiles().iter().any(|profile| profile.id == id && profile.installable && profile.backend == ENGINE_ID)
}

fn profile(id: &'static str, label: &'static str, ids: &[&'static str]) -> Profile {
    let all = components();
    // The badge follows the machine, the same answer the first-run banner gives.
    let recommended = id == recommended_profile();
    let ram_needed_gb = crate::hardware::ram_needed_gb(id, profile_weights_bytes(id), crate::hardware::hardware().total_vram_gb);
    Profile { id, label, backend: ENGINE_ID, installable: true, recommended, components: ids.to_vec(), total_bytes: ids.iter().filter_map(|id| all.iter().find(|component| component.id == *id)).map(|component| component.bytes).sum(), ram_needed_gb }
}

fn components() -> Vec<Component> {
    vec![
        c("backbone-bf16", "backbone", "YuE2-3B-BF16.gguf", 7166072352, "668ca9ffa4622449916aae5068b5737363e7ec186a824cd3d7a1a8a95269f166"),
        c("backbone-q8", "backbone", "YuE2-3B-Q8_0.gguf", 3810232064, "41121ce97786d7795a325bcf123ca196956c03bb252c9e75384cfc1f2fc19e6b"),
        c("backbone-q6", "backbone", "YuE2-3B-Q6_K.gguf", 2943331072, "42068d5e57713df11b9f6a5a3751072d9bbefe231d1d088f9c100e026ffa4ef9"),
        c("backbone-q5", "backbone", "YuE2-3B-Q5_K_M.gguf", 2622936832, "cd3efd250b734a229172800f08f33a893ad1f66b521666e176cdae4a0729b281"),
        // The VAE ships in F32 only: its weights are the audio.
        c("vae-f32", "vae", "YuE2-Vae-F32.gguf", 530497344, "93e49dfb1970e89ad64cacb17cf13b5d05f6bb30ef7ed3adae3050bcb728638a"),
        // SheetSage2's head; the MERT encoder it reads is a file of its own
        c("transcriber-f32", "transcriber", "SheetSage2-F32.gguf", 228738176, "4988c01b64c2812dd71eb89a9ab1937a3385ff2ba869bf495ccb69af4ec744cd"),
        c("transcriber-q8", "transcriber", "SheetSage2-Q8_0.gguf", 105629856, "95d8fa8761753b4833782a52b68d5c0afa3bd3cfbc03d3dcbbf8ded0c13f611e"),
        c("mert-f32", "transcriber-base", "MERT-v2-FullSong-F32.gguf", 2529780448, "695b247660405474fffa7bab4d019227325b8d169f96aaa96bad488033fec637"),
        c("mert-q8", "transcriber-base", "MERT-v2-FullSong-Q8_0.gguf", 902283712, "35a6032ec7e8123c02becdefc7bfe10a3adb33faa8a1951dc0af864b4494b8ef"),
        // Mothersuperior's pair of the v9 tokenizer head, CC BY-NC 4.0
        Component {
            id: COMPANION,
            kind: "companion",
            filename: "nar_lora_joint_v9.safetensors",
            bytes: 140560592,
            sha256: "585f303da1d5252d228d1e8ac6d4c4d11d970df9297406935cc8bdafa49cfa7e",
            repository: COMPANION_REPOSITORY,
            revision: "e2e63d859f3af879baf1b4d4e9f22d1eeda6fde5",
        },
    ]
}

/// The parts of a set the studio runs without: SheetSage2 and the MERT it reads.
fn optional_kind(kind: &str) -> bool {
    matches!(kind, "transcriber" | "transcriber-base")
}

/// Transcribers of the single-file release, read as the nearest one published now.
const RETIRED_TRANSCRIBERS: [(&str, &str); 2] = [("transcriber-q6", "transcriber-q8"), ("transcriber-q5", "transcriber-q8")];

/// Files of the single-file SheetSage2 release no transcriber reads any more,
/// with their size where the name lives on in the current release.
const RETIRED_FILES: [(&str, Option<u64>); 4] = [
    ("SheetSage2-Q6_K.gguf", None),
    ("SheetSage2-Q5_K_M.gguf", None),
    ("SheetSage2-F32.gguf", Some(2708176640)),
    ("SheetSage2-Q8_0.gguf", Some(957571488)),
];

/// Deletes what is left of the single-file transcriber: the engine reads none of it.
fn remove_retired_files(root: &Path) {
    for (name, size) in RETIRED_FILES {
        let path = root.join(name);
        let retired = fs::metadata(&path).is_ok_and(|meta| meta.is_file() && size.is_none_or(|bytes| meta.len() == bytes));
        if retired {
            if let Err(error) = fs::remove_file(&path) {
                eprintln!("[ERROR] could not remove the retired transcriber {}: {error}", path.display());
            }
        }
    }
}

/// The ggml type a GGUF file name ends in: `-Q8_0` of `MERT-v2-FullSong-Q8_0.gguf`.
fn precision(filename: &str) -> &str {
    let stem = filename.strip_suffix(".gguf").unwrap_or(filename);
    stem.rsplit_once('-').map_or(stem, |(_, tail)| tail)
}

fn c(id: &'static str, kind: &'static str, filename: &'static str, bytes: u64, sha256: &'static str) -> Component {
    Component { id, kind, filename, bytes, sha256, repository: REPOSITORY, revision: REVISION }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_picked_set_equal_to_a_declared_one_is_that_set() {
        let ids = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(profile_matching(&ids(&["transcriber-f32", "mert-f32", "backbone-bf16", "vae-f32"])), Some("native"));
        assert_eq!(profile_matching(&ids(&["transcriber-f32", "mert-f32", "backbone-bf16", "vae-f32", COMPANION])), Some("native"));
        assert_eq!(profile_matching(&ids(&["backbone-q8", "vae-f32", "transcriber-q8", "mert-q8"])), Some("quality-q8"));
        assert_eq!(profile_matching(&ids(&["backbone-q8", "vae-f32"])), None);
        assert_eq!(profile_matching(&ids(&["backbone-q8", "vae-f32", "transcriber-f32"])), None);
    }

    #[test]
    fn recommended_profile_is_a_complete_runnable_set() {
        let selected = resolve_install(InstallRequest { profile_id: Some(recommended_profile().into()), component_ids: vec![] }).unwrap();
        assert_eq!(selected.profile_id.as_deref(), Some(recommended_profile()));
        validate_complete_set(&selected.components).unwrap();
    }

    #[test]
    fn an_empty_request_downloads_nothing() {
        let error = resolve_install(InstallRequest { profile_id: None, component_ids: vec![] }).expect_err("an empty request is a mistake");
        assert!(error.to_string().contains("nothing was selected"));
    }

    #[test]
    fn a_set_needs_one_backbone_and_one_vae() {
        let ids = |list: &[&str]| InstallRequest { profile_id: None, component_ids: list.iter().map(|id| id.to_string()).collect() };
        assert!(resolve_install(ids(&["backbone-q8"])).is_err());
        assert!(resolve_install(ids(&["backbone-q8", "backbone-q6", "vae-f32"])).is_err());
        assert!(resolve_install(ids(&["backbone-q8", "vae-f32", "transcriber-q8", "transcriber-f32"])).is_err());
    }

    #[test]
    fn the_transcriber_is_optional() {
        let selection = resolve_install(InstallRequest { profile_id: None, component_ids: vec!["backbone-q6".into(), "vae-f32".into()] }).unwrap();
        let files = profile_files_from_components(&selection.components);
        assert_eq!(files.backbone, "YuE2-3B-Q6_K.gguf");
        assert_eq!(files.vae, "YuE2-Vae-F32.gguf");
        assert!(files.transcriber.is_none());
        assert_eq!(files.companion, "nar_lora_joint_v9.safetensors");
    }

    #[test]
    fn the_transcriber_brings_mert_of_its_precision_and_old_picks_land_on_q8() {
        let picked = |list: &[&str]| resolve_install(InstallRequest { profile_id: None, component_ids: list.iter().map(|id| id.to_string()).collect() }).unwrap();
        let ids = |selection: ResolvedInstall| selection.components.iter().map(|component| component.id).collect::<Vec<_>>();
        assert!(ids(picked(&["backbone-q8", "vae-f32", "transcriber-f32"])).contains(&"mert-f32"));
        let old = ids(picked(&["backbone-q5", "vae-f32", "transcriber-q5"]));
        assert!(old.contains(&"transcriber-q8") && old.contains(&"mert-q8"));
        assert!(validate_complete_set(&components().into_iter().filter(|component| ["backbone-q8", "vae-f32", COMPANION, "transcriber-q8", "mert-f32"].contains(&component.id)).collect::<Vec<_>>()).is_err());
        assert_eq!(precision("MERT-v2-FullSong-Q8_0.gguf"), precision("SheetSage2-Q8_0.gguf"));
    }

    #[test]
    fn a_missing_transcriber_leaves_the_set_runnable_without_one() {
        let root = std::env::temp_dir().join(format!("yue2-optional-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let manager = ModelManager {
            root: root.clone(),
            state_path: root.join("state.json"),
            http: reqwest::Client::new(),
            state: Arc::new(RwLock::new(PersistentState::default())),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        for component in components().iter().filter(|component| ["backbone-q8", "vae-f32", COMPANION].contains(&component.id)) {
            fs::File::create(root.join(component.filename)).unwrap().set_len(component.bytes).unwrap();
        }
        fs::write(root.join("SheetSage2-Q8_0.gguf"), b"the single-file release").unwrap();
        let files = manager.installed_profile_files("quality-q8").unwrap();
        assert!(files.transcriber.is_none());
        let status = status_snapshot(root.clone(), None, Some(InstallRequest { profile_id: Some("quality-q8".into()), component_ids: vec![] }));
        assert!(status.ready && status.download_pending > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_single_file_transcriber_is_cleared_and_a_head_goes_with_its_mert() {
        let root = std::env::temp_dir().join(format!("yue2-retired-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        fs::File::create(root.join("SheetSage2-Q8_0.gguf")).unwrap().set_len(957571488).unwrap();
        fs::write(root.join("SheetSage2-Q6_K.gguf"), b"old").unwrap();
        fs::File::create(root.join("SheetSage2-F32.gguf")).unwrap().set_len(228738176).unwrap();
        remove_retired_files(&root);
        assert!(!root.join("SheetSage2-Q8_0.gguf").exists() && !root.join("SheetSage2-Q6_K.gguf").exists());
        assert!(root.join("SheetSage2-F32.gguf").exists(), "the current head keeps its name and size");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn every_set_carries_the_companion() {
        let picked = resolve_install(InstallRequest { profile_id: None, component_ids: vec!["backbone-q6".into(), "vae-f32".into()] }).unwrap();
        assert!(picked.components.iter().any(|component| component.id == COMPANION));
        let named = resolve_install(InstallRequest { profile_id: None, component_ids: vec!["backbone-q6".into(), "vae-f32".into(), COMPANION.into()] }).unwrap();
        assert_eq!(named.components.len(), picked.components.len());
        assert!(profiles().iter().all(|profile| profile.components.contains(&COMPANION)));
    }

    #[test]
    fn a_picked_set_stays_chosen_while_only_the_companion_is_missing() {
        let root = std::env::temp_dir().join(format!("yue2-picked-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let manager = ModelManager {
            root: root.clone(),
            state_path: root.join("state.json"),
            http: reqwest::Client::new(),
            state: Arc::new(RwLock::new(PersistentState::default())),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let picked = vec!["backbone-q6".to_string(), "vae-f32".to_string()];
        assert!(!manager.picked_components_installed(&picked));
        let catalog = components();
        let size = |id: &str| catalog.iter().find(|component| component.id == id).unwrap().bytes;
        fs::write(root.join("YuE2-3B-Q6_K.gguf"), b"weights").unwrap();
        assert!(!manager.picked_components_installed(&picked), "a file of another size is not the published one");
        fs::File::create(root.join("YuE2-3B-Q6_K.gguf")).unwrap().set_len(size("backbone-q6")).unwrap();
        fs::File::create(root.join("YuE2-Vae-F32.gguf")).unwrap().set_len(size("vae-f32")).unwrap();
        assert!(manager.picked_components_installed(&picked));
        assert!(manager.installed_component_files(&picked).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn every_profile_resolves_to_named_files() {
        for profile in profiles() {
            let selection = resolve_install(InstallRequest { profile_id: Some(profile.id.into()), component_ids: vec![] }).unwrap();
            let files = profile_files_from_components(&selection.components);
            assert!(files.backbone.starts_with("YuE2-3B-"));
            assert!(files.transcriber.is_some());
        }
    }

    #[test]
    fn catalog_pins_every_published_file() {
        assert_eq!(components().len(), 10);
        assert!(components().iter().all(|component| component.sha256.len() == 64 && component.revision.len() == 40));
        assert!(components().iter().filter(|component| component.id != COMPANION).all(|component| component.revision == REVISION));
    }

    #[tokio::test]
    async fn file_hashing_does_not_block_the_async_runtime() {
        let path = std::env::temp_dir().join(format!("yue2-hash-test-{}", uuid::Uuid::now_v7()));
        fs::File::create(&path).unwrap().set_len(8 * 1024 * 1024).unwrap();
        let component = Component { id: "test", kind: "test", filename: "test", bytes: 8 * 1024 * 1024, sha256: "not-a-real-digest", repository: REPOSITORY, revision: REVISION };
        let hash_task = tokio::spawn(verified_file_async(path.clone(), component));
        tokio::time::timeout(std::time::Duration::from_secs(1), tokio::time::sleep(std::time::Duration::from_millis(1))).await.unwrap();
        assert!(!hash_task.await.unwrap().unwrap());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn startup_marks_orphaned_download_as_cancelled_without_erasing_resume_state() {
        let mut state = PersistentState { active: Some(DownloadJob { id: "job".into(), profile_id: Some("light".into()), component_ids: vec!["backbone-q5".into()], status: DownloadStatus::Downloading, downloaded_bytes: 123, total_bytes: 456, error: None }) };
        assert!(recover_interrupted_download(&mut state));
        let recovered = state.active.unwrap();
        assert!(matches!(recovered.status, DownloadStatus::Cancelled));
        assert_eq!(recovered.downloaded_bytes, 123);
    }
}
