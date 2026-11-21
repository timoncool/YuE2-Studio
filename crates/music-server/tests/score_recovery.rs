use axum::{
    Json, Router,
    extract::Query,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
async fn completed_score_is_available_by_its_original_id_after_studio_restarts() {
    let data = tempfile::tempdir().unwrap();
    let engine = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let engine_port = engine.local_addr().unwrap().port();
    let abc = "X:1\nT:Score kept through restart\nK:C\nC D E F|";
    let fixture = tokio::spawn(async move {
        let submissions = AtomicUsize::new(0);
        let router = Router::new()
            .route("/props", get(|| async { Json(json!({})) }))
            .route(
                "/synth",
                post(move || {
                    let id = match submissions.fetch_add(1, Ordering::Relaxed) {
                        0 => "original-score",
                        1 => "cancelled-score",
                        2 => "interrupted-score",
                        _ => unreachable!(),
                    };
                    async move { Json(json!({"id":id})) }
                }),
            )
            .route(
                "/job",
                get(
                    move |Query(params): Query<HashMap<String, String>>| async move {
                        if matches!(
                            params["id"].as_str(),
                            "interrupted-score" | "cancelled-score"
                        ) {
                            return Json(json!({"status":"running"}));
                        }
                        assert_eq!(params["id"], "original-score");
                        if params.get("result").is_some() {
                            Json(json!({"abc":abc}))
                        } else {
                            Json(json!({"status":"done"}))
                        }
                    },
                )
                .post(|Query(params): Query<HashMap<String, String>>| async move {
                    assert_eq!(params["id"], "cancelled-score");
                    assert_eq!(params["cancel"], "1");
                    Json(json!({"status":"cancelled"}))
                }),
            );
        axum::serve(engine, router).await.unwrap();
    });
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_music-server"));
    command
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_PORT", engine_port.to_string())
        .env("YUE_STUDIO_DATA_ROOT", data.path())
        .env("YUE_ENGINE_BIN", data.path().join("absent-engine"))
        .env("TIMONCOOL_SHARED_DIR", data.path().join("registry"))
        .kill_on_drop(true);
    let restart_ready = || async {
        timeout(Duration::from_secs(20), async {
            loop {
                if http
                    .get(format!("{url}/v1/configuration"))
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

    let mut running = command.spawn().unwrap();
    restart_ready().await;
    let submitted: Value = http
        .post(format!("{url}/v1/scores"))
        .json(&json!({"style":"piano", "lm_seed":42}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(submitted["id"], "original-score");
    let score_url = format!("{url}/v1/scores/original-score");
    let before = timeout(Duration::from_secs(15), async {
        loop {
            let result: Value = http
                .get(&score_url)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            if result["status"] != "running" {
                return result;
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
    let cancel_url = format!("{url}/v1/scores/cancelled-score");
    let to_cancel: Value = http
        .post(format!("{url}/v1/scores"))
        .json(&json!({"style":"piano"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(to_cancel["id"], "cancelled-score");
    let live_cancel: Value = http
        .post(cancel_url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(live_cancel["status"], "cancelled");

    let interrupted: Value = http
        .post(format!("{url}/v1/scores"))
        .json(&json!({"style":"piano","lm_seed":43}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        interrupted,
        json!({"id":"interrupted-score","status":"running"})
    );
    running.kill().await.unwrap();
    fixture.abort();
    let _ = fixture.await;

    let mut restarted = command.spawn().unwrap();
    restart_ready().await;
    let reopened: Value = http
        .get(&score_url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reopened, before);
    let cancelled: Value = http
        .post(&score_url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cancelled, before);
    let interrupted_url = format!("{url}/v1/scores/interrupted-score");
    let recovered: Value = http
        .get(&interrupted_url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(recovered["id"], "interrupted-score");
    assert_eq!(recovered["status"], "failed");
    assert!(
        recovered["error"]
            .as_str()
            .unwrap()
            .contains("Studio stopped")
    );
    let cancelled_interrupted: Value = http
        .post(interrupted_url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cancelled_interrupted, recovered);
    restarted.kill().await.unwrap();
}
