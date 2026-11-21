#[tokio::main]
async fn main() -> anyhow::Result<()> {
    music_server::run_ace_adapter().await
}
