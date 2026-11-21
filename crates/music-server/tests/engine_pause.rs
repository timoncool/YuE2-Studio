use axum::{
    Json, Router,
    routing::{get, post},
};
use reqwest::{
    StatusCode,
    header::RANGE,
    multipart::{Form, Part},
};
use serde_json::{Value, json};
use std::{
    io::Cursor,
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
async fn active_generation_refuses_pause_while_saved_audio_remains_playable() {
    let data = tempfile::tempdir().unwrap();
    let engine = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let engine_port = engine.local_addr().unwrap().port();
    let submitted = Arc::new(AtomicUsize::new(0));
    let count = submitted.clone();
    let fixture = tokio::spawn(async move {
        let routes = Router::new()
            .route("/props", get(|| async { Json(json!({})) }))
            .route(
                "/synth",
                post(move || async move {
                    count.fetch_add(1, Ordering::Relaxed);
                    Json(json!({"id":"working"}))
                }),
            )
            .route(
                "/job",
                get(|| async { Json(json!({"status":"running"})) })
                    .post(|| async { Json(json!({"status":"cancelled"})) }),
            );
        axum::serve(engine, routes).await.unwrap();
    });
    let socket = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let url = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut studio = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_PORT", engine_port.to_string())
        .env("YUE_STUDIO_DATA_ROOT", data.path())
        .env("YUE_ENGINE_BIN", data.path().join("absent-engine"))
        .env("TIMONCOOL_SHARED_DIR", data.path().join("registry"))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    timeout(Duration::from_secs(20), async {
        loop {
            if http
                .get(format!("{url}/v1/configuration"))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();

    let mut wav = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(&mut wav, spec).unwrap();
    for index in 0..1600 {
        writer.write_sample::<i16>(index as i16 - 800).unwrap();
    }
    writer.finalize().unwrap();
    let stored: Value = http
        .post(format!("{url}/v1/library/import"))
        .multipart(Form::new().part(
            "audio",
            Part::bytes(wav.into_inner()).file_name("existing.wav"),
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = stored["id"].as_str().unwrap();

    let request = json!({"style":"piano","duration_seconds":10,"music_source":{"kind":"local"}});
    let jobs = format!("{url}/v1/music/jobs");
    assert_eq!(
        http.post(&jobs)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let pause = format!("{url}/engine/pause");
    assert_eq!(
        http.post(&pause).send().await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(submitted.load(Ordering::Relaxed), 1);
    http.post(format!("{jobs}/working"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    http.post(&pause)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    for _ in 0..3 {
        let status: Value = http
            .get(format!("{url}/setup/status"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status["engine_paused"], true);
        assert_eq!(status["engine_ready"], false);
    }
    assert_eq!(
        http.post(&jobs)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(submitted.load(Ordering::Relaxed), 1);
    let media = http
        .get(format!("{url}/v1/library/media/{id}"))
        .header(RANGE, "bytes=0-15")
        .send()
        .await
        .unwrap();
    assert_eq!(media.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(&media.bytes().await.unwrap()[..4], b"RIFF");
    assert_eq!(submitted.load(Ordering::Relaxed), 1);
    studio.kill().await.unwrap();
    fixture.abort();
}
