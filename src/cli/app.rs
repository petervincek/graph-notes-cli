use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde_json::json;
use sqlx::{Pool, Sqlite};

use crate::db::{
    connection::PoolError,
    links::{LinkService, LinkType, NewLink},
    notes::{NewNote, NoteService},
};

pub struct App {
    note_service: NoteService,
    link_service: LinkService,
}

#[derive(Debug, Parser)]
#[command(name = "graph-notes-cli")]
#[command(about = "Terminal application for managing graph notes")]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create a new graph note
    Create { title: String, content: String },
    /// Read a graph note by provided id
    Read { id: i64 },
    /// Update a existing graph note by id
    Update {
        id: i64,
        title: String,
        content: String,
    },
    /// Delete/Remove a graph note by id
    Delete { id: i64 },
    /// Link to graph notes
    Link {
        from_note_id: i64,
        to_note_id: i64,
        link_type: LinkType,
    },
}

impl App {
    pub async fn create(pool: Arc<Pool<Sqlite>>) -> Result<Self, PoolError> {
        let app = App {
            note_service: NoteService::create(pool.clone()),
            link_service: LinkService::create(pool.clone()),
        };
        Ok(app)
    }

    async fn run_with_args(&self, args: Args) {
        match args.command {
            Commands::Create { title, content } => {
                log::info!(
                    "Calling create with title: {:?}, content: {:?}",
                    title,
                    content
                );
                let created_note = self
                    .note_service
                    .create_note(NewNote {
                        title,
                        content,
                        metadata: json!({}),
                    })
                    .await
                    .expect("Expecting to create a note");
                log::info!("Created note with id: {:?}", created_note.id);
            }
            Commands::Read { id } => {
                log::info!("Reading/Fetching graph note with id: {:?}", id);
                let note = self
                    .note_service
                    .get_note_by_id(id)
                    .await
                    .expect("Expecting to get note");
                log::info!("Fetched note: {:?}", note);
            }
            Commands::Update { id, title, content } => {
                log::info!(
                    "Updating graph note with id: {:?} and title: {:?}, content: {:?}",
                    id,
                    title,
                    content
                );
                let existing_note = self
                    .note_service
                    .get_note_by_id(id)
                    .await
                    .expect("Expecting note to exist");
                // use the existing note for update purpose
                let mut note_to_update = existing_note;
                note_to_update.title = title;
                note_to_update.content = content;
                let updated_note = self
                    .note_service
                    .update_note(note_to_update)
                    .await
                    .expect("Expecting existing note to be updated");
                log::info!("Updated note with id: {:?}", updated_note.id);
            }
            Commands::Delete { id } => {
                log::info!("Deleting/Removing graph note with id: {:?}", id);
                // by deleting the note we delete also the related links to this node (DELETE CASCADE)
                self.note_service
                    .delete_note_by_id(id)
                    .await
                    .expect("Expecting to delete a note");
                log::info!("Note with id: {:?} deleted successfully", id);
            }
            Commands::Link {
                from_note_id,
                to_note_id,
                link_type,
            } => {
                log::info!(
                    "Linking two graph notes, from_note_id: {:?} -> {:?} -> to_note_id: {:?}",
                    from_note_id,
                    link_type,
                    to_note_id
                );
                let created_link = self
                    .link_service
                    .create_link(NewLink {
                        from_note_id,
                        to_note_id,
                        link_type,
                    })
                    .await
                    .expect("Expecting to create a link between notes");
                log::info!(
                    "Graph notes linked successfully, from_note_id: {:?} -> {:?} -> to_note_id: {:?}",
                    created_link.from_note_id,
                    created_link.link_type,
                    created_link.to_note_id
                );
            }
        }
    }

    pub async fn run(&self) {
        self.run_with_args(Args::parse()).await;
    }
}

#[cfg(test)]
mod tests {
    use crate::db::connection;
    use crate::db::links::Link;
    use crate::db::links::LinkServiceError;
    use crate::db::notes::Note;
    use crate::db::notes::NoteServiceError;

