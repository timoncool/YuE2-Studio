use super::*;

#[derive(Deserialize)]
pub(crate) struct Requested {
    from_generation: String,
    to_generation: String,
}

async fn restore<P: Package>(state: &AppState, id: &str, request: Requested) -> Result<Selection> {
    ensure!(portable_name(id), "invalid package identifier");
    let data = update_data_root(state);
    let root = data.join(P::FOLDER).join(id);
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .open(root.join("install.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("an installation is already running")?;
    let current = read_selection(&root)?;
    ensure!(
        request.from_generation != request.to_generation,
        "identical generations"
    );
    if current.active == request.to_generation
        && current.previous.as_ref() == Some(&request.from_generation)
        && current.automatic_updates_paused
    {
        return Ok(current);
    }
    ensure!(
        current.active == request.from_generation
            && current.previous.as_ref() == Some(&request.to_generation),
        "the installed generations changed; refresh before rolling back"
    );
    // A local rollback can reuse only a retained generation with its original
    // signed proof and all of its executable, library or weight bytes intact.
    resolve_package::<P>(data, id, Some(&request.to_generation), Probe::Sha256).await?;
    let selection = Selection {
        active: request.to_generation,
        previous: Some(current.active),
        automatic_updates_paused: true,
    };
    let mut file = tempfile::NamedTempFile::new_in(&root)?;
    serde_json::to_writer(file.as_file_mut(), &selection)?;
    file.as_file().sync_all()?;
    let _activation = state.engine_use.try_write()
        .map_err(|_| anyhow::anyhow!("The local music engine is busy."))?;
    ensure!(!local_work_pending(state).await, "The local music engine is busy.");
    file.persist(root.join("active.json"))?;
    Ok(selection)
}

pub(crate) async fn apply(
    State(state): State<AppState>,
    RoutePath((kind, id)): RoutePath<(String, String)>,
    Json(request): Json<Requested>,
) -> Result<Json<Selection>, Failure> {
    let result = match kind.as_str() {
        EnginePackage::FOLDER => restore::<EnginePackage>(&state, &id, request).await,
        ModelPackage::FOLDER => restore::<ModelPackage>(&state, &id, request).await,
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "unknown package kind".into(),
            ));
        }
    };
    result
        .map(Json)
        .map_err(|e| api_error(StatusCode::CONFLICT, format!("{e:#}")))
}
