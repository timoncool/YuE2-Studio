//! Example adapter. It's built explicitly for packaging and isn't launched by
//! Studio unless it comes from a verified adapter package.
use std::{net::Ipv4Addr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine as _;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{ace, signed_music_protocol as protocol};

#[derive(Clone)]
struct Adapter {
    client: ace::AceClient,
    identity: Value,
    token: String,
    submitted: Arc<Mutex<Option<(String, Value)>>>,
}

struct Failure(StatusCode, String);
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

fn bad(message: impl Into<String>) -> Failure {
    Failure(StatusCode::BAD_REQUEST, message.into())
}

fn engine_failure(error: anyhow::Error) -> Failure {
    Failure(StatusCode::BAD_GATEWAY, error.to_string())
}

/// Starts the reference music-http-v1 adapter over a bundled native ace-server.
pub async fn run_ace_adapter() -> Result<()> {
    let models = PathBuf::from(
        std::env::var_os("YUE_MUSIC_ADAPTER_MODELS").context("missing verified model directory")?,
    );
    let token = std::env::var("YUE_MUSIC_ADAPTER_TOKEN")?;
    ensure!(!token.is_empty(), "missing adapter token");
    ensure!(
        std::env::var("YUE_MUSIC_ADAPTER_HOST")? == "127.0.0.1",
        "adapter must use loopback"
    );
    let port: u16 = std::env::var("YUE_MUSIC_ADAPTER_PORT")?.parse()?;
    let engine = std::env::var("YUE_MUSIC_ADAPTER_ID")?;
    let version = std::env::var("YUE_MUSIC_ADAPTER_VERSION")?;
    let path = std::env::current_exe()?
        .with_file_name(format!("ace-server{}", std::env::consts::EXE_SUFFIX));
    let cuda_dir = path.with_file_name(if cfg!(windows) {
        "ggml-cuda.dll"
    } else {
        "libggml-cuda.so"
    });
    let cuda = cuda_dir.is_file();
    let log_dir = tempfile::tempdir()?;
    let runtime = ace::Runtime::default();
    let client = runtime
        .start(ace::Launch {
            executable: path,
            models_root: models,
            adapters_root: None,
            device: Some(if cuda { "CUDA0" } else { "CPU" }.into()),
            cuda_backend: cuda.then_some(cuda_dir),
            log_path: log_dir.path().join("ace.log"),
        })
        .await?;
    let state = Adapter {
        client,
        identity: json!({"protocol":protocol::NAME,"engine_id":engine,"version":version,
                        "compute_backend":if cuda { "unknown" } else { "cpu" }}),
        token: format!("Bearer {token}"),
        submitted: Arc::default(),
    };
    let app = Router::new()
        .route(
            "/v1/health",
            get(|State(adapter): State<Adapter>| async move { Json(adapter.identity) }),
        )
        .route("/v1/jobs", post(submit))
        .route("/v1/jobs/{id}", get(status))
        .route("/v1/jobs/{id}/result", get(result))
        .route("/v1/jobs/{id}/cancel", post(cancel))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let (close, closed) = tokio::sync::oneshot::channel();
    // Studio closes this private pipe on both normal shutdown and process exit.
    // Give the engine RAII guard a chance to reap the bundled C++ child.
    std::thread::spawn(move || {
        use std::io::Read;
        let _ = std::io::copy(
            &mut std::io::stdin().lock().take(u64::MAX),
            &mut std::io::sink(),
        );
        let _ = close.send(());
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = closed.await;
        })
        .await?;
    runtime.stop().await?;
    Ok(())
}

async fn authenticate(State(adapter): State<Adapter>, request: Request, next: Next) -> Response {
    if request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some(adapter.token.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

fn engine_request(submission: protocol::Submission) -> Result<Value, Failure> {
    if submission.model.trim().is_empty() {
        return Err(bad(
            "Provide the model ID for the verified model directory.",
        ));
    }
    let source = submission.payload;
    match submission.mode {
        protocol::Mode::Create => Ok(json!({
            "task_type": "text2music", "caption": source["style"], "lyrics": source["lyrics"],
            "duration": source["duration_seconds"].as_f64().unwrap_or(10.0),
            "inference_steps": source["steps"].as_u64().unwrap_or(8),
            "guidance_scale": source["cfg_scale"].as_f64().unwrap_or(1.0),
            "seed": source["seed"].as_i64().filter(|seed| *seed >= 0).unwrap_or(42),
            "solver":"euler", "output_format":"wav16", "synth_batch_size":1,
        })),
        protocol::Mode::Replay => {
            let mut saved = source["saved"]
                .as_object()
                .cloned()
                .ok_or_else(|| bad("Replay needs a saved ACE request."))?;
            for (input, engine) in [("seed", "seed"), ("steps", "inference_steps")] {
                if let Some(value) = source["overrides"]
                    .get(input)
                    .filter(|value| !value.is_null())
                {
                    saved.insert(engine.into(), value.clone());
                }
            }
            if !source["overrides"]["solver"].is_null() {
                return Err(bad("This example adapter uses the Euler solver."));
            }
            Ok(Value::Object(saved))
        }
    }
}

async fn submit(
    State(adapter): State<Adapter>,
    Json(submission): Json<protocol::Submission>,
) -> Result<impl IntoResponse, Failure> {
    let request = engine_request(submission)?;
    let mut jobs = adapter.submitted.lock().await;
    if jobs.is_some() {
        return Err(Failure(
            StatusCode::CONFLICT,
            "The example adapter serves one job per process.".into(),
        ));
    }
    let id = adapter
        .client
        .submit_synth(std::slice::from_ref(&request), ace::Sources::default())
        .await
        .map_err(engine_failure)?;
    *jobs = Some((id.clone(), request));
    Ok((StatusCode::ACCEPTED, Json(json!({"id":id}))))
}

async fn lookup(adapter: &Adapter, id: &str) -> Result<Value, Failure> {
    adapter
        .submitted
        .lock()
        .await
        .as_ref()
        .filter(|(job, _)| job == id)
        .map(|(_, request)| request.clone())
        .ok_or_else(|| Failure(StatusCode::NOT_FOUND, "Job not found.".into()))
}

fn progress(status: String) -> Json<Value> {
    let status = match status.as_str() {
        "queued" => "queued",
        "running" => "running",
        "done" => "completed",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    Json(json!({"status": status}))
}

async fn status(
    State(adapter): State<Adapter>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Failure> {
    lookup(&adapter, &id).await?;
    Ok(progress(
        adapter.client.status(&id).await.map_err(engine_failure)?,
    ))
}

async fn cancel(
    State(adapter): State<Adapter>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Failure> {
    lookup(&adapter, &id).await?;
    Ok(progress(
        adapter
            .client
            .cancel_status(&id)
            .await
            .map_err(engine_failure)?,
    ))
}

async fn result(
    State(adapter): State<Adapter>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Failure> {
    let request = lookup(&adapter, &id).await?;
    let (content_type, body) = adapter.client.result(&id).await.map_err(engine_failure)?;
    let mut takes = ace::takes(&[request], &content_type, &body).map_err(engine_failure)?;
    if takes.len() != 1 {
        return Err(Failure(
            StatusCode::BAD_GATEWAY,
            "Expected one WAV from ACE-Step.".into(),
        ));
    }
    let take = takes.pop().expect("exactly one take");
    let wav = base64::engine::general_purpose::STANDARD.encode(take.audio);
    Ok(Json(
        json!({"audio_base64":wav, "replay":take.replay, "settings":take.replay}),
    ))
}
