use super::*;
use std::time::Duration;

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Mode {
    #[default]
    Manual,
    Notify,
    Auto,
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Policy {
    mode: Mode,
}

static MODE_CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();

fn policy(root: &Path) -> Result<Policy> {
    match std::fs::read(root.join("music-package-updates.json")) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Policy::default()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Serialize)]
struct Update {
    kind: &'static str,
    id: String,
    from_version: String,
    to_version: String,
    changed_bytes: u64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    automatic_updates_paused: bool,
}

async fn pending<P: Package>(
    root: &Path,
    candidate: &P,
    version: impl Fn(&P) -> &str,
) -> Result<Option<Update>> {
    let dir = root.join(P::FOLDER).join(candidate.id());
    let selection = read_selection(&dir)?;
    let active = selection.active;
    if active.is_empty() {
        return Ok(None);
    }
    let proof = channel::verify_engine_proof(&dir.join(active).join("proof")).await?;
    let old = P::select(parse(&proof)?, candidate.id())?;
    let mut changed_bytes = 0u64;
    for file in candidate.files() {
        if !old.files().iter().any(|previous| {
            previous.name == file.name
                && previous.sha256 == file.sha256
                && previous.bytes == file.bytes
        }) {
            changed_bytes = changed_bytes
                .checked_add(file.bytes)
                .context("package size overflow")?;
        }
    }
    if serde_json::to_vec(&old)? == serde_json::to_vec(candidate)? {
        return Ok(None);
    }
    Ok(Some(Update {
        kind: P::FOLDER,
        id: candidate.id().into(),
        from_version: version(&old).into(),
        to_version: version(candidate).into(),
        changed_bytes,
        automatic_updates_paused: selection.automatic_updates_paused,
    }))
}

#[derive(Default)]
struct Plan {
    updates: Vec<Update>,
    errors: Vec<serde_json::Value>,
}

impl Plan {
    async fn inspect<P: Package>(
        &mut self,
        root: &Path,
        candidate: &P,
        version: impl Fn(&P) -> &str,
    ) {
        match pending(root, candidate, version).await {
            Ok(Some(update)) => self.updates.push(update),
            Ok(None) => (),
            Err(error) => self.errors.push(serde_json::json!({
                "kind": P::FOLDER, "id": candidate.id(), "error": format!("{error:#}")
            })),
        }
    }
}

async fn plan(root: &Path) -> Result<Plan> {
    let mut updates = Plan::default();
    let Some(bytes) = channel::cached_catalogue(channel::ENGINES).await? else {
        return Ok(updates);
    };
    let catalogue = parse(&bytes)?;
    let platform = host_platform();
    let engines: Vec<_> = catalogue
        .engines
        .iter()
        .filter(|engine| engine.platform == platform && super::supported_protocol(&engine.protocol))
        .collect();
    for engine in &engines {
        updates.inspect(root, *engine, |p| &p.version).await;
    }
    for model in &catalogue.models {
        if model
            .engines
            .iter()
            .any(|id| engines.iter().any(|engine| &engine.id == id))
        {
            updates.inspect(root, model, |p| &p.version).await;
        }
    }
    Ok(updates)
}

fn failure(error: anyhow::Error) -> Failure {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}"))
}

pub(crate) async fn list(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, Failure> {
    let root = update_data_root(&state);
    let plan = plan(root).await.map_err(failure)?;
    Ok(Json(
        serde_json::json!({"mode":policy(root).map_err(failure)?.mode,
        "updates":plan.updates, "errors":plan.errors}),
    ))
}

pub(crate) async fn set_mode(
    State(state): State<AppState>,
    Json(next): Json<Policy>,
) -> Result<Json<Policy>, Failure> {
    let root = update_data_root(&state);
    let save = || -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        serde_json::to_writer(file.as_file_mut(), &next)?;
        file.as_file().sync_all()?;
        file.persist(root.join("music-package-updates.json"))?;
        Ok(())
    };
    save().map_err(failure)?;
    super::super::note_updates_changed();
    MODE_CHANGED.notify_one();
    Ok(Json(next))
}

async fn check(state: &AppState) -> Result<()> {
    let root = update_data_root(state);
    let mode = policy(root)?.mode;
    if mode == Mode::Manual {
        return Ok(());
    }
    super::super::refresh()
        .await
        .map_err(|(_, body)| anyhow::anyhow!(body.error.clone()))?;
    if mode == Mode::Auto {
        for change in plan(root).await?.updates {
            if policy(root)?.mode != Mode::Auto {
                break;
            }
            if change.automatic_updates_paused {
                continue;
            }
            let result = if change.kind == EnginePackage::FOLDER {
                update_engine(state, &change.id, true).await
            } else {
                update_model(state, &change.id, true).await
            };
            if let Err((_, error)) = result {
                eprintln!("component update failed: {}", error.error);
            }
        }
    }
    Ok(())
}

pub(crate) async fn watch(state: AppState) {
    loop {
        tokio::select! {
            biased;
            _ = MODE_CHANGED.notified() => continue,
            result = check(&state) => {
                if let Err(error) = result { eprintln!("component update check: {error:#}"); }
            }
        }
        tokio::select! {
            _ = MODE_CHANGED.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(24 * 3600)) => {},
        }
    }
}