    use super::*;
    use clap::CommandFactory;
    use clap::Parser;
    use futures_util::future::FutureExt;
    use rstest::rstest;
    use sqlx::SqlitePool;

    struct AppWithPool {
        app: App,
        pool: Arc<Pool<Sqlite>>,
    }

    async fn create_sut() -> Result<AppWithPool, PoolError> {
        let pool = Arc::new(SqlitePool::connect(":memory:").await?);
        connection::run_migrations(pool.clone()).await?;
        let app = App::create(pool.clone()).await?;
        Ok(AppWithPool { app, pool })
    }

    #[tokio::test]
    async fn test_cli_help_message() -> Result<(), PoolError> {
        let mut cmd = Args::command();
        let mut help_buf = Vec::new();
        cmd.write_long_help(&mut help_buf).unwrap();
        let help_str = String::from_utf8(help_buf).unwrap();
        assert!(help_str.contains("Terminal application for managing graph notes"));
        assert!(help_str.contains("Usage: graph-notes-cli <COMMAND>"));
        assert!(help_str.contains("Commands:"));
        assert!(help_str.contains("create  Create a new graph note"));
        assert!(help_str.contains("read    Read a graph note by provided id"));
        assert!(help_str.contains("update  Update a existing graph note by id"));
        assert!(help_str.contains("delete  Delete/Remove a graph note by id"));
        assert!(help_str.contains("link    Link to graph notes"));
        assert!(
            help_str.contains("help    Print this message or the help of the given subcommand(s)")
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_create_note() -> Result<(), NoteServiceError> {
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
        app.run_with_args(Args::parse_from(args)).await;
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
    async fn test_update_note() -> Result<(), NoteServiceError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());

        // Create a note first
        let create_args = vec![
            "graph-notes-cli",
            "create",
            "Original Title",
            "Original Content",
        ];
        app.run_with_args(Args::parse_from(create_args)).await;
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
        app.run_with_args(Args::parse_from(update_args)).await;

        // Verify update
        let updated = note_service.get_note_by_id(note_id).await?;
        assert_eq!(updated.title, "Updated Title");
        assert_eq!(updated.content, "Updated Content");
        Ok(())
    }

    #[tokio::test]
    async fn test_delete_note() -> Result<(), NoteServiceError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());

        // Create a note first
        let create_args = vec!["graph-notes-cli", "create", "Title", "Content"];
        app.run_with_args(Args::parse_from(create_args)).await;
        let notes = note_service.list_notes(1, 0).await?;
        assert_eq!(notes.len(), 1);
        let note_id = notes[0].id;
        let note_id_string = note_id.to_string();

        // Delete the note
        let delete_args = vec!["graph-notes-cli", "delete", &note_id_string];
        app.run_with_args(Args::parse_from(delete_args)).await;

        // Verify deletion
        let notes_after = note_service.list_notes(1, 0).await?;
        assert_eq!(notes_after.len(), 0, "Expecting no notes after deletion");
        Ok(())
    }

    #[tokio::test]
    async fn test_read_note() -> Result<(), NoteServiceError> {
        let AppWithPool { app, pool } = create_sut().await?;
        let note_service = NoteService::create(pool.clone());

        // Create a note first
        let create_args = vec!["graph-notes-cli", "create", "Read Title", "Read Content"];
        app.run_with_args(Args::parse_from(create_args)).await;
        let notes = note_service.list_notes(1, 0).await?;
        assert_eq!(notes.len(), 1);
        let note_id = notes[0].id;
        let note_id_string = note_id.to_string();

        // Read the note via CLI (should not panic)
        let read_args = vec!["graph-notes-cli", "read", &note_id_string];
        app.run_with_args(Args::parse_from(read_args)).await;

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
    ) -> Result<(), LinkServiceError> {
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
        app.run_with_args(Args::parse_from(link_args)).await;

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

        // The CLI will panic on error, so we catch the panic
        let result = std::panic::AssertUnwindSafe(app.run_with_args(Args::parse_from(link_args)))
            .catch_unwind()
            .await;

        assert!(
            result.is_err(),
            "Expected panic/error when linking to nonexistent note"
        );

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
