use serde_json::{Value, json};
use std::{fs, net::TcpListener, path::Path, time::Duration};
use tokio::{
    process::{Child, Command},
    time::{sleep, timeout},
};

async fn studio(root: &Path, http: &reqwest::Client) -> anyhow::Result<(Child, String)> {
    let socket = TcpListener::bind(("127.0.0.1", 0))?;
    let address = socket.local_addr()?;
    drop(socket);
    let server = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", address.port().to_string())
        .env("YUE_STUDIO_DATA_ROOT", root.join("data"))
        .env("YUE_ENGINE_BIN", root.join("absent-engine"))
        .env("TIMONCOOL_SHARED_DIR", root.join("registry"))
        .env("DO_NOT_TRACK", "1")
        .env("TEMP", root)
        .env("TMP", root)
        .env("TMPDIR", root)
        .kill_on_drop(true)
        .spawn()?;
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
    Ok((server, base))
}

async fn mcp(http: &reqwest::Client, base: &str, tool: &str, args: Value) -> anyhow::Result<Value> {
    let reply: Value = http
        .post(format!("{base}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":tool, "arguments":args}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    Ok(reply["result"]["structuredContent"].clone())
}

#[tokio::test]
async fn preferred_take_survives_restart_and_reordering_without_touching_media()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let http = reqwest::Client::builder().no_proxy().build()?;
    let (mut server, base) = studio(root.path(), &http).await?;
    let media = root.path().join("data/media");
    fs::create_dir_all(&media)?;
    let mut songs = Vec::new();
    for (file, sample) in [
        ("one.wav", 100_i16),
        ("two.wav", -200),
        ("outsider.wav", 300),
    ] {
        let path = media.join(file);
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )?;
        wav.write_sample(sample)?;
        wav.finalize()?;
        let song: Value = http
            .post(format!("{base}/v1/library/songs"))
            .json(&json!({"title":"Same title", "audio_path":file, "engine_id":"minimaxmusic-cpp"}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let id = song["id"].as_str().unwrap().to_owned();
        // Renaming writes the tags synchronously so the startup backfill does
        // not rewrite these fixture files after restarting the service.
        http.put(format!("{base}/v1/library/songs/{id}/title"))
            .json(&json!({"title":"Same title"}))
            .send()
            .await?
            .error_for_status()?;
        let original = http
            .get(format!("{base}/v1/library/media/{id}"))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        songs.push((id, original));
    }
    let (first, second, outsider) = (&songs[0].0, &songs[1].0, &songs[2].0);
    let playlist: Value = http
        .post(format!("{base}/v1/library/playlists"))
        .json(&json!({"name":"Drafts", "song_ids":[first, second]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(playlist.get("preferred_song_id").is_none());
    let id = playlist["id"].as_str().unwrap();
    let endpoint = format!("{base}/v1/library/playlists/{id}");
    let rejected = http
        .put(format!("{endpoint}/preferred-take"))
        .json(&json!({"song_id":outsider}))
        .send()
        .await?;
    assert_eq!(rejected.status(), reqwest::StatusCode::BAD_REQUEST);
    let args = json!({"playlist_id":id, "song_id":second});
    assert_eq!(
        mcp(&http, &base, "playlist_select_preferred_take", args.clone()).await?,
        json!({"preferred_song_id":second, "changed":true})
    );
    assert_eq!(
        mcp(&http, &base, "playlist_select_preferred_take", args).await?,
        json!({"preferred_song_id":second, "changed":false})
    );
    let journal: Vec<Value> = http
        .get(format!("{base}/v1/journal"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        journal
            .iter()
            .filter(|entry| entry["source"] == "agent"
                && entry["verb"] == "updated"
                && entry["kind"] == "playlist"
                && entry["target"] == "Drafts")
            .count(),
        1,
        "Journal must contain one named change for the first selection: {journal:?}",
    );
    let reversed: Value = http
        .put(&endpoint)
        .json(&json!({"name":"Renamed", "song_ids":[second, first]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(reversed["preferred_song_id"], *second);
    server.kill().await?;

    let (_reopened, base) = studio(root.path(), &http).await?;
    let reopened = mcp(&http, &base, "playlist_get", json!({"playlist_id":id})).await?;
    assert_eq!(reopened["preferred_song_id"], *second);
    assert_eq!(reopened["song_ids"], json!([second, first]));
    for (song_id, original) in &songs[..2] {
        let data = http
            .get(format!("{base}/v1/library/media/{song_id}"))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        assert_eq!(&data, original);
    }
    let response: Value = http
        .put(format!("{base}/v1/library/playlists/{id}"))
        .json(&json!({"name":"Renamed", "song_ids":[first]}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert!(response.get("preferred_song_id").is_none());
    http.get(format!("{base}/v1/library/media/{second}"))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
