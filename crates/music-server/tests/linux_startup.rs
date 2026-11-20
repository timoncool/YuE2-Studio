#![cfg(target_os = "linux")]

#[tokio::test]
async fn an_empty_system_trust_store_reports_how_to_start_the_studio() {
    let root = tempfile::tempdir().unwrap();
    let cert_dir = root.path().join("empty-certs");
    std::fs::create_dir(&cert_dir).unwrap();
    let bundle = root.path().join("empty.pem");
    std::fs::write(&bundle, []).unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut service = tokio::process::Command::new(env!("CARGO_BIN_EXE_music-server"));
    service
        .env("YUE_STUDIO_PORT", port.to_string())
        .env("YUE_STUDIO_DATA_ROOT", root.path())
        .env("SSL_CERT_FILE", bundle)
        .env("SSL_CERT_DIR", cert_dir)
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), service.output())
        .await.expect("service should report a startup error").unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("ca-certificates"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}
