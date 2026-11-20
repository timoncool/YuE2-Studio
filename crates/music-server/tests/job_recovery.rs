use axum::{Json, Router, routing::get};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    net::TcpListener,
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
async fn reopening_with_a_cloud_provider_keeps_the_original_request_without_a_phantom_retry() {
    let data = tempfile::tempdir().unwrap();
    let engine = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let engine_port = engine.local_addr().unwrap().port();
    tokio::spawn(async move {
        let routes = Router::new().route("/props", get(|| async { Json(json!({})) }));
        axum::serve(engine, routes).await.unwrap();
    });

    let socket = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
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
    let mut studio = command.spawn().unwrap();
    let cfg_url = format!("{url}/v1/configuration");
    let mut config: Value = timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(response) = http.get(&cfg_url).send().await {
                break response.error_for_status().unwrap().json().await.unwrap();
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let music = config["selections"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|selection| selection["capability"] == "music_generation")
        .unwrap();
    music["mode"] = json!("open_router");
    music["cloud_model"] = json!("qa/model");
    music["local_engine"] = Value::Null;
    http.put(cfg_url)
        .json(&config)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    studio.kill().await.unwrap();

    let db = Connection::open(data.path().join("library.sqlite")).unwrap();
    db.execute("INSERT INTO music_jobs(id,submitted_at,engine_id,title,style,lyrics,duration_seconds,generation_settings_json,request_json,status)
        VALUES('interrupted',1,'yue2-cpp','Draft','piano','',5,'{}','{\"style\":\"piano\"}','running')", []).unwrap();
    let mut studio = command.spawn().unwrap();
    let (replacement, message, request): (Option<String>, String, String) = timeout(Duration::from_secs(20), async {
        loop {
            let (status, replacement, message, request) = db.query_row(
                "SELECT status,resumed_as,message,request_json FROM music_jobs WHERE id='interrupted'", [],
                |row| Ok((row.get::<_,String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).unwrap();
            if status == "cancelled" { break (replacement, message, request); }
            sleep(Duration::from_millis(50)).await;
        }
    }).await.unwrap();
    studio.kill().await.unwrap();
    assert!(replacement.is_none(), "{message}");
    assert!(
        message.contains("yue2-cpp") && message.contains("openrouter"),
        "{message}"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&request).unwrap(),
        json!({"style":"piano"})
    );
}
