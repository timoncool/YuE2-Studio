use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Read},
    net::TcpListener,
    time::Duration,
};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
async fn exports_same_title_originals_and_reports_the_failing_song() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let data = root.path().join("data");
    let socket = TcpListener::bind(("127.0.0.1", 0))?;
    let address = socket.local_addr()?;
    drop(socket);
    let mut server = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", address.port().to_string())
        .env("YUE_STUDIO_DATA_ROOT", &data)
        .env("YUE_ENGINE_BIN", root.path().join("absent-engine"))
        .env("TIMONCOOL_SHARED_DIR", root.path().join("registry"))
        .env("DO_NOT_TRACK", "1")
        .env("TEMP", root.path())
        .env("TMP", root.path())
        .env("TMPDIR", root.path())
        .kill_on_drop(true)
        .spawn()?;
    let http = reqwest::Client::builder().no_proxy().build()?;
    let base = format!("http://{address}");
    timeout(Duration::from_secs(30), async {
        while !http
            .get(format!("{base}/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;

    let media = data.join("media");
    fs::create_dir_all(&media)?;
    let score = "X:1\nT:Take\nM:4/4\nL:1/4\nK:C\nCDEF|]";
    let mut ids = Vec::new();
    for (file, sample, abc) in [("one.wav", 100_i16, Some(score)), ("two.wav", -200, None)] {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(media.join(file), spec)?;
        wav.write_sample(sample)?;
        wav.finalize()?;
        let song: Value = http.post(format!("{base}/v1/library/songs"))
            .json(&json!({
                "title": "Same title", "audio_path": file, "engine_id": "minimaxmusic-cpp",
                "caption": "folk prompt", "lyrics": "verse text", "generation_settings": {"abc": abc, "seed": 35},
            }))
            .send().await?.error_for_status()?.json().await?;
        ids.push(song["id"].as_str().unwrap().to_owned());
    }
    let midi = b"MThd\0\0\0\x06\0\0\0\x01\0\x60MTrk\0\0\0\x04\0\xff\x2f\0";
    fs::write(media.join(format!("{}.mid", ids[0])), midi)?;
    let endpoint = format!("{base}/v1/exports/tracks");
    let bundle: Value = http
        .post(&endpoint)
        .json(&json!({"song_ids": [&ids[0], &ids[1], &ids[0]]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let response = http
        .get(format!("{base}{}", bundle["url"].as_str().unwrap()))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(response.headers()["content-type"], "application/zip");
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"tracks.zip\""
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(response.bytes().await?))?;
    assert_eq!(
        archive
            .file_names()
            .filter_map(Result::ok)
            .filter(|name| name.ends_with("/audio.wav"))
            .count(),
        2
    );
    let mut read = |name: &str| -> anyhow::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        archive.by_name(name)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    assert_eq!(
        read("001 - Same title/audio.wav")?,
        fs::read(media.join("one.wav"))?
    );
    assert_eq!(
        read("002 - Same title/audio.wav")?,
        fs::read(media.join("two.wav"))?
    );
    assert_eq!(read("001 - Same title/transcription.mid")?, midi);
    assert!(String::from_utf8(read("001 - Same title/score.abc")?)?.contains("CDEF"));
    let settings = String::from_utf8(read("001 - Same title/settings.txt")?)?;
    for text in ["folk prompt", "verse text", "\"seed\": 35"] {
        assert!(settings.contains(text));
    }
    assert!(read("002 - Same title/score.abc").is_err());

    fs::remove_file(media.join("two.wav"))?;
    let response = http
        .post(&endpoint)
        .json(&json!({"song_ids": ids}))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = response.json().await?;
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains("Same title") && error.contains(&ids[1]) && error.contains("original audio")
    );
    server.kill().await?;
    Ok(())
}
