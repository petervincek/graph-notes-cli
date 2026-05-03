use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite, prelude::FromRow};
use thiserror::Error;

use crate::db::connection::PoolError;

/// Represents a note stored in the database.
#[derive(Debug, Serialize, Deserialize, FromRow, PartialEq)]
pub struct Note {
    /// Unique identifier for the note (auto-incremented).
    pub id: i64,
    /// Title of the note (must be unique and non-empty).
    pub title: String,
    /// Content/body of the note.
    pub content: String,
    /// Arbitrary metadata stored as JSON.
    pub metadata: serde_json::Value,
    /// Timestamp when the note was created.
    pub created_at: chrono::NaiveDateTime,
}

/// Data required to create a new note.
#[derive(Debug, Serialize, Deserialize)]
pub struct NewNote {
    /// Title of the note (must be unique and non-empty).
    pub title: String,
    /// Content/body of the note.
    pub content: String,
    /// Arbitrary metadata stored as JSON.
    pub metadata: serde_json::Value,
}

impl NewNote {
    /// Validates the fields of the new note.
    ///
    /// Returns an error if the title or content is empty.
    fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(NoteServiceError::Validation("Title cannot be empty".into()));
        }
        if self.content.trim().is_empty() {
            return Err(NoteServiceError::Validation(
                "Content cannot be empty".into(),
            ));
        }
        Ok(())
    }
}

/// Service for managing notes in the database.
pub struct NoteService<'a> {
    /// Reference to the SQLite connection pool.
    pool: &'a Pool<Sqlite>,
}

