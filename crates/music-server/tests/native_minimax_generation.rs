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

#[tokio::test]
async fn audio_cpp_minimax_music3_generates_and_saves_q8_provenance() {
    use base64::Engine as _;
    let root = tempfile::tempdir().unwrap();
    let engine = build_engine(root.path()).await;
    let data = root.path().join("data");
    let model = data.join("audiocpp/models/minimax_music3");
    fs::create_dir_all(model.join("config")).unwrap();
    fs::create_dir_all(model.join("tokenizer")).unwrap();
    for file in [
        "config.json",
        "config/language_model.json",
        "config/rvq_depth_decoder.json",
        "config/condition_encoder.json",
        "config/transformer.json",
        "config/vocoder.json",
        "tokenizer/tokenizer.json",
        "tokenizer/tokenizer_config.json",
    ] {
        fs::write(model.join(file), b"{}").unwrap();
    }
    let weights = [
        "language_model_q8_0.gguf",
        "rvq_depth_decoder_q8_0.gguf",
        "transformer_q8_0.gguf",
        "condition_encoder.gguf",
        "vocoder.gguf",
    ];
    let wav = fs::read(root.path().join("fixture.wav")).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(wav);
    fs::write(
        root.path().join("audio-cpp-response.json"),
        json!({"audio": encoded}).to_string(),
    )
    .unwrap();
    let port = free_port().await;
    let mut studio = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_BIN", root.path().join("absent-yue-server"))
        .env("YUE_STUDIO_DATA_ROOT", &data)
        .env("YUE_AUDIOCPP_SERVER", &engine)
        .env("TIMONCOOL_SHARED_DIR", root.path().join("registry"))
        .env("DO_NOT_TRACK", "0")
        .env("STUDIO_TELEMETRY", "1")
        .env("STUDIO_HUB_URL", format!("http://127.0.0.1:{port}/fixture-hub"))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let http = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let catalog = format!("{url}/v1/music/audio-cpp/models");
    let initial = timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(response) = http.get(&catalog).send().await {
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
    assert_eq!(initial["models"][0]["ready"], false);
    assert!(
        initial["models"][0]["missing"]
            .as_array()
            .unwrap()
            .contains(&json!(weights[0]))
    );
    for name in weights {
        let mut gguf = vec![0; 32];
        gguf[..4].copy_from_slice(b"GGUF");
        fs::write(model.join(name), gguf).unwrap();
    }
    let ready: Value = http
        .get(&catalog)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ready["models"][0]["id"], "minimax_music3");
    assert_eq!(ready["models"][0]["profile_id"], "quality-q8");
    assert_eq!(ready["models"][0]["ready"], true);
    let endpoint = format!("{url}/v1/music/jobs");
    let request = json!({
        "music_source": {"kind":"audio_cpp","model":"minimax_music3"},
        "style":"Soft folk guitar", "lyrics":"[verse] Morning comes",
        "duration_seconds":3.2, "steps":2, "seed":424242
    });
    let mut unknown = request.clone();
    unknown["music_source"]["model"] = json!("unknown_model");
    let rejected = http.post(&endpoint).json(&unknown).send().await.unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert!(!root.path().join("audio-cpp-submitted.json").exists());
    http.post(format!("{url}/v1/hub/telemetry"))
        .json(&json!({"enabled": true, "acknowledge": true}))
        .send().await.unwrap().error_for_status().unwrap();
    let accepted = http.post(&endpoint).json(&request).send().await.unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let id = accepted.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let completed = timeout(Duration::from_secs(20), async {
        loop {
            let job: Value = http
                .get(format!("{endpoint}/{id}"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            match job["status"].as_str() {
                Some("completed") => break job,
                Some("queued" | "running") => sleep(Duration::from_millis(50)).await,
                _ => panic!("audio.cpp generation did not complete: {job}"),
            }
        }
    })
    .await
    .unwrap();
    let track = &completed["songs"][0];
    let song = &track["song"];
    assert_eq!(song["engine_id"], "audio.cpp");
    assert_eq!(song["profile_id"], "quality-q8");
    assert_eq!(song["generation_settings"]["family"], "minimax_music3");
    assert_eq!(song["generation_settings"]["quantization"], "Q8_0");
    assert_eq!(
        song["generation_settings"]["weight_files"]["minimax_music3.language_model_gguf"],
        "language_model_q8_0.gguf"
    );
    assert_eq!(song["generation_settings"]["seed"], 424242);
    assert_eq!(song["metadata"]["output_format"], "flac");
    let media = http
        .get(format!("{url}{}", track["audio_url"].as_str().unwrap()))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(&media[..4], b"fLaC");
    let sent: Value =
        serde_json::from_slice(&fs::read(root.path().join("audio-cpp-submitted.json")).unwrap())
            .unwrap();
    assert_eq!(sent["model"], "minimax_music3");
    assert_eq!(sent["request"]["duration_seconds"], 3.2);
    assert_eq!(sent["request"]["num_inference_steps"], 2);
    assert_eq!(sent["request"]["lyrics"], "[verse] Morning comes");
    let unloaded: Value =
        serde_json::from_slice(&fs::read(root.path().join("audio-cpp-unloaded.json")).unwrap())
            .unwrap();
    assert_eq!(unloaded["model_ids"], json!(["minimax_music3"]));
    let report = timeout(Duration::from_secs(5), async {
        loop {
            let preview: Value = http.get(format!("{url}/v1/hub/telemetry/preview"))
                .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
            if let Some(rows) = preview["report"]["speed"].as_array() {
                if !rows.is_empty() {
                    break preview["report"].clone();
                }
            }
            sleep(Duration::from_millis(50)).await;
        }
    }).await.expect("saved audio.cpp song has a timing in the scheduled report");
    assert_eq!(report["counts"]["songs"], 1);
    assert_eq!(report["counts"]["song_done"], 1);
    assert_eq!(report["model_sets"][0]["count"], 1);
    let set = report["model_sets"][0]["set"].as_str().unwrap();
    assert!(set.starts_with("audio.cpp.minimax_music3."));
    assert_eq!(report["speed"][0]["set"], set);
    assert_eq!(report["speed"][0]["kind"], "song");
    studio.kill().await.unwrap();
}

#[tokio::test]
async fn cancelled_audio_cpp_heartmula_worker_exits_before_the_next_song() {
    use base64::Engine as _;
    use std::io::Write;
    use tokio::net::TcpStream;

    let root = tempfile::tempdir().unwrap();
    let engine = build_engine(root.path()).await;
    let data = root.path().join("data");
    let model = data.join("audiocpp/models/HeartMuLa-GGUF/heartmula-q8_0.gguf");
    fs::create_dir_all(model.parent().unwrap()).unwrap();
    let mut weights = fs::File::create(&model).unwrap();
    #[cfg(windows)]
    {
        let sparse = Command::new("fsutil")
            .args(["sparse", "setflag"])
            .arg(&model)
            .status()
            .await
            .unwrap();
        assert!(sparse.success());
    }
    weights.write_all(b"GGUF").unwrap();
    weights.set_len(7_659_762_592).unwrap();
    drop(weights);

    let wav = fs::read(root.path().join("fixture.wav")).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(wav);
    fs::write(
        root.path().join("audio-cpp-response.json"),
        json!({"audio": encoded}).to_string(),
    )
    .unwrap();
    fs::write(root.path().join("audio-cpp-hold-next"), b"").unwrap();

    let port = free_port().await;
    let http = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let jobs = format!("{url}/v1/music/jobs");
    let mut studio = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_BIN", root.path().join("absent-yue-server"))
        .env("YUE_STUDIO_DATA_ROOT", &data)
        .env("YUE_AUDIOCPP_SERVER", &engine)
        .env("TIMONCOOL_SHARED_DIR", root.path().join("registry"))
        .env("DO_NOT_TRACK", "1")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    timeout(Duration::from_secs(30), async {
        while http.get(format!("{url}/health")).send().await.is_err() {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let request = json!({
        "music_source":{"kind":"audio_cpp","model":"heartmula"},
        "title":"Cancelled fixture song", "style":"Soft piano", "duration_seconds":3.2,
        "steps":2, "seed":42,
    });
    let first: Value = json_response(http.post(&jobs).json(&request)).await;
    let id = first["id"].as_str().unwrap();
    let worker: Value = timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(bytes) = fs::read(root.path().join("audio-cpp-held-worker.json"))
                && let Ok(ready) = serde_json::from_slice(&bytes)
            {
                break ready;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let worker_port = worker["port"].as_u64().unwrap() as u16;
    let sent: Value =
        serde_json::from_slice(&fs::read(root.path().join("audio-cpp-submitted.json")).unwrap())
            .unwrap();
    assert_eq!(sent["model"], "heartmula");
    assert_eq!(sent["request"]["options"]["tags"], "Soft piano");
    assert!(TcpStream::connect(("127.0.0.1", worker_port)).await.is_ok());
    assert_eq!(
        http.post(&jobs)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );

    let stopping: Value = json_response(http.post(format!("{jobs}/{id}"))).await;
    assert_eq!(stopping["status"], "running");
    assert!(
        stopping["message"]
            .as_str()
            .unwrap()
            .starts_with("Stopping")
    );
    let cancelled: Value = timeout(Duration::from_secs(5), async {
        loop {
            let job: Value = json_response(http.get(format!("{jobs}/{id}"))).await;
            if job["status"] == "cancelled" {
                break job;
            }
            assert_eq!(job["status"], "running");
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("cancellation released the engine without waiting for the held task");
    assert!(cancelled["song"].is_null());
    assert!(
        TcpStream::connect(("127.0.0.1", worker_port))
            .await
            .is_err()
    );
    assert!(!root.path().join("audio-cpp-finished-hold").exists());

    let mut retry = request;
    retry["title"] = json!("Next fixture song");
    let next: Value = json_response(http.post(&jobs).json(&retry)).await;
    let id = next["id"].as_str().unwrap();
    let completed: Value = timeout(Duration::from_secs(10), async {
        loop {
            let job: Value = json_response(http.get(format!("{jobs}/{id}"))).await;
            if job["status"] == "completed" {
                break job;
            }
            assert_eq!(job["status"], "running");
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let audio = completed["song"]["audio_url"].as_str().unwrap();
    let media = http
        .get(format!("{url}{audio}"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(&media[..4], b"fLaC");
    let new_pid: u64 = fs::read_to_string(root.path().join("audio-cpp-completed-worker.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(Some(new_pid), worker["pid"].as_u64());
    studio.kill().await.unwrap();
}

#[tokio::test]
async fn audio_cpp_vevo2_rejects_truncated_weights_and_sings_with_a_local_bundle() {
    use base64::Engine as _;
    use reqwest::multipart::{Form, Part};

    let root = tempfile::tempdir().unwrap();
    let engine = build_engine(root.path()).await;
    let data = root.path().join("data");
    let model = data.join("audiocpp/models/vevo2");
    fs::create_dir_all(&model).unwrap();
    let mut gguf = vec![0; 32];
    gguf[..4].copy_from_slice(b"GGUF");
    fs::write(model.join("vevo2-q8_0.gguf"), gguf).unwrap();
    let wav = fs::read(root.path().join("fixture.wav")).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&wav);
    fs::write(
        root.path().join("audio-cpp-response.json"),
        json!({"audio": encoded}).to_string(),
    )
    .unwrap();

    let port = free_port().await;
    let mut studio = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_BIN", root.path().join("absent-yue-server"))
        .env("YUE_STUDIO_DATA_ROOT", &data)
        .env("YUE_AUDIOCPP_SERVER", &engine)
        .env("TIMONCOOL_SHARED_DIR", root.path().join("registry"))
        .env("DO_NOT_TRACK", "1")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let http = Client::builder().no_proxy().build().unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let catalog = format!("{url}/v1/music/audio-cpp/models");
    let models = timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(response) = http.get(&catalog).send().await {
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
    let vevo2 = models["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == "vevo2")
        .unwrap();
    assert_eq!(vevo2["ready"], false);
    assert_eq!(vevo2["missing"], json!(["vevo2-q8_0.gguf"]));
    let attach = format!("{url}/v1/music/audio-cpp/vevo2/model-directory");
    let truncated = http
        .post(&attach)
        .json(&json!({"directory": model}))
        .send()
        .await
        .unwrap();
    assert_eq!(truncated.status(), StatusCode::BAD_REQUEST);
    let weights = std::env::var_os("YUE2_QA_VEVO2_DIRECTORY");
    if let Some(directory) = &weights {
        http.post(&attach)
            .json(&json!({"directory": std::path::Path::new(directory)}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }

    let mut ids = Vec::new();
    for title in ["Dry voice", "Melody"] {
        let form = Form::new().text("title", title).part(
            "audio",
            Part::bytes(wav.clone()).file_name(format!("{title}.wav")),
        );
        let imported: Value = http
            .post(format!("{url}/v1/library/import"))
            .multipart(form)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        ids.push(imported["id"].as_str().unwrap().to_owned());
    }

    let endpoint = format!("{url}/v1/music/jobs");
    let mut request = json!({
        "music_source": {"kind":"audio_cpp","model":"vevo2"},
        "lyrics":"Morning comes", "seed":123
    });
    let missing_voice = http.post(&endpoint).json(&request).send().await.unwrap();
    assert_eq!(missing_voice.status(), StatusCode::BAD_REQUEST);

    request["vevo2"] = json!({"voice_song_id": ids[0], "melody_song_id": ids[1]});
    let accepted = http.post(&endpoint).json(&request).send().await.unwrap();
    if weights.is_none() {
        assert_eq!(accepted.status(), StatusCode::SERVICE_UNAVAILABLE);
        studio.kill().await.unwrap();
        return;
    }
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let accepted: Value = accepted.json().await.unwrap();
    let id = accepted["id"].as_str().unwrap();
    let completed = timeout(Duration::from_secs(20), async {
        loop {
            let job: Value = http
                .get(format!("{endpoint}/{id}"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            match job["status"].as_str() {
                Some("completed") => break job,
                Some("queued" | "running") => sleep(Duration::from_millis(50)).await,
                _ => panic!("Vevo2 generation did not complete: {job}"),
            }
        }
    })
    .await
    .unwrap();
    let track = &completed["songs"][0];
    assert_eq!(track["song"]["engine_id"], "audio.cpp");
    assert_eq!(track["song"]["profile_id"], "quality-q8");
    assert_eq!(
        track["song"]["generation_settings"]["route"],
        "text_to_singing"
    );
    assert_eq!(
        track["song"]["generation_settings"]["vevo2"],
        request["vevo2"]
    );
    let audio = http
        .get(format!("{url}{}", track["audio_url"].as_str().unwrap()))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(&audio[..4], b"fLaC");

    let sent: Value =
        serde_json::from_slice(&fs::read(root.path().join("audio-cpp-submitted.json")).unwrap())
            .unwrap();
    assert_eq!(sent["model"], "vevo2");
    assert_eq!(sent["request"]["text"], "Morning comes");
    assert_eq!(sent["request"]["options"]["route"], "text_to_singing");
    assert_eq!(sent["request"]["options"]["seed"], 123);
    assert_eq!(sent["request"]["options"]["use_prosody_code"], true);
    for option in ["target_voice", "prosody_ref"] {
        let staged = Path::new(sent["request"]["options"][option].as_str().unwrap());
        assert_eq!(staged.extension().unwrap(), "wav");
        assert!(
            !staged.exists(),
            "the completed job retained its temporary input"
        );
    }
    studio.kill().await.unwrap();
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
        .env("DO_NOT_TRACK", "0")
        .env("STUDIO_TELEMETRY", "1")
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

    http.post(format!("{url}/v1/hub/telemetry"))
        .json(&json!({"enabled":true,"acknowledge":true}))
        .send().await.unwrap().error_for_status().unwrap();
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
    timeout(Duration::from_secs(10), async {
        loop {
            let preview: Value = http.get(format!("{url}/v1/hub/telemetry/preview"))
                .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
            if preview["report"]["model_sets"].as_array().is_some_and(|sets| sets.iter().any(|set|
                set["set"] == "native.minimax_music3" && set["components"] == json!([ENGINE])
            )) { break }
            sleep(Duration::from_millis(50)).await;
        }
    }).await.expect("native MiniMax replay appears in the scheduled report");
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
