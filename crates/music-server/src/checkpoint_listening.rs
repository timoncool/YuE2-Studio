use super::*;
use serde_json::json;

const AUDITION_SEED: i64 = 24917;
const SCORE: &str = "X:1\nT:Checkpoint audition\nM:4/4\nL:1/4\nQ:1/4=96\nK:C\nV:melody\n|: C D E G | A G E D | C E G A | G2 E2 :|";

/// Render every saved YuE2 checkpoint on one short score. The playlist and the
/// per-step client references survive a restart and a partially submitted batch.
pub(super) async fn listen(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let _guard = state.checkpoint_listening.lock().await;
    let run = state.training.run(&id).map_err(training_error)?;
    if run.engine != PRIMARY_MUSIC_ENGINE_ID {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "Checkpoint comparison requires a YuE2 LoRA training run".into(),
        ));
    }
    let mut checkpoints: Vec<_> = state
        .training
        .checkpoints(&id)
        .into_iter()
        .map(|item| item.step)
        .collect();
    checkpoints.sort_unstable();
    checkpoints.dedup();
    if checkpoints.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "This training run has no saved checkpoints".into(),
        ));
    }
    let playlist = format!("checkpoint-listen-v1-{id}");
    state
        .library
        .ensure_checkpoint_playlist(&playlist, &run.name)
        .map_err(training_error)?;
    let mut jobs = Vec::new();
    let installed = state.adapters.installed(None);
    for step in std::iter::once(None).chain(checkpoints.into_iter().map(Some)) {
        let label = step
            .map(|n| format!("Step {n}"))
            .unwrap_or_else(|| "Base".into());
        let client_ref = format!("{playlist}/{label}");
        if let Some(job) = state
            .library
            .checkpoint_listening_job(&playlist, &client_ref)
            .map_err(training_error)?
        {
            jobs.push(json!({"step":step,"job_id":job,"reused":true}));
            continue;
        }
        let mut request = CreateMusicJobRequest {
            music_source: Some(MusicSource::Local), client_ref: Some(client_ref), title: Some(label),
            playlist_id: Some(playlist.clone()), style: adapters::upstream_style("warm acoustic pop, piano, clear vocal, 96 BPM", &run.trigger),
            lyrics: "[Verse]\nWe follow the river\nWe sing through the night\n[Chorus]\nCarry the melody\nInto the light".into(),
            abc: Some(SCORE.into()), lm_seed: Some(AUDITION_SEED), seed: Some(AUDITION_SEED),
            duration_seconds: Some(35.0), lm_batch_size: Some(1), synth_batch_size: Some(1),
            ..Default::default()
        };
        if let Some(step) = step {
            let existing = installed.iter().find(|item| matches!(&item.meta.origin, adapters::Origin::Trained {run, step: n, ..} if run == &id && *n == step));
            let adapter = match existing {
                Some(item) => item.meta.id.clone(),
                None => {
                    install_training_checkpoint(
                        State(state.clone()),
                        Path((id.clone(), step)),
                        Json(InstallCheckpoint { name: None }),
                    )
                    .await?
                    .0
                    .id
                }
            };
            request.adapters.push(AdapterUse {
                id: adapter,
                ..Default::default()
            });
        }
        let (status, Json(job)) = submit_music_job(state.clone(), request, 0, None).await;
        if !status.is_success() || matches!(job.status, MusicJobStatus::Failed) {
            return Err(api_error(
                if status.is_success() {
                    StatusCode::CONFLICT
                } else {
                    status
                },
                job.message,
            ));
        }
        jobs.push(json!({"step":step,"job_id":job.id,"reused":false}));
    }
    Ok(Json(
        json!({"playlist_id":playlist,"seed":AUDITION_SEED,"jobs":jobs}),
    ))
}
