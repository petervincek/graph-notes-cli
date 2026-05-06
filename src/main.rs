use anyhow::Result;
use graph_notes_cli::cli;
use graph_notes_cli::db::connection;
use graph_notes_cli::logger::logger;

#[tokio::main]
async fn main() -> Result<()> {
    logger::setup_logger();
    log::info!("Graph Notes CLI");

    // create the instance of the App and run it
    let app = cli::app::App::create(connection::get_db_connection_pool().await?).await?;
    app.run().await;

    Ok(())
}
