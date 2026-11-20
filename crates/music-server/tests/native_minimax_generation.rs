use std::{fs, io::Cursor, path::Path, time::Duration};

use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    process::Command,
    time::{sleep, timeout},
};

const ENGINE: &str = "minimaxmusic-cpp";
const SAMPLE_RATE: u32 = 44_100;
const FRAMES: u32 = 512;
const MODELS: [(&str, u64); 5] = [
    ("mm3-lm-Q3_K_M.gguf", 4589971808),
    ("mm3-depth-Q4_K_M.gguf", 404836256),
    ("MiniMax-Music3-condition_encoder-F32.gguf", 100672192),
    ("mm3-dit-Q3_K_M.gguf", 1138427712),
    ("MiniMax-Music3-vocoder-F32.gguf", 306102784),
];

#[tokio::test]
async fn native_minimax_generation_imports_explicit_wav_with_fades_and_cancels() {
    exercise_generation(Some("wav32")).await;
}

#[tokio::test]
async fn native_minimax_generation_defaults_to_flac() {
    exercise_generation(None).await;
}

#[tokio::test]
async fn native_minimax_generation_records_the_encoded_mp3_bitrate() {
    exercise_generation(Some("mp3")).await;
}

async fn exercise_generation(format: Option<&str>) {
    let root = tempfile::tempdir().unwrap();
    let engine = build_engine(root.path()).await;
    let models = root.path().join("weights");
    fs::create_dir(&models).unwrap();
    for (file, length) in MODELS {
        let path = models.join(file);
        let file = fs::File::create(&path).unwrap();
        // The service inspects lengths when discovering an existing installation.
        // The fixture never reads model contents or allocates weight buffers.
        #[cfg(windows)]
        {
            let output = Command::new("fsutil")
                .args(["sparse", "setflag"])
                .arg(&path)
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "creating a sparse fixture: {:?}",
                output
            );
        }
        file.set_len(length).unwrap();
    }
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let (port, engine_port) = (free_port().await, free_port().await);
    let url = format!("http://127.0.0.1:{port}");
    let http = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let mut studio = Command::new(env!("CARGO_BIN_EXE_music-server"));
    studio
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_PORT", engine_port.to_string())
        .env("YUE_STUDIO_DATA_ROOT", &data)
        .env("YUE_ENGINE_BIN", root.path().join("absent-yue-server"))
        .env("YUE_MINIMAX_MM_SERVER_BIN", &engine)
        .env("YUE_MINIMAX_MODELS_ROOT", &models)
        .env("TIMONCOOL_SHARED_DIR", root.path().join("registry"))
        .env("DO_NOT_TRACK", "1")
        .env("STUDIO_HUB_URL", format!("{url}/fixture-hub"))
        .kill_on_drop(true);
    let mut studio = studio.spawn().unwrap();

    let profile_url = format!("{url}/v1/music/native/{ENGINE}/profiles");
    let catalog = timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(response) = http.get(&profile_url).send().await {
                break response
                    .error_for_status()
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap();
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(catalog["engine"], ENGINE);
    assert_eq!(catalog["engine_installed"], true);
    let entries = catalog["profiles"].as_array().unwrap();
    assert_eq!(entries[0]["id"], "minimal");
    assert_eq!(entries[0]["ready"], true);
    assert_eq!(entries[0]["missing"], json!([]));
    assert_eq!(entries[1]["ready"], false);
    assert!(!root.path().join("submitted-1.json").exists());

    let rpc_url = format!("{url}/mcp");
    let initial = http
        .post(&rpc_url)
        .header("Accept", "application/json, text/event-stream")
        .json(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-03-26","capabilities":{},
                "clientInfo":{"name":"native-minimax-test","version":"1"},
            }}),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let session = initial.headers().get("Mcp-Session-Id").cloned();
    let mcp_request = || {
        let request = http
            .post(&rpc_url)
            .header("Accept", "application/json, text/event-stream");
        match &session {
            Some(session) => request.header("Mcp-Session-Id", session.clone()),
            None => request,
        }
    };
    mcp_request()
        .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let tools = rpc(mcp_request(), 2, "minimax_music_profiles", json!({})).await;
    assert_eq!(tools["engine"], ENGINE);
    assert_eq!(tools["profiles"][0]["id"], "minimal");

    let jobs = format!("{url}/v1/music/jobs");
    let mut request = json!({"music_source":{"kind":"native","engine":ENGINE,"model":"minimal"},
        "style":"Folk with close acoustic guitar","lyrics":"[verse]\nEarly morning",
        "title":"MiniMax fixture","duration_seconds":3.2,"seed":42,"steps":2});
    if let Some(format) = format {
        request["output_format"] = json!(format);
    }
    if format == Some("wav32") {
        request["fade_in"] = json!(0.001);
        request["fade_out"] = json!(0.002);
    }
    if format == Some("mp3") {
        request["mp3_bitrate"] = json!(161);
    }
    let accepted = rpc(mcp_request(), 3, "song_create", request.clone()).await;
    let id = accepted["id"].as_str().unwrap();
    let job_url = format!("{jobs}/{id}");
    let job = wait_status(&http, &job_url, "completed").await;
    let hub_path = data.join("studio-hub.json");
    wait_for_completed_song_counts(&hub_path, 1).await;
    for _ in 0..3 {
        let polled = json_response(http.get(&job_url)).await;
        assert_eq!(polled["status"], "completed");
    }
    assert_completed_song_counts(&hub_path, 1);
    assert_eq!(job["engine_id"], ENGINE);
    assert_eq!(job["songs"].as_array().unwrap().len(), 1);
    let song = &job["songs"][0]["song"];
    assert_eq!(song["engine_id"], ENGINE);
    assert_eq!(song["profile_id"], "minimal");
    assert_eq!(
        song["generation_settings"]["music_source"],
        request["music_source"]
    );
    assert_eq!(song["replay_request"]["audio_codes"], "1,2,3");
    let audio = job["songs"][0]["audio_url"].as_str().unwrap();
    let audio = http
        .get(format!("{url}{audio}"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let effective = format.unwrap_or("flac");
    assert_eq!(song["generation_settings"]["output_format"], effective);
    assert_eq!(song["replay_request"]["output_format"], effective);
    assert_eq!(song["metadata"]["output_format"], effective);
    match effective {
        "wav32" => {
            assert!(song["audio_path"].as_str().unwrap().ends_with(".wav"));
            let mut reader = hound::WavReader::new(Cursor::new(audio)).unwrap();
            assert_eq!(reader.spec().sample_rate, SAMPLE_RATE);
            assert_eq!(reader.spec().channels, 2);
            assert_eq!(reader.spec().sample_format, hound::SampleFormat::Float);
            assert_eq!(reader.duration(), FRAMES);
            let pcm: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
            assert_eq!(&pcm[..2], &[0.0, 0.0]);
            assert_eq!(&pcm[256..258], &[0.125, -0.125]);
            assert_eq!(&pcm[pcm.len() - 2..], &[0.0, 0.0]);
            for record in [&song["generation_settings"], &song["replay_request"]] {
                assert!(
                    (record["fade_in"].as_f64().unwrap() - 44.0 / f64::from(SAMPLE_RATE)).abs()
                        < 1e-9
                );
                assert!(
                    (record["fade_out"].as_f64().unwrap() - 88.0 / f64::from(SAMPLE_RATE)).abs()
                        < 1e-9
                );
            }
        }
        "mp3" => {
            assert!(song["audio_path"].as_str().unwrap().ends_with(".mp3"));
            assert!(audio.starts_with(b"ID3") || audio.starts_with(&[0xff, 0xfb]));
            assert_eq!(song["generation_settings"]["mp3_bitrate"], 160);
            assert_eq!(song["replay_request"]["mp3_bitrate"], 160);
        }
        _ => {
            assert!(song["audio_path"].as_str().unwrap().ends_with(".flac"));
            assert!(audio.starts_with(b"fLaC"));
            assert!(song["generation_settings"].get("mp3_bitrate").is_none());
            assert_eq!(song["generation_settings"]["fade_in"], 0.0);
            assert_eq!(song["replay_request"]["fade_out"], 0.0);
        }
    }
    let submitted: Value =
        serde_json::from_slice(&fs::read(root.path().join("submitted-1.json")).unwrap()).unwrap();
    assert_eq!(submitted["caption"], request["style"]);
    assert_eq!(submitted["lm_model"], MODELS[0].0);
    assert_eq!(submitted["output_format"], "wav32");

    let replay = rpc(
        mcp_request(),
        5,
        "song_replay",
        json!({"song_id":song["id"],"seed":43,"output_format":"wav32"}),
    )
    .await;
    let replay_id = replay["id"].as_str().unwrap();
    let rendered = wait_status(&http, &format!("{jobs}/{replay_id}"), "completed").await;
    wait_for_completed_song_counts(&hub_path, 2).await;
    let take = &rendered["song"]["song"];
    assert_ne!(take["id"], song["id"]);
    assert_eq!(take["profile_id"], song["profile_id"]);
    assert_eq!(take["engine_id"], ENGINE);
    assert_eq!(take["metadata"]["derived"]["from"], song["id"]);
    assert_eq!(
        take["replay_request"]["audio_codes"],
        song["replay_request"]["audio_codes"]
    );
    let replay_body: Value =
        serde_json::from_slice(&fs::read(root.path().join("submitted-2.json")).unwrap()).unwrap();
    assert_eq!(
        replay_body["audio_codes"],
        song["replay_request"]["audio_codes"]
    );
    assert_eq!(replay_body["seed"], 43);
    assert_eq!(replay_body["dit_model"], submitted["dit_model"]);

    let second = timeout(Duration::from_secs(10), async {
        loop {
            let response = http.post(&jobs).json(&request).send().await.unwrap();
            if response.status() != StatusCode::CONFLICT {
                assert_eq!(response.status(), StatusCode::ACCEPTED);
                break response.json::<Value>().await.unwrap();
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let id = second["id"].as_str().unwrap();
    wait_status(&http, &format!("{jobs}/{id}"), "running").await;
    rpc(mcp_request(), 4, "song_job_cancel", json!({"job_id":id})).await;
    let cancelled = wait_status(&http, &format!("{jobs}/{id}"), "cancelled").await;
    assert!(
        cancelled.get("songs").is_none(),
        "empty take lists are omitted"
    );
    assert!(cancelled["song"].is_null());
    assert_eq!(
        fs::read_to_string(root.path().join("cancelled")).unwrap(),
        "3"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("fetched")).unwrap(),
        "2"
    );
    let library = json_response(http.get(format!("{url}/v1/library/songs"))).await;
    assert_eq!(library.as_array().unwrap().len(), 2);
    assert_completed_song_counts(&hub_path, 2);
    studio.kill().await.unwrap();
}

async fn wait_for_completed_song_counts(path: &Path, expected: u64) {
    timeout(Duration::from_secs(3), async {
        loop {
            let hub: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            let (songs, done) = (
                hub["totals"]["songs"].as_u64().unwrap_or_default(),
                hub["totals"]["song_done"].as_u64().unwrap_or_default(),
            );
            assert!(
                songs <= expected && done <= expected,
                "duplicate song completion: {songs}/{done}"
            );
            if songs == expected && done == expected {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("MiniMax completion must be counted");
}

fn assert_completed_song_counts(path: &Path, expected: u64) {
    let hub: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(hub["totals"]["songs"], expected);
    assert_eq!(hub["totals"]["song_done"], expected);
}

async fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn json_response(request: RequestBuilder) -> Value {
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

async fn wait_status(http: &Client, url: &str, expected: &str) -> Value {
    timeout(Duration::from_secs(15), async {
        loop {
            let job = json_response(http.get(url)).await;
            if job["status"] == expected {
                return job;
            }
            assert!(job["status"] != "failed", "{job}");
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {expected} at {url}"))
}

async fn rpc(request: RequestBuilder, id: u32, name: &str, arguments: Value) -> Value {
    let body = request
        .json(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":name,"arguments":arguments}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    let line = body
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap_or(&body);
    let response: Value = serde_json::from_str(line).unwrap();
    let reply = &response["result"];
    assert!(
        response.get("error").is_none() && reply["isError"] != true,
        "{response}"
    );
    reply.get("structuredContent").cloned().unwrap_or_else(|| {
        serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap()
    })
}

async fn build_engine(root: &Path) -> std::path::PathBuf {
    fs::write(
        root.join("fixture.rs"),
        include_str!("fixtures/mm3_http_fixture.rs"),
    )
    .unwrap();
    let binary = root.join(format!("mm-server{}", std::env::consts::EXE_SUFFIX));
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(compiler)
        .arg(root.join("fixture.rs"))
        .arg("-o")
        .arg(&binary)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for cuda in ["cuda12", "cuda13"] {
        let directory = root.join(cuda);
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join(if cfg!(windows) {
                "ggml-cuda.dll"
            } else {
                "libggml-cuda.so"
            }),
            [],
        )
        .unwrap();
    }
    let path = root.join("fixture.wav");
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for _ in 0..FRAMES {
        writer.write_sample(0.125_f32).unwrap();
        writer.write_sample(-0.125_f32).unwrap();
    }
    writer.finalize().unwrap();
    binary
}