/// Errors that can occur when working with notes.
#[derive(Debug, Error)]
pub enum NoteServiceError {
    /// Error from the database layer.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    /// Error from migration or pool setup.
    #[error("Migration error: {0}")]
    Migrations(#[from] PoolError),
    /// Error during (de)serialization of metadata.
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    /// Returned when a note is not found in the database.
    #[error("Note not found")]
    NotFound,
    /// Returned when validation of input data fails.
    #[error("Validation error: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, NoteServiceError>;

impl<'a> NoteService<'a> {
    /// Creates a new NoteService with a reference to the database pool.
    pub fn create(pool: &'a Pool<Sqlite>) -> Self {
        NoteService { pool: pool }
    }

    /// Inserts a new note into the database after validating the input.
    ///
    /// Returns the created note with its generated id and timestamp.
    pub async fn create_note(&self, new_note: NewNote) -> Result<Note> {
        new_note.validate()?;
        let created_note = sqlx::query_as::<_, Note>(
            r#"
            INSERT INTO notes (title, content, metadata)
            VALUES (?1, ?2, ?3)
            RETURNING id, title, content, metadata, created_at
            "#,
        )
        .bind(&new_note.title)
        .bind(&new_note.content)
        .bind(&new_note.metadata)
        .fetch_one(self.pool)
        .await?;
        Ok(created_note)
    }

    /// Retrieves a note by its id.
    ///
    /// Returns an error if the note does not exist.
    pub async fn get_note_by_id(&self, id: i64) -> Result<Note> {
        let maybe_note = sqlx::query_as::<_, Note>(
            r#"
            SELECT id, title, content, metadata, created_at
            FROM notes
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;
        match maybe_note {
            Some(note) => Ok(note),
            None => Err(NoteServiceError::NotFound),
        }
    }

    /// Fetches a paginated list of notes from the database.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of notes to return.
    /// * `offset` - Number of notes to skip (for pagination).
    pub async fn list_notes(&self, limit: i64, offset: i64) -> Result<Vec<Note>> {
        let notes = sqlx::query_as::<_, Note>(
            r#"
        SELECT id, title, content, metadata, created_at
        FROM notes
        ORDER BY created_at DESC
        LIMIT ?1 OFFSET ?2
        "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await?;
        Ok(notes)
    }

    /// Updates an existing note in the database.
    ///
    /// Returns the updated note, or an error if the note does not exist.
    pub async fn update_note(&self, note: Note) -> Result<Note> {
        let maybe_note = sqlx::query_as::<_, Note>(
            r#"
            UPDATE notes
            SET title = ?1, content = ?2, metadata = ?3
            WHERE id = ?4
            RETURNING id, title, content, metadata, created_at
            "#,
        )
        .bind(&note.title)
        .bind(&note.content)
        .bind(&note.metadata)
        .bind(note.id)
        .fetch_optional(self.pool)
        .await?;
        match maybe_note {
            Some(note) => Ok(note),
            None => Err(NoteServiceError::NotFound),
        }
    }

    /// Deletes a note by its id.
    ///
    /// Returns an error if the note does not exist.
    pub async fn delete_note_by_id(&self, id: i64) -> Result<()> {
        let result = sqlx::query(
            r#"
            DELETE FROM notes
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .execute(self.pool)
        .await?;
        if result.rows_affected() == 0 {
            Err(NoteServiceError::NotFound)
        } else {
            Ok(())
        }
    }
}

/// Unit and integration tests for the NoteService and related logic.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::run_migrations;
    use rstest::rstest;
    use sqlx::SqlitePool;

    #[rstest]
    #[case::both_empty("", "", serde_json::Value::Null, Some("Title cannot be empty"))]
    #[case::content_empty("Title", "", serde_json::Value::Null, Some("Content cannot be empty"))]
    #[case::title_empty("", "Content", serde_json::Value::Null, Some("Title cannot be empty"))]
    #[case::valid("Title", "Content", serde_json::Value::Null, None)]
    fn test_new_note_validation(
        #[case] title: String,
        #[case] content: String,
        #[case] metadata: serde_json::Value,
        #[case] expected_error: Option<&str>,
    ) {
        // create the sut
        let new_note = NewNote {
            title: title,
            content: content,
            metadata: metadata,
        };
        // exercise, verify
        let result = new_note.validate();
        match expected_error {
            Some(expected_error_msg) => match result {
                Err(NoteServiceError::Validation(actual_error_msg)) => {
                    assert_eq!(actual_error_msg, expected_error_msg)
                }
                other => panic!(
                    "Expected validation error msg: {:?}, got: {:?}",
                    expected_error, other
                ),
            },
            None => {
                assert!(result.is_ok(), "Expected Ok(), got: {:?}", result)
            }
        }
    }

    // helper function to setup in-memory database (SQLite) for testing purposes
    async fn setup_test_db() -> Result<SqlitePool> {
        let pool = SqlitePool::connect(":memory:").await?;
        run_migrations(&pool).await?;
        Ok(pool)
    }

    #[tokio::test]
    async fn test_create_and_get_note() -> Result<()> {
        // create sut
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);

        // exercise, verify
        // create part
        let created_note = note_service
            .create_note(NewNote {
                title: String::from("Note Title"),
                content: String::from("note content"),
                metadata: serde_json::Value::Null,
            })
            .await?;
        assert_eq!(created_note.id, 1, "Expected note id to be set");
        // retrieve/fetch part
        let retrieved_note = note_service.get_note_by_id(created_note.id).await?;
        assert_eq!(
            created_note, retrieved_note,
            "Expecting both notes to be equal"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_create_update_get_note() -> Result<()> {
        // create sut
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);

        // exercise, verify
        // create part
        let created_note = note_service
            .create_note(NewNote {
                title: String::from("Note Title"),
                content: String::from("note content"),
                metadata: serde_json::Value::Null,
            })
            .await?;
        assert_eq!(created_note.id, 1, "Expected note id to be set");
        // retrieve/fetch part
        let mut retrieved_note = note_service.get_note_by_id(created_note.id).await?;
        assert_eq!(
            created_note, retrieved_note,
            "Expecting both notes to be equal"
        );
        // update part
        retrieved_note.title = String::from("Updated Title");
        let fetched_updated_note = note_service.update_note(retrieved_note).await?;
        assert_eq!(
            fetched_updated_note.title, "Updated Title",
            "Expecting both notes to be equal"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_create_get_delete_note() -> Result<()> {
        // create sut
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);

        // exercise, verify
        // create part
        let created_note = note_service
            .create_note(NewNote {
                title: String::from("Note Title"),
                content: String::from("note content"),
                metadata: serde_json::Value::Null,
            })
            .await?;
        assert_eq!(created_note.id, 1, "Expected note id to be set");
        // retrieve/fetch part
        let retrieved_note = note_service.get_note_by_id(created_note.id).await?;
        assert_eq!(
            created_note, retrieved_note,
            "Expecting both notes to be equal"
        );
        // delete part
        note_service.delete_note_by_id(retrieved_note.id).await?;
        let result = note_service.get_note_by_id(retrieved_note.id).await;
        match result {
            Err(NoteServiceError::NotFound) => Ok(()),
            other => panic!("Unexpected outcome: {:?}", other),
        }
    }

    // 1. Test for Duplicate Titles (Unique Constraint)
    #[tokio::test]
    async fn test_duplicate_title() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);

        let _note1 = note_service
            .create_note(NewNote {
                title: "UniqueTitle".to_string(),
                content: "First note".to_string(),
                metadata: serde_json::Value::Null,
            })
            .await?;

        let result = note_service
            .create_note(NewNote {
                title: "UniqueTitle".to_string(),
                content: "Second note".to_string(),
                metadata: serde_json::Value::Null,
            })
            .await;

        assert!(
            matches!(result, Err(NoteServiceError::Database(_))),
            "Expected database error for duplicate title"
        );
        Ok(())
    }

    // 2. Test for Pagination
    #[tokio::test]
    async fn test_list_notes_pagination() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);

        // Insert 5 notes
        for i in 0..5 {
            note_service
                .create_note(NewNote {
                    title: format!("Note {}", i),
                    content: format!("Content {}", i),
                    metadata: serde_json::Value::Null,
                })
                .await?;
        }

        let page1 = note_service.list_notes(2, 0).await?;
        let page2 = note_service.list_notes(2, 2).await?;
        let page3 = note_service.list_notes(2, 4).await?;

        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 2);
        assert_eq!(page3.len(), 1);
        Ok(())
    }

