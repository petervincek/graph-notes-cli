use anyhow::Result;
use graph_notes_cli::cli;

#[tokio::main]
async fn main() -> Result<()> {
    // create the instance of the App and run it
    let app = cli::app::App::create(None);
    app.run().await?;
    Ok(())
}
