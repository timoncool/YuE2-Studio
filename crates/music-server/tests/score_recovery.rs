use axum::{
    Json, Router,
    extract::Query,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
async fn score_ids_survive_studio_restart_with_a_live_or_missing_worker_job() {
    let data = tempfile::tempdir().unwrap();
    let engine = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let engine_port = engine.local_addr().unwrap().port();
    let abc = "X:1\nT:Score kept through restart\nK:C\nC D E F|";
    let stage = Arc::new(AtomicUsize::new(0));
    let engine_stage = stage.clone();
    let pending = [
        "interrupted-score",
        "upstream-score",
        "still-running-score",
        "stalled-score",
        "offline-score",
    ];
    let fixture = tokio::spawn(async move {
        let submissions = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .route("/props", get(|| async { Json(json!({})) }))
            .route(
                "/synth",
                post(move || {
                    let id = match submissions.fetch_add(1, Ordering::Relaxed) {
                        0 => "original-score",
                        1 => "cancelled-score",
                        index => pending[index - 2],
                    };
                    async move { Json(json!({"id":id})) }
                }),
            )
            .route(
                "/job",
                get(move |Query(params): Query<HashMap<String, String>>| {
                    let stage = engine_stage.load(Ordering::Acquire);
                    async move {
                        let id = params["id"].as_str();
                        if id == "unknown-score" || (id == "interrupted-score" && stage > 0) {
                            return StatusCode::NOT_FOUND.into_response();
                        }
                        if id == "offline-score" && stage == 3 {
                            return StatusCode::SERVICE_UNAVAILABLE.into_response();
                        }
                        if id == "stalled-score" && stage == 1 {
                            sleep(Duration::from_secs(15)).await;
                        }
                        if id == "original-score"
                            || (id == "upstream-score" && stage > 0)
                            || (matches!(id, "stalled-score" | "still-running-score") && stage > 1)
                        {
                            if params.contains_key("result") {
                                Json(json!({"abc":abc})).into_response()
                            } else {
                                Json(json!({"status":"done"})).into_response()
                            }
                        } else {
                            Json(json!({"status":"running"})).into_response()
                        }
                    }
                })
                .post(
                    |Query(params): Query<HashMap<String, String>>| async move {
                        assert_eq!(params["id"], "cancelled-score");
                        assert_eq!(params["cancel"], "1");
                        Json(json!({"status":"cancelled"}))
                    },
                ),
            );
        axum::serve(engine, router).await.unwrap();
    });
    let socket = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_music-server"));
    command
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_PORT", engine_port.to_string())
        .env("YUE_STUDIO_DATA_ROOT", data.path())
        .env("YUE_ENGINE_BIN", data.path().join("absent-engine"))
        .env("TIMONCOOL_SHARED_DIR", data.path().join("registry"))
        .kill_on_drop(true);
    let ready = || async {
        timeout(Duration::from_secs(20), async {
            loop {
                if http
                    .get(format!("{base}/v1/configuration"))
                    .send()
                    .await
                    .is_ok()
                {
                    return;
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
    };
    let submit = || async {
        read_json(
            http.post(format!("{base}/v1/scores"))
                .json(&json!({"style":"piano","lm_seed":42})),
        )
        .await
    };
    let score = |id: &str| read_json(http.get(format!("{base}/v1/scores/{id}")));
    let cancel = |id: &str| read_json(http.post(format!("{base}/v1/scores/{id}")));

    let mut studio = command.spawn().unwrap();
    ready().await;
    assert_eq!(submit().await["id"], "original-score");
    let before = timeout(Duration::from_secs(15), async {
        loop {
            let result = score("original-score").await;
            if result["status"] != "running" {
                break result;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        before,
        json!({"id":"original-score","status":"done","abc":abc})
    );
    assert_eq!(submit().await["id"], "cancelled-score");
    assert_eq!(cancel("cancelled-score").await["status"], "cancelled");
    for id in pending {
        assert_eq!(submit().await, json!({"id":id,"status":"running"}));
    }
    studio.kill().await.unwrap();

    stage.store(1, Ordering::Release);
    let mut restarted = command.spawn().unwrap();
    ready().await;
    assert_eq!(score("original-score").await, before);
    assert_eq!(cancel("original-score").await, before);
    assert_eq!(
        score("upstream-score").await,
        json!({"id":"upstream-score","status":"done","abc":abc})
    );
    assert_eq!(score("still-running-score").await["status"], "running");
    for id in ["interrupted-score", "stalled-score"] {
        let interrupted = score(id).await;
        assert_eq!(interrupted["id"], id);
        assert_eq!(interrupted["status"], "failed");
        assert!(
            interrupted["error"]
                .as_str()
                .unwrap()
                .contains("engine did not return it")
        );
    }
    assert_eq!(
        cancel("interrupted-score").await,
        score("interrupted-score").await
    );
    let unknown = http
        .get(format!("{base}/v1/scores/unknown-score"))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    // A transient timeout must not overwrite a result the worker returns later.
    stage.store(2, Ordering::Release);
    assert_eq!(score("stalled-score").await["status"], "done");
    assert_eq!(score("still-running-score").await["status"], "done");
    stage.store(3, Ordering::Release);
    assert_eq!(score("offline-score").await["status"], "failed");
    fixture.abort();
    let _ = fixture.await;
    assert_eq!(score("upstream-score").await["status"], "done");
    restarted.kill().await.unwrap();
}

async fn read_json(request: reqwest::RequestBuilder) -> Value {
    request
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}
