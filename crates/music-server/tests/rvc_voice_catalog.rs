use std::{
    fs,
    io::{Cursor, Write},
    time::Duration,
};

use reqwest::{
    multipart::{Form, Part},
    Client, StatusCode,
};
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    process::Command,
    time::{sleep, timeout},
};

fn checkpoint() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("archive/data.pkl", stored).unwrap();
    // The catalog checks the container and inference-weight names. The
    // full native parser is exercised by inference tests.
    zip.write_all(b"enc_p.emb_phone.weight emb_g.weight dec.ups.0.weight X\x07\0\0\0versionq\x01X\x02\0\0\0v2").unwrap();
    zip.start_file("archive/data/0", stored).unwrap();
    zip.write_all(b"tensor").unwrap();
    zip.finish().unwrap().into_inner()
}

fn index(dim: u32) -> Vec<u8> {
    let mut data = vec![0; 37 + dim as usize * 4];
    data[..4].copy_from_slice(b"IwFl");
    data[4..8].copy_from_slice(&dim.to_le_bytes());
    data[8] = 1; // One vector.
    data[32] = 1; // Trained.
    data[33] = 1; // L2.
    data
}

async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn imports_rvc_checkpoint_and_optional_index_through_the_catalog() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let url = format!("{base}/v1/rvc/voices");
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let mut studio = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_ENGINE_PORT", free_port().await.to_string())
        .env("YUE_STUDIO_DATA_ROOT", &data)
        .env("YUE_ENGINE_BIN", root.path().join("absent-yue-server"))
        .env("TIMONCOOL_SHARED_DIR", root.path().join("registry"))
        .env("DO_NOT_TRACK", "1")
        .env("STUDIO_HUB_URL", format!("{base}/fixture-hub"))
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let initial = timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(response) = client.get(&url).send().await {
                break response
                    .error_for_status()
                    .unwrap()
                    .json::<Vec<Value>>()
                    .await
                    .unwrap();
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert!(initial.is_empty());

    let voice_part = || Part::bytes(checkpoint()).file_name("my-rvc-v2.pt");
    let request = |form| client.post(format!("{url}/import")).multipart(form);
    let wrong_index = Form::new()
        .part("model", voice_part())
        .part("index", Part::bytes(index(256)).file_name("v1.index"));
    assert_eq!(
        request(wrong_index).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let forged = Form::new().part(
        "model",
        Part::bytes(b"not weights".as_slice()).file_name("fake.pth"),
    );
    assert_eq!(
        request(forged).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let with_index = || {
        Form::new()
            .part("model", voice_part())
            .part("index", Part::bytes(index(768)).file_name("v2.index"))
    };
    let result = request(with_index()).send().await.unwrap();
    assert_eq!(result.status(), StatusCode::CREATED);
    let indexed: Value = result.json().await.unwrap();
    assert_eq!(indexed["version"], 2);
    assert_eq!(indexed["index_name"], "v2.index");
    let repeated = request(with_index()).send().await.unwrap();
    assert_eq!(repeated.status(), StatusCode::OK);
    assert_eq!(repeated.json::<Value>().await.unwrap(), indexed);

    let plain = request(Form::new().part("model", voice_part()))
        .send()
        .await
        .unwrap();
    assert_eq!(plain.status(), StatusCode::CREATED);
    let plain: Value = plain.json().await.unwrap();
    assert!(plain["index_name"].is_null());
    let list: Vec<Value> = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.contains(&indexed));
    assert!(list.contains(&plain));

    let catalog = data.join("rvc/voices");
    let indexed_files = catalog.join(indexed["id"].as_str().unwrap());
    assert_eq!(
        fs::read(indexed_files.join("voice.pth")).unwrap(),
        checkpoint()
    );
    assert_eq!(
        fs::read(indexed_files.join("retrieval.index")).unwrap(),
        index(768)
    );
    let model = root.path().join("my-rvc-v2.pt");
    let retrieval = root.path().join("v2.index");
    fs::write(&model, checkpoint()).unwrap();
    fs::write(&retrieval, index(768)).unwrap();
    let rpc = |id, name, args: Value| {
        client
            .post(format!("{base}/mcp"))
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":name,"arguments":args}}))
    };
    let tool_import = rpc(
        1,
        "rvc_voice_import",
        json!({"model_path":model, "index_path":retrieval}),
    )
    .send()
    .await
    .unwrap()
    .json::<Value>()
    .await
    .unwrap();
    assert_eq!(
        tool_import["result"]["structuredContent"], indexed,
        "{tool_import}"
    );
    let tool_list = rpc(2, "rvc_voices_list", json!({}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(
        tool_list["result"]["structuredContent"],
        json!(list),
        "{tool_list}"
    );

    assert_eq!(fs::read_dir(&catalog).unwrap().count(), 2);
    studio.kill().await.unwrap();
}
