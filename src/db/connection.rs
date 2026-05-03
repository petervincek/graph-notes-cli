//! Database connection pool management.
//!
//! This module provides a singleton pattern for managing SQLite database connections
//! using a thread-safe connection pool with lazy initialization and automatic migration support.

use std::env::{self, VarError};
use std::sync::Arc;

use once_cell::sync::OnceCell;
use sqlx::migrate::MigrateError;
use sqlx::{Pool, Sqlite, SqlitePool};
use thiserror::Error;
use tokio::sync::Mutex;

/// Lazily initialized SQLite connection pool wrapped in Arc for thread-safe sharing.
static DB_POOL: OnceCell<Arc<SqlitePool>> = OnceCell::new();

/// Lock used to synchronize pool initialization to ensure only one thread initializes it.
static INIT_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Error)]
pub enum PoolError {
    #[error("Pool can not be created, error: {0}")]
    PoolNotCreated(#[from] sqlx::Error),
    #[error("Pool can not be created due to missing config, error: {0}")]
    MissingConfig(#[from] VarError),
    #[error("Can not load config, error: {0}")]
    ConfigError(#[from] dotenvy::Error),
    #[error("Migration run failed, error: {0}")]
    MigrationFailed(#[from] MigrateError),
}

pub type Result<T> = std::result::Result<T, PoolError>;

pub async fn get_db_connection_pool() -> Result<Arc<Pool<Sqlite>>> {
    if let Some(pool) = DB_POOL.get() {
        return Ok(pool.clone());
    }

    let _guard = INIT_LOCK.lock().await;

    dotenvy::dotenv()?;
    let db_url = env::var("GRAPH_NOTES_DB_URL")?;
    let db_pool = SqlitePool::connect(&db_url).await?;
    let pool = Arc::new(db_pool);
    let _ = DB_POOL.set(pool.clone());
    run_migrations(pool.as_ref()).await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &Pool<Sqlite>) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    log::info!("Run migrations scripts successful.");
    Ok(())
}
