use anyhow::{Context, Result};
use reqwest::{Client, RequestBuilder, StatusCode, multipart};
use serde_json::{Value, json};
use std::{fs, net::TcpListener, path::Path, time::Duration};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
#[ignore = "subprocess used by the headless save contract test"]
async fn serve_with_file_dialog_fixture() -> Result<()> {
    let folder =
        std::env::var_os("YUE_SAVE_TEST_FOLDER").context("missing save fixture directory")?;
    music_server::set_save_dialog(Box::new(move |name, _| {
        if name == "Отмена.wav" {
            None
        } else if name == "Ошибка.wav" {
            Some(Path::new(&folder).join("Нет папки").join(name))
        } else {
            Some(Path::new(&folder).join(name))
        }
    }));
    music_server::serve().await
}

async fn response(request: RequestBuilder) -> Result<Value> {
    Ok(request.send().await?.error_for_status()?.json().await?)
}

async fn choose(client: &Client, base: &str, name: &str) -> Result<Value> {
    response(
        client
            .post(format!("{base}/v1/files/save"))
            .json(&json!({ "name": name })),
    )
    .await
}

fn write(client: &Client, base: &str, place: &Value) -> RequestBuilder {
    client.post(format!(
        "{base}/v1/files/save/{}",
        place["id"].as_str().unwrap()
    ))
}

#[tokio::test]
async fn desktop_save_contract_runs_through_http_with_unicode_paths() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("Готовые песни");
    fs::create_dir(&output)?;
    let socket = TcpListener::bind(("127.0.0.1", 0))?;
    let address = socket.local_addr()?;
    let base = format!("http://{address}");
    drop(socket);
    let mut service = Command::new(std::env::current_exe()?)
        .args([
            "--ignored",
            "--exact",
            "serve_with_file_dialog_fixture",
            "--nocapture",
        ])
        .env("YUE_STUDIO_PORT", address.port().to_string())
        .env("YUE_STUDIO_DATA_ROOT", temp.path().join("studio"))
        .env("YUE_ENGINE_BIN", temp.path().join("absent-engine"))
        .env("TIMONCOOL_SHARED_DIR", temp.path().join("registry"))
        .env("YUE_SAVE_TEST_FOLDER", &output)
        .kill_on_drop(true)
        .spawn()?;
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()?;
    timeout(Duration::from_secs(30), async {
        loop {
            if client
                .get(format!("{base}/health"))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;

    let original = temp.path().join("Исходник.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(&original, spec)?;
    for frame in 0..44100 {
        wav.write_sample(
            ((frame as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 4096.0) as i16,
        )?;
    }
    wav.finalize()?;
    let before = fs::read(&original)?;
    let source = response(client.post(format!("{base}/v1/library/import")).multipart(
        multipart::Form::new().text("title", "Репетиция").part(
            "audio",
            multipart::Part::bytes(before.clone()).file_name("Исходник.wav"),
        ),
    ))
    .await?;
    let song_id = source["id"].as_str().context("no imported song id")?;
    let media = format!("/v1/library/media/{song_id}");
    let media_before = client
        .get(format!("{base}{media}"))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    assert_eq!(
        choose(&client, &base, "Отмена.wav").await?,
        json!({ "cancelled": true })
    );
    assert_eq!(fs::read_dir(&output)?.count(), 0);
    let failed = choose(&client, &base, "Ошибка.wav").await?;
    let error = write(&client, &base, &failed)
        .json(&json!({ "url": media }))
        .send()
        .await?;
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bad_body: Value = error.json().await?;
    assert!(
        bad_body["error"]
            .as_str()
            .is_some_and(|e| e.contains("Ошибка.wav")),
        "{bad_body}"
    );
    assert_eq!(
        write(&client, &base, &failed)
            .body("retry")
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(fs::read_dir(&output)?.count(), 0);

    let zip = response(
        client
            .post(format!("{base}/v1/exports/tracks"))
            .json(&json!({ "song_ids": [song_id] })),
    )
    .await?;
    let zip_url = zip["url"].as_str().context("missing ZIP URL")?;
    let ableton = response(
        client
            .post(format!("{base}/v1/exports/live"))
            .json(&json!({ "song_id": song_id })),
    )
    .await?;
    let live_url = format!(
        "/v1/exports/live/{}/file",
        ableton["id"].as_str().context("missing Live project id")?
    );

    let mut outputs = vec![
        ("Песня.wav".to_owned(), media.clone()),
        ("Архив.zip".to_owned(), zip_url.to_owned()),
        ("Проект Ableton.zip".to_owned(), live_url),
    ];
    for (format, depth) in [("wav", Some("16")), ("flac", Some("24")), ("mp3", None)] {
        let job = response(client.post(format!("{base}/v1/exports")).json(
            &json!({"song_id": song_id, "format": format, "bit_depth": depth, "mp3_kbps": 128}),
        ))
        .await?;
        let id = job["id"].as_str().context("missing audio export id")?;
        let state = format!("{base}/v1/exports/{id}");
        timeout(Duration::from_secs(30), async {
            loop {
                let report = response(client.get(&state)).await?;
                if report["done"].as_bool() == Some(true) {
                    anyhow::ensure!(report["error"].is_null(), "{report}");
                    return Ok::<_, anyhow::Error>(());
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await??;
        outputs.push((
            format!("Сведённая песня.{format}"),
            format!("/v1/exports/{id}/file"),
        ));
    }

    for (name, url) in &outputs {
        let expected = client
            .get(format!("{base}{url}"))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        assert!(!expected.is_empty());
        let selected = choose(&client, &base, name).await?;
        assert_eq!(
            selected["path"].as_str().map(Path::new),
            Some(output.join(name).as_path())
        );
        let saved = response(write(&client, &base, &selected).json(&json!({ "url": url }))).await?;
        assert_eq!(saved["written"].as_u64(), Some(expected.len() as u64));
        assert_eq!(fs::read(output.join(name))?, expected);
    }
    let name = "Текст песни.txt";
    let selected = choose(&client, &base, name).await?;
    let text = "Проверка текста".as_bytes();
    let saved = response(
        write(&client, &base, &selected)
            .header("Content-Type", "application/octet-stream")
            .body(text),
    )
    .await?;
    assert_eq!(saved["written"].as_u64(), Some(text.len() as u64));
    assert_eq!(fs::read(output.join(name))?, text);

    assert_eq!(fs::read(&original)?, before);
    assert_eq!(
        client
            .get(format!("{base}{media}"))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?,
        media_before
    );
    assert_eq!(fs::read_dir(&output)?.count(), outputs.len() + 1);
    service.kill().await?;
    Ok(())
}
