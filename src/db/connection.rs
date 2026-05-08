//! Database connection pool management.
//!
//! This module provides a singleton pattern for managing SQLite database connections
//! using a thread-safe connection pool with lazy initialization and automatic migration support.

use std::env::VarError;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use sqlx::migrate::MigrateError;
use sqlx::pool::PoolConnectionMetadata;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{ConnectOptions, Pool, Sqlite, SqliteConnection, SqlitePool};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::config::config;

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
    #[error("Connection not initialized")]
    ConnectionNotInitialized(),
}

pub struct Connection {
    pub config: config::AppConfig,
}

pub type Result<T> = std::result::Result<T, PoolError>;

impl Connection {
    pub async fn get_db_connection_pool(&self) -> Result<Arc<Pool<Sqlite>>> {
        let _guard = INIT_LOCK.lock().await;

        if let Some(pool) = DB_POOL.get() {
            return Ok(pool.clone());
        }

        dotenvy::dotenv().ok();
        // let db_url = env::var("GRAPH_NOTES_DB_URL")?;
        let db_url = &self.config.db_url;
        // IMPORTANT: SQLite does NOT enforce foreign key constraints (including ON DELETE CASCADE)
        // unless PRAGMA foreign_keys = ON is set for every new connection. This is required even if
        // your schema defines foreign keys. The after_connect hook below ensures that foreign key
        // enforcement is enabled for every pooled connection, so that referential integrity and
        // cascade behaviors work as expected throughout the application.
        let mut options = SqliteConnectOptions::new()
            .filename(&db_url.replace("sqlite://", ""))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        // set the logging
        options = options.log_statements(log::LevelFilter::Off);
        let db_pool = SqlitePoolOptions::new()
            .after_connect(enable_sqlite_foreign_keys())
            .connect_with(options)
            .await?;
        let pool = Arc::new(db_pool);
        match DB_POOL.set(pool.clone()) {
            Ok(()) => {
                // first time we set the pool, we need to run the migration
                run_migrations(pool.clone()).await?;
                Ok(pool)
            }
            // if I get a error while trying to set the pool, it means that the pool was already set
            Err(_) => Ok(DB_POOL.get().cloned().unwrap_or(pool)),
        }
    }
}

pub async fn run_migrations(pool: Arc<Pool<Sqlite>>) -> Result<()> {
    sqlx::migrate!("./migrations").run(&*pool).await?;
    log::info!("Run migrations scripts successful.");
    Ok(())
}

// Define a reusable callback for after_connect
pub fn enable_sqlite_foreign_keys() -> Box<
    dyn for<'c> Fn(
            &'c mut SqliteConnection,
            PoolConnectionMetadata,
        ) -> Pin<Box<dyn Future<Output = sqlx::Result<()>> + Send + 'c>>
        + Send
        + Sync,
> {
    Box::new(|conn, _meta| {
        Box::pin(async move {
            sqlx::query("PRAGMA foreign_keys = ON;")
                .execute(conn)
                .await?;
            Ok(())
        })
    })
}
