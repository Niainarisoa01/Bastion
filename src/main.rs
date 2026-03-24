#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("Starting Bastion Gateway...");

    // Core gateway loop will go here

    Ok(())
}
