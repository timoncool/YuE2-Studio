use anyhow::{Context, Result, ensure};
use std::{
    path::Path,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) async fn start(
    exe: &Path,
    models: &Path,
    props: &str,
    stop: &AtomicBool,
) -> Result<(
    tokio::process::Child,
    reqwest::Client,
    String,
    serde_json::Value,
)> {
    let socket = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = socket.local_addr()?.port();
    drop(socket);
    let mut command = tokio::process::Command::new(exe);
    command
        .arg("--models")
        .arg(models)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()]);
    if let Some(directory) = exe.parent() {
        command.current_dir(directory);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.as_std_mut().creation_flags(0x0800_0000);
    }
    let mut child = command
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Start HOT-Step")?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("http://127.0.0.1:{port}");
    let started = tokio::time::Instant::now();
    loop {
        ensure!(!stop.load(Ordering::SeqCst), "Stopped.");
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("HOT-Step exited while starting ({status}).");
        }
        if let Ok(reply) = http
            .get(format!("{url}{props}"))
            .timeout(Duration::from_secs(1))
            .send()
            .await
        {
            let props = json_response(reply).await?;
            return Ok((child, http, url, props));
        }
        ensure!(
            started.elapsed() < Duration::from_secs(60),
            "HOT-Step did not start within a minute."
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

pub(super) async fn json_response(reply: reqwest::Response) -> Result<serde_json::Value> {
    let status = reply.status();
    let value: serde_json::Value = reply.json().await.context("Read HOT-Step response")?;
    ensure!(
        status.is_success(),
        "HOT-Step request failed ({status}): {value}"
    );
    Ok(value)
}
