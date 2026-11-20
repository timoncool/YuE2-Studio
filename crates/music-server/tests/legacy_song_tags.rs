use lofty::{
    prelude::{Accessor, TaggedFileExt},
    probe::Probe,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{fs, io::Cursor, net::TcpListener, time::Duration};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

#[tokio::test]
async fn a_migrated_song_keeps_its_performer_when_renamed_and_downloaded() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("current");
    let socket = TcpListener::bind(("127.0.0.1", 0))?;
    let base = format!("http://{}", socket.local_addr()?);
    let port = socket.local_addr()?.port();
    drop(socket);
    let mut server = Command::new(env!("CARGO_BIN_EXE_music-server"))
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_STUDIO_DATA_ROOT", &data)
        .env("YUE_ENGINE_BIN", temp.path().join("absent-engine"))
        .env("TIMONCOOL_SHARED_DIR", temp.path().join("registry"))
        .kill_on_drop(true)
        .spawn()?;
    let http = reqwest::Client::builder().no_proxy().build()?;
    timeout(Duration::from_secs(30), async {
        loop {
            if http
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

    let template = temp.path().join("empty.sqlite");
    Connection::open(data.join("library.sqlite"))?
        .execute("VACUUM INTO ?1", [template.to_str().unwrap()])?;

    for (engine, artist) in [
        ("acestep-cpp", "ACE-Step"),
        ("minimaxmusic-cpp", "MiniMax Music 3"),
    ] {
        let old = temp.path().join(engine);
        fs::create_dir_all(old.join("media"))?;
        let source_db = old.join("library.sqlite");
        // Start with the real schema and keep each source library independent.
        fs::copy(&template, &source_db)?;
        let db = Connection::open(source_db)?;
        db.execute("INSERT INTO songs (id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,engine_id,source,created_at,updated_at)
                    VALUES ('old-song','Take','old-song.wav','','','{}','{}',?1,'native','1','1')", params![engine])?;
        drop(db);
        let source = old.join("media/old-song.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec)?;
        for sample in [100_i16, -100, 200, -200] {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
        let original = fs::read(&source)?;

        let result: Value = http
            .post(format!("{base}/v1/library/migration/import"))
            .json(&json!({"path": old}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(result["added"], 1);
        let songs: Vec<Value> = http
            .get(format!("{base}/v1/library/songs"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let id = songs
            .iter()
            .find(|song| song["engine_id"] == engine)
            .unwrap()["id"]
            .as_str()
            .unwrap();
        http.put(format!("{base}/v1/library/songs/{id}/title"))
            .json(&json!({"title": "My take"}))
            .send()
            .await?
            .error_for_status()?;
        let bytes = http
            .get(format!("{base}/v1/library/media/{id}"))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let file = Probe::new(Cursor::new(bytes.as_ref()))
            .guess_file_type()?
            .read()?;
        let tags = file.primary_tag().unwrap();
        assert_eq!(tags.artist().as_deref(), Some(artist));
        assert_eq!(tags.title().as_deref(), Some("My take"));
        let mut audio = hound::WavReader::new(Cursor::new(bytes.as_ref()))?;
        assert_eq!(
            audio.samples::<i16>().collect::<Result<Vec<_>, _>>()?,
            [100, -100, 200, -200]
        );
        assert_eq!(fs::read(source)?, original);
    }
    server.kill().await?;
    Ok(())
}