    // 3. Test for Metadata Handling
    #[tokio::test]
    async fn test_metadata_handling() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);

        let metadata = serde_json::json!({"tag": "unit", "priority": 1});
        let created_note = note_service
            .create_note(NewNote {
                title: "MetaNote".to_string(),
                content: "Has metadata".to_string(),
                metadata: metadata.clone(),
            })
            .await?;

        let fetched_note = note_service.get_note_by_id(created_note.id).await?;
        assert_eq!(fetched_note.metadata, metadata);
        Ok(())
    }

    // 4. Test for Update Not Found
    #[tokio::test]
    async fn test_update_note_not_found() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);

        let fake_note = Note {
            id: 999,
            title: "Doesn't exist".to_string(),
            content: "No content".to_string(),
            metadata: serde_json::Value::Null,
            created_at: chrono::Utc::now().naive_utc(),
        };
        let result = note_service.update_note(fake_note).await;
        assert!(matches!(result, Err(NoteServiceError::NotFound)));
        Ok(())
    }

    // 5. Test for Delete Not Found
    #[tokio::test]
    async fn test_delete_note_not_found() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);
        let result = note_service.delete_note_by_id(999).await;
        assert!(matches!(result, Err(NoteServiceError::NotFound)));
        Ok(())
    }

    // 6. Test for Validation in create_note
    #[tokio::test]
    async fn test_create_note_validation_error() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);
        let result = note_service
            .create_note(NewNote {
                title: "".to_string(),
                content: "".to_string(),
                metadata: serde_json::Value::Null,
            })
            .await;
        assert!(matches!(result, Err(NoteServiceError::Validation(_))));
        Ok(())
    }
}
