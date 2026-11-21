use super::*;

async fn inspect<P: Package>(
    root: &Path,
    id: &str,
    describe: &impl Fn(P) -> (String, String),
) -> Result<Option<serde_json::Value>> {
    let dir = root.join(P::FOLDER).join(id);
    let selected = read_selection(&dir)?;
    if selected.active.is_empty() {
        return Ok(None);
    }
    let proof = channel::verify_engine_proof(&dir.join(&selected.active).join("proof")).await?;
    let (name, version) = describe(P::select(parse(&proof)?, id)?);
    Ok(Some(
        serde_json::json!({"kind":P::FOLDER,"id":id,"name":name,"version":version,
        "generation":selected.active,"previous_generation":selected.previous,
        "automatic_updates_paused":selected.automatic_updates_paused}),
    ))
}

async fn collect<P: Package>(
    root: &Path,
    packages: &mut Vec<serde_json::Value>,
    errors: &mut Vec<serde_json::Value>,
    describe: impl Fn(P) -> (String, String),
) -> Result<()> {
    let mut entries = match tokio::fs::read_dir(root.join(P::FOLDER)).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let Ok(id) = entry.file_name().into_string() else {
            continue;
        };
        if !portable_name(&id) || !entry.file_type().await?.is_dir() {
            continue;
        }
        match inspect::<P>(root, &id, &describe).await {
            Ok(Some(package)) => packages.push(package),
            Ok(None) => (),
            Err(error) => errors
                .push(serde_json::json!({"kind":P::FOLDER,"id":id,"error":format!("{error:#}")})),
        }
    }
    Ok(())
}

pub(crate) async fn list(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, Failure> {
    let root = update_data_root(&state);
    let mut packages = Vec::new();
    let mut errors = Vec::new();
    let result = async {
        collect::<EnginePackage>(root, &mut packages, &mut errors, |p| (p.name, p.version)).await?;
        collect::<ModelPackage>(root, &mut packages, &mut errors, |p| (p.name, p.version)).await
    }
    .await;
    result.map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
    packages.sort_by(|a, b| {
        (a["kind"].as_str(), a["id"].as_str()).cmp(&(b["kind"].as_str(), b["id"].as_str()))
    });
    Ok(Json(
        serde_json::json!({"packages":packages,"errors":errors}),
    ))
}
