use std::{fs, path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use config::ConfigError;
use sqlx::{Pool, Sqlite};
use thiserror::Error;
use tokio::sync::OnceCell;

use crate::{
    config::config::{AppConfig, CliOptions, xdg_config_path},
    db::{
        connection::{Connection, PoolError},
        links::{LinkService, LinkServiceError, LinkType, NewLink},
        notes::{NewNote, NoteService, NoteServiceError},
    },
    logger::logger::setup_logger,
};

/// Lazily initialized Connection wrapped in Arc for thread-safe sharing.
static CONNECTION: once_cell::sync::OnceCell<Arc<Connection>> = once_cell::sync::OnceCell::new();
pub struct App {
    pool: OnceCell<Arc<Pool<Sqlite>>>,
    note_service: OnceCell<NoteService>,
    link_service: OnceCell<LinkService>,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Application note error: {0}")]
    AppNoteError(#[from] NoteServiceError),
    #[error("Application link error: {0}")]
    AppLinkError(#[from] LinkServiceError),
    #[error("Application pool error: {0}")]
    AppPoolError(#[from] PoolError),
    #[error("Application config error: {0}")]
    AppConfigError(#[from] ConfigError),
    #[error("Serialization error: {0}")]
    AppSerializationError(#[from] serde_json::Error),
    #[error("Application IO error: {0}")]
    AppIoError(#[from] std::io::Error),
}

#[derive(Debug, Parser)]
#[command(author = "Peter Vincek")]
#[command(name = "graph-notes-cli", version)]
#[command(about = "Terminal application for managing graph notes")]
pub struct Args {
    /// Optional config file path (overrides default config discovery)
    #[arg(long)]
    pub config: Option<String>,

    /// Optional database URL (overrides config/env)
    #[arg(long)]
    pub db_url: Option<String>,

    /// Optional log level (overrides config/env)
    #[arg(long)]
    pub log_level: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create a new graph note
    Create {
        title: String,
        content: String,
        #[arg(long)]
        metadata: Option<String>,
    },
    /// Read a graph note by provided id
    Read { id: i64 },
    /// Update a existing graph note by id
    Update {
        id: i64,
        title: String,
        content: String,
        #[arg(long)]
        metadata: Option<String>,
    },
    /// Delete/Remove a graph note by id
    Delete { id: i64 },
    /// Link to graph notes
    Link {
        from_note_id: i64,
        to_note_id: i64,
        link_type: LinkType,
    },
    /// Initialize a default configuration file in the standard config directory
    InitConfig,
}

impl App {
    pub fn create(option_pool: Option<Arc<Pool<Sqlite>>>) -> Self {
        if let Some(pool) = option_pool {
            // if there is a pool injected through the creator function, then use it
            // this can be used to control the dependency in automated test environment
            Self {
                pool: OnceCell::from(pool),
                note_service: OnceCell::new(),
                link_service: OnceCell::new(),
            }
        } else {
            // if there is no pool injected, create the pool from config values
            Self {
                pool: OnceCell::new(),
                note_service: OnceCell::new(),
                link_service: OnceCell::new(),
            }
        }
    }

    async fn get_pool(&self) -> Result<Arc<Pool<Sqlite>>, PoolError> {
        self.pool
            .get_or_try_init(|| async {
                match CONNECTION.get() {
                    Some(connection) => connection.get_db_connection_pool().await,
                    None => Err(PoolError::ConnectionNotInitialized()),
                }
            })
            .await
            .map(Clone::clone)
    }

    // internal getter function that will take the advantage of the lazy loading
    async fn get_note_service(&self) -> Result<&NoteService, PoolError> {
        self.note_service
            .get_or_try_init(|| async {
                let pool_result = self.get_pool().await;
                match pool_result {
                    Ok(pool) => Ok(NoteService::create(pool)),
                    Err(error) => Err(error),
                }
            })
            .await
    }

    // internal getter function that will take the advantage of the lazy loading
    async fn get_link_service(&self) -> Result<&LinkService, PoolError> {
        self.link_service
            .get_or_try_init(|| async {
                let pool_result = self.get_pool().await;
                match pool_result {
                    Ok(pool) => Ok(LinkService::create(pool)),
                    Err(error) => Err(error),
                }
            })
            .await
    }

    async fn run_with_args(&self, args: Args) -> Result<(), AppError> {
        match args.command {
            Commands::Create {
                title,
                content,
                metadata,
            } => {
                log::debug!(
                    "Calling create with title: {:?}, content: {:?}",
                    title,
                    content
                );
                let parsed_metadata = if let Some(possible_metadata) = metadata {
                    serde_json::from_str(&possible_metadata)?
                } else {
                    serde_json::Value::Null
                };
                let created_note = self
                    .get_note_service()
                    .await?
                    .create_note(NewNote {
                        title,
                        content,
                        metadata: parsed_metadata,
                    })
                    .await?;
                log::debug!("Created note with id: {:?}", created_note.id);
                println!("{}", serde_json::to_string_pretty(&created_note)?);
                Ok(())
            }
            Commands::Read { id } => {
                log::debug!("Reading/Fetching graph note with id: {:?}", id);
                let note = self.get_note_service().await?.get_note_by_id(id).await?;
                log::debug!("Fetched note: {:?}", note);
                println!("{}", serde_json::to_string_pretty(&note)?);
                Ok(())
            }
            Commands::Update {
                id,
                title,
                content,
                metadata,
            } => {
                log::debug!(
                    "Updating graph note with id: {:?} and title: {:?}, content: {:?}, metadata: {:?}",
                    id,
                    title,
                    content,
                    metadata
                );
                let existing_note = self.get_note_service().await?.get_note_by_id(id).await?;

                // use the existing note for update purpose
                let mut note_to_update = existing_note;
                note_to_update.title = title;
                note_to_update.content = content;
                if let Some(possible_metadata) = metadata {
                    note_to_update.metadata = serde_json::from_str(&possible_metadata)?;
                }
                let updated_note = self
                    .get_note_service()
                    .await?
                    .update_note(note_to_update)
                    .await?;
                log::debug!("Updated note with id: {:?}", updated_note.id);
                println!("{}", serde_json::to_string_pretty(&updated_note)?);
                Ok(())
            }
            Commands::Delete { id } => {
                log::debug!("Deleting/Removing graph note with id: {:?}", id);
                // by deleting the note we delete also the related links to this node (DELETE CASCADE)
                self.get_note_service().await?.delete_note_by_id(id).await?;
                log::debug!("Note with id: {:?} deleted successfully", id);
                println!("Note with id: {} deleted.", id);
                Ok(())
            }
            Commands::Link {
                from_note_id,
                to_note_id,
                link_type,
            } => {
                log::debug!(
                    "Linking two graph notes, from_note_id: {:?} -> {:?} -> to_note_id: {:?}",
                    from_note_id,
                    link_type,
                    to_note_id
                );
                let created_link = self
                    .get_link_service()
                    .await?
                    .create_link(NewLink {
                        from_note_id,
                        to_note_id,
                        link_type,
                    })
                    .await?;
                log::debug!(
                    "Graph notes linked successfully, from_note_id: {:?} -> {:?} -> to_note_id: {:?}",
                    created_link.from_note_id,
                    created_link.link_type,
                    created_link.to_note_id
                );
                println!("{}", serde_json::to_string_pretty(&created_link)?);
                Ok(())
            }
            Commands::InitConfig => {
                let config_dir = xdg_config_path();
                let config_path = config_dir.join("config.toml");
                if config_path.exists() {
                    println!("Config file already exists at: {}", config_path.display());
                } else {
                    fs::create_dir_all(&config_dir)?;
                    // Provide your default config content here
                    let default_config = r#"
db_url = "sqlite://notes.db"
log_level = "info"
"#;
                    fs::write(&config_path, default_config)?;
                    println!("Default config created at: {}", config_path.display());
                }
                Ok(())
            }
        }
    }

    pub async fn run(&self) -> Result<(), AppError> {
        // parse the command line arguments
        let args = Args::parse();
        let config_path = args.config.as_ref().map(|s| PathBuf::from(s));
        // provide the possible config overrides from command line arguments
        let config = AppConfig::from_sources(
            config_path,
            CliOptions {
                db_url: args.db_url.clone(),
                log_level: args.log_level.clone(),
            },
        )?;
        setup_logger(&config.log_level);
        log::debug!("Config: {:?}", config);
        let connection = Arc::new(Connection { config: config });
        CONNECTION.set(connection).map_err(|_| {
            ConfigError::Message("global connection has already been initialized".to_string())
        })?;
        self.run_with_args(args).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::connection;
    use crate::db::connection::PoolError;
    use crate::db::links::Link;
    use crate::db::links::LinkServiceError;
    use crate::db::notes::Note;

    use super::*;
    use clap::CommandFactory;
    use clap::Parser;
    use rstest::rstest;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    struct AppWithPool {
        app: App,
        pool: Arc<Pool<Sqlite>>,
    }

    async fn create_sut() -> Result<AppWithPool, PoolError> {
        let pool = Arc::new(
            SqlitePoolOptions::new()
                .max_connections(1)
                .after_connect(connection::enable_sqlite_foreign_keys())
                .connect(":memory:")
                .await?,
        );
        connection::run_migrations(pool.clone()).await?;
        let app = App::create(Some(pool.clone()));
        Ok(AppWithPool { app, pool })
    }

    #[tokio::test]
    async fn test_cli_help_message() -> Result<(), PoolError> {
        let mut cmd = Args::command();
        let mut help_buf = Vec::new();
        cmd.write_long_help(&mut help_buf).unwrap();
        let help_str = String::from_utf8(help_buf).unwrap();
        assert!(help_str.contains("Terminal application for managing graph notes"));
        assert!(help_str.contains("Usage: graph-notes-cli [OPTIONS] <COMMAND>"));
        assert!(help_str.contains("Commands:"));
        assert!(help_str.contains("create       Create a new graph note"));
        assert!(help_str.contains("read         Read a graph note by provided id"));
        assert!(help_str.contains("update       Update a existing graph note by id"));
        assert!(help_str.contains("delete       Delete/Remove a graph note by id"));
        assert!(help_str.contains("link         Link to graph notes"));
        assert!(help_str.contains(
            "init-config  Initialize a default configuration file in the standard config directory"
        ));
        assert!(
            help_str
                .contains("help         Print this message or the help of the given subcommand(s)")
        );
        assert!(help_str.contains("Options:"));
        assert!(help_str.contains("      --config <CONFIG>"));
        assert!(
            help_str
                .contains("        Optional config file path (overrides default config discovery)")
        );
        assert!(help_str.contains("      --db-url <DB_URL>"));
        assert!(help_str.contains("          Optional database URL (overrides config/env)"));
        assert!(help_str.contains("      --log-level <LOG_LEVEL>"));
        assert!(help_str.contains("          Optional log level (overrides config/env)"));
        assert!(help_str.contains("  -h, --help"));
        assert!(help_str.contains("          Print help"));
        assert!(help_str.contains("  -V, --version"));
        assert!(help_str.contains("          Print version"));

        Ok(())
    }

    #[tokio::test]
    async fn test_create_note() -> Result<(), AppError> {
        let args = vec!["graph-notes-cli", "create", "Test Title", "Test Content"];
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool);

        // exercise, verify
        // check the state of the database before the action
        assert_eq!(
            note_service.list_notes(1, 0).await?.len(),
            0,
            "Expecting no notes in the fresh db"
        );
        // run the CLI tool
        app.run_with_args(Args::parse_from(args)).await?;
        // check the state of the database after the action
        let result = note_service.list_notes(2, 0).await?;
        assert_eq!(
            result.len(),
            1,
            "Expecting exactly one created note in the db"
        );
        let Note {
            id: _,
            title,
            content,
            metadata: _,
            created_at: _,
        } = &result[0];
        assert_eq!(
            title, "Test Title",
            "Expecting the persisted title to match the provided value"
        );
        assert_eq!(
            content, "Test Content",
            "Expecting the persisted content to match the provided value"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_update_note() -> Result<(), AppError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());

        // Create a note first
        let create_args = vec![
            "graph-notes-cli",
            "create",
            "Original Title",
            "Original Content",
        ];
        app.run_with_args(Args::parse_from(create_args)).await?;
        let notes = note_service.list_notes(1, 0).await?;
        assert_eq!(notes.len(), 1);
        let note_id = notes[0].id;
        let note_id_string = note_id.to_string();

        // Update the note
        let update_args = vec![
            "graph-notes-cli",
            "update",
            &note_id_string,
            "Updated Title",
            "Updated Content",
        ];
        app.run_with_args(Args::parse_from(update_args)).await?;

        // Verify update
        let updated = note_service.get_note_by_id(note_id).await?;
        assert_eq!(updated.title, "Updated Title");
        assert_eq!(updated.content, "Updated Content");
        Ok(())
    }

    #[tokio::test]
    async fn test_delete_note() -> Result<(), AppError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());

        // Create a note first
        let create_args = vec!["graph-notes-cli", "create", "Title", "Content"];
        app.run_with_args(Args::parse_from(create_args)).await?;
        let notes = note_service.list_notes(1, 0).await?;
        assert_eq!(notes.len(), 1);
        let note_id = notes[0].id;
        let note_id_string = note_id.to_string();

        // Delete the note
        let delete_args = vec!["graph-notes-cli", "delete", &note_id_string];
        app.run_with_args(Args::parse_from(delete_args)).await?;

        // Verify deletion
        let notes_after = note_service.list_notes(1, 0).await?;
        assert_eq!(notes_after.len(), 0, "Expecting no notes after deletion");
        Ok(())
    }

    #[tokio::test]
    async fn test_read_note() -> Result<(), AppError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());

        // Create a note first
        let create_args = vec!["graph-notes-cli", "create", "Read Title", "Read Content"];
        app.run_with_args(Args::parse_from(create_args)).await?;
        let notes = note_service.list_notes(1, 0).await?;
        assert_eq!(notes.len(), 1);
        let note_id = notes[0].id;
        let note_id_string = note_id.to_string();

        // Read the note via CLI (should not panic)
        let read_args = vec!["graph-notes-cli", "read", &note_id_string];
        app.run_with_args(Args::parse_from(read_args)).await?;

        // Optionally, verify the note still exists and is unchanged
        let note = note_service.get_note_by_id(note_id).await?;
        assert_eq!(note.title, "Read Title");
        assert_eq!(note.content, "Read Content");
        Ok(())
    }

    #[tokio::test]
    #[rstest]
    #[case::related_link_type("related", LinkType::Related)]
    #[case::related_link_type("reference", LinkType::Reference)]
    #[case::related_link_type("parent", LinkType::Parent)]
    #[case::related_link_type("child", LinkType::Child)]
    async fn test_link_notes(
        #[case] link_type: String,
        #[case] expected_link_type: LinkType,
    ) -> Result<(), AppError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());

        // Create two notes
        let created_from_note = note_service
            .create_note(NewNote {
                title: String::from("From Note"),
                content: String::from("from content"),
                metadata: json!({}),
            })
            .await?;
        let created_from_note_id = created_from_note.id;
        let created_from_note_id_string = created_from_note_id.to_string();
        let created_to_note = note_service
            .create_note(NewNote {
                title: String::from("To Note"),
                content: String::from("to content"),
                metadata: json!({}),
            })
            .await?;
        let created_to_note_id = created_to_note.id;
        let created_to_note_id_string = created_to_note_id.to_string();

        // Link them
        let link_args = vec![
            "graph-notes-cli",
            "link",
            &created_from_note_id_string,
            &created_to_note_id_string,
            &link_type,
        ];
        app.run_with_args(Args::parse_from(link_args)).await?;

        // Verify link exists
        let links = link_service.list_links(2, 0).await?;
        assert_eq!(links.len(), 1, "Should have one link");
        let Link {
            from_note_id,
            to_note_id,
            link_type,
            ..
        } = &links[0];
        assert_eq!(*from_note_id, created_from_note_id);
        assert_eq!(*to_note_id, created_to_note_id);
        assert_eq!(*link_type, expected_link_type);
        Ok(())
    }

    #[tokio::test]
    async fn test_link_nonexistent_notes_should_fail() -> Result<(), LinkServiceError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());

        // Create only one note
        let created_note = note_service
            .create_note(NewNote {
                title: String::from("Only Note"),
                content: String::from("content"),
                metadata: json!({}),
            })
            .await?;
        let valid_id = created_note.id;
        let valid_id_string = valid_id.to_string();
        let invalid_id = 9999; // This ID does not exist
        let invalid_id_string = invalid_id.to_string();

        // Try to link valid -> invalid (should fail)
        let link_args = vec![
            "graph-notes-cli",
            "link",
            &valid_id_string,
            &invalid_id_string,
            "reference",
        ];

        let result = app.run_with_args(Args::parse_from(link_args)).await;
        assert!(
            result.is_err(),
            "Expected error when linking to nonexistent note"
        );
        assert!(matches!(result, Err(AppError::AppLinkError(_))));

        // Ensure no links were created
        let links = link_service.list_links(1, 0).await?;
        assert_eq!(links.len(), 0, "No links should be created on error");

        Ok(())
    }

    #[test]
    fn test_cli_help() {
        Args::command().debug_assert();
    }
}
