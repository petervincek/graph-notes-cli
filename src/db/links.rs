use std::sync::Arc;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sqlx::{
    Executor, Pool, Sqlite,
    prelude::{FromRow, Type},
    types::chrono,
};
use thiserror::Error;

use crate::db::{
    connection::PoolError,
    notes::{NewNote, NoteService, NoteServiceError},
};

#[derive(Debug, Error)]
pub enum LinkServiceError {
    /// Error from the database layer.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Link not found")]
    NotFound,
    #[error("Migration error: {0}")]
    Migrations(#[from] PoolError),
    #[error("Note service error: {0}")]
    NoteError(#[from] NoteServiceError),
}

pub type Result<T> = std::result::Result<T, LinkServiceError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type, ValueEnum)]
#[sqlx(type_name = "TEXT")] // SQLite uses TEXT for enums
pub enum LinkType {
    #[serde(rename = "reference")]
    #[sqlx(rename = "reference")]
    Reference,
    #[serde(rename = "related")]
    #[sqlx(rename = "related")]
    Related,
    #[serde(rename = "parent")]
    #[sqlx(rename = "parent")]
    Parent,
    #[serde(rename = "child")]
    #[sqlx(rename = "child")]
    Child,
}

#[derive(Debug, Serialize, Deserialize, FromRow, PartialEq)]
pub struct Link {
    pub from_note_id: i64,
    pub to_note_id: i64,
    pub link_type: LinkType,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct NewLink {
    pub from_note_id: i64,
    pub to_note_id: i64,
    pub link_type: LinkType,
}

/// Represents a pair of new notes and the type of link to create between them.
#[derive(Debug)]
pub struct NotesWithLinkType {
    from_note: NewNote,
    to_note: NewNote,
    link_type: LinkType,
}

impl NotesWithLinkType {
    /// Constructs a new `NotesWithLinkType` value.
    ///
    /// # Arguments
    ///
    /// * `from_note` - The source note to be created.
    /// * `to_note` - The target note to be created.
    /// * `link_type` - The type of link to establish between the notes.
    pub fn create(from_note: NewNote, to_note: NewNote, link_type: LinkType) -> Self {
        NotesWithLinkType {
            from_note,
            to_note,
            link_type,
        }
    }
}

#[derive(Debug)]
pub struct LinkService {
    /// Reference to the SQLite connection pool.
    pool: Arc<Pool<Sqlite>>,
    note_service: NoteService,
}

impl LinkService {
    /// Creates a new LinkService with a reference to the database pool.
    pub fn create(pool: Arc<Pool<Sqlite>>) -> Self {
        LinkService {
            pool: pool.clone(),
            note_service: NoteService::create(pool.clone()),
        }
    }

    pub async fn create_link(&self, new_link: NewLink) -> Result<Link> {
        self.create_link_through_executor(new_link, &*self.pool)
            .await
    }

    pub async fn create_link_through_executor<'e, E>(
        &self,
        new_link: NewLink,
        executor: E,
    ) -> Result<Link>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        let created_link = sqlx::query_as::<_, Link>(
            r#"
            INSERT INTO links (from_note_id, to_note_id, link_type)
            VALUES (?1, ?2, ?3)
            RETURNING from_note_id, to_note_id, link_type, created_at
            "#,
        )
        .bind(new_link.from_note_id)
        .bind(new_link.to_note_id)
        .bind(new_link.link_type)
        .fetch_one(executor)
        .await?;
        Ok(created_link)
    }

    pub async fn list_links(&self, limit: i64, offset: i64) -> Result<Vec<Link>> {
        self.list_links_through_executor(limit, offset, &*self.pool)
            .await
    }

    /// Fetches a paginated list of links from the database.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of notes to return.
    /// * `offset` - Number of notes to skip (for pagination).
    pub async fn list_links_through_executor<'e, E>(
        &self,
        limit: i64,
        offset: i64,
        executor: E,
    ) -> Result<Vec<Link>>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        let links = sqlx::query_as::<_, Link>(
            r#"
        SELECT from_note_id, to_note_id, link_type, created_at 
        FROM links
        ORDER BY created_at DESC
        LIMIT ?1 OFFSET ?2
        "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(executor)
        .await?;
        Ok(links)
    }

    pub async fn update_link(&self, updated_link: Link) -> Result<Link> {
        self.update_link_through_executor(updated_link, &*self.pool)
            .await
    }

    pub async fn update_link_through_executor<'e, E>(
        &self,
        updated_link: Link,
        executor: E,
    ) -> Result<Link>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        let maybe_link = sqlx::query_as::<_, Link>(
            r#"
            UPDATE links
            SET link_type = ?3
            WHERE from_note_id = ?1 AND to_note_id = ?2
            RETURNING from_note_id, to_note_id, link_type, created_at
            "#,
        )
        .bind(updated_link.from_note_id)
        .bind(updated_link.to_note_id)
        .bind(updated_link.link_type)
        .fetch_optional(executor)
        .await?;
        match maybe_link {
            Some(link) => Ok(link),
            None => Err(LinkServiceError::NotFound),
        }
    }

    pub async fn delete_link(&self, link_to_delete: Link) -> Result<()> {
        self.delete_link_through_executor(link_to_delete, &*self.pool)
            .await
    }

    pub async fn delete_link_through_executor<'e, E>(
        &self,
        link_to_delete: Link,
        executor: E,
    ) -> Result<()>
    where
        E: Executor<'e, Database = Sqlite>,
    {
        let result = sqlx::query(
            r#"
            DELETE FROM links
            WHERE from_note_id = ?1 AND to_note_id = ?2
            "#,
        )
        .bind(link_to_delete.from_note_id)
        .bind(link_to_delete.to_note_id)
        .execute(executor)
        .await?;
        if result.rows_affected() == 0 {
            Err(LinkServiceError::NotFound)
        } else {
            Ok(())
        }
    }

    pub async fn create_notes_with_relationship(
        &self,
        notes_with_link_types: Vec<NotesWithLinkType>,
    ) -> Result<Vec<Link>> {
        // first define a new transaction
        let mut tx = self.pool.begin().await?;
        let mut created_links: Vec<Link> = vec![];

        for notes_with_link_type in notes_with_link_types {
            // deconstruct the partial data
            let NotesWithLinkType {
                from_note,
                to_note,
                link_type,
            } = notes_with_link_type;

            let from_note_created = self
                .note_service
                .create_note_through_executor(from_note, &mut *tx)
                .await?;
            let to_note_created = self
                .note_service
                .create_note_through_executor(to_note, &mut *tx)
                .await?;
            let link_created = self
                .create_link_through_executor(
                    NewLink {
                        from_note_id: from_note_created.id,
                        to_note_id: to_note_created.id,
                        link_type,
                    },
                    &mut *tx,
                )
                .await?;
            // collect that link with the accumulator
            created_links.push(link_created);
        }
        // commit the transaction
        tx.commit().await?;

        Ok(created_links)
    }
}

/// Unit and integration tests for the LinkService and related logic.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        connection::run_migrations,
        notes::{NewNote, NoteService},
    };
    use serde_json::json;
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

    // helper function to setup in-memory database (SQLite) for testing purposes
    async fn setup_test_db() -> Result<Arc<SqlitePool>> {
        let pool = Arc::new(
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect(":memory:")
                .await?,
        );
        run_migrations(pool.clone()).await?;
        Ok(pool)
    }

    #[tokio::test]
    async fn test_create_link_between_notes() -> Result<()> {
        // create sut
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        // exercise, verify
        let note_1 = note_service
            .create_note(NewNote {
                title: String::from("Title 1"),
                content: String::from("content for note 1"),
                metadata: json!({}),
            })
            .await?;
        let note_2 = note_service
            .create_note(NewNote {
                title: String::from("Title 2"),
                content: String::from("content for note 1"),
                metadata: json!({}),
            })
            .await?;
        let created_link = link_service
            .create_link(NewLink {
                from_note_id: note_1.id,
                to_note_id: note_2.id,
                link_type: LinkType::Reference,
            })
            .await?;
        assert_eq!(
            created_link.from_note_id, note_1.id,
            "Expected the same id between created link and note_1"
        );
        assert_eq!(
            created_link.to_note_id, note_2.id,
            "Expected the same id between created link and note_2"
        );
        assert_eq!(
            created_link.link_type,
            LinkType::Reference,
            "Expected the link type to be 'reference'"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_duplicate_link_creation_should_fail() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        let note_1 = note_service
            .create_note(NewNote {
                title: "dup1".into(),
                content: "c1".into(),
                metadata: json!({}),
            })
            .await?;
        let note_2 = note_service
            .create_note(NewNote {
                title: "dup2".into(),
                content: "c2".into(),
                metadata: json!({}),
            })
            .await?;
        let new_link = NewLink {
            from_note_id: note_1.id,
            to_note_id: note_2.id,
            link_type: LinkType::Reference,
        };
        let _ = link_service.create_link(new_link.clone()).await?;
        let result = link_service.create_link(new_link).await;
        assert!(
            matches!(result, Err(LinkServiceError::Database(_))),
            "Expected database error for duplicate link"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_update_link_type() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        let note_1 = note_service
            .create_note(NewNote {
                title: "A".into(),
                content: "A".into(),
                metadata: json!({}),
            })
            .await?;
        let note_2 = note_service
            .create_note(NewNote {
                title: "B".into(),
                content: "B".into(),
                metadata: json!({}),
            })
            .await?;
        let link = link_service
            .create_link(NewLink {
                from_note_id: note_1.id,
                to_note_id: note_2.id,
                link_type: LinkType::Reference,
            })
            .await?;
        let updated = link_service
            .update_link(Link {
                from_note_id: note_1.id,
                to_note_id: note_2.id,
                link_type: LinkType::Related,
                created_at: link.created_at,
            })
            .await?;
        assert_eq!(updated.link_type, LinkType::Related);
        Ok(())
    }

    #[tokio::test]
    async fn test_update_nonexistent_link_should_fail() -> Result<()> {
        let pool = setup_test_db().await?;
        let link_service = LinkService::create(pool);
        let fake_link = Link {
            from_note_id: 100,
            to_note_id: 200,
            link_type: LinkType::Parent,
            created_at: chrono::Utc::now().naive_utc(),
        };
        let result = link_service.update_link(fake_link).await;
        assert!(
            matches!(result, Err(LinkServiceError::NotFound)),
            "Expected NotFound error"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_delete_link() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        let note_1 = note_service
            .create_note(NewNote {
                title: "del1".into(),
                content: "c1".into(),
                metadata: json!({}),
            })
            .await?;
        let note_2 = note_service
            .create_note(NewNote {
                title: "del2".into(),
                content: "c2".into(),
                metadata: json!({}),
            })
            .await?;
        let link = link_service
            .create_link(NewLink {
                from_note_id: note_1.id,
                to_note_id: note_2.id,
                link_type: LinkType::Child,
            })
            .await?;
        let result = link_service.delete_link(link).await;
        assert!(result.is_ok(), "Expected successful deletion");
        Ok(())
    }

    #[tokio::test]
    async fn test_delete_nonexistent_link_should_fail() -> Result<()> {
        let pool = setup_test_db().await?;
        let link_service = LinkService::create(pool);
        let fake_link = Link {
            from_note_id: 999,
            to_note_id: 888,
            link_type: LinkType::Parent,
            created_at: chrono::Utc::now().naive_utc(),
        };
        let result = link_service.delete_link(fake_link).await;
        assert!(
            matches!(result, Err(LinkServiceError::NotFound)),
            "Expected NotFound error"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_linktype_enum_db_mapping() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        let note_1 = note_service
            .create_note(NewNote {
                title: "enum1".into(),
                content: "c1".into(),
                metadata: json!({}),
            })
            .await?;
        let note_2 = note_service
            .create_note(NewNote {
                title: "enum2".into(),
                content: "c2".into(),
                metadata: json!({}),
            })
            .await?;
        for variant in [
            LinkType::Reference,
            LinkType::Related,
            LinkType::Parent,
            LinkType::Child,
        ] {
            let link = link_service
                .create_link(NewLink {
                    from_note_id: note_1.id,
                    to_note_id: note_2.id,
                    link_type: variant.clone(),
                })
                .await?;
            assert_eq!(link.link_type, variant);
            // Clean up for next variant
            link_service.delete_link(link).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_foreign_key_constraint_should_fail() -> Result<()> {
        let pool = setup_test_db().await?;
        let link_service = LinkService::create(pool);
        let new_link = NewLink {
            from_note_id: 12345, // does not exist
            to_note_id: 67890,   // does not exist
            link_type: LinkType::Reference,
        };
        let result = link_service.create_link(new_link).await;
        assert!(
            matches!(result, Err(LinkServiceError::Database(_))),
            "Expected database error for foreign key violation"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_self_link_creation() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        let note = note_service
            .create_note(NewNote {
                title: "selflink".into(),
                content: "self".into(),
                metadata: json!({}),
            })
            .await?;
        let link = link_service
            .create_link(NewLink {
                from_note_id: note.id,
                to_note_id: note.id,
                link_type: LinkType::Parent,
            })
            .await?;
        assert_eq!(
            link.from_note_id, link.to_note_id,
            "Self-link should have same from and to id"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_transaction_commit_persists_changes() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        let mut tx = pool.begin().await?;

        // Create two notes and a link between them in a transaction
        let note_1 = note_service
            .create_note_through_executor(
                NewNote {
                    title: "tx_note1".into(),
                    content: "content1".into(),
                    metadata: json!({}),
                },
                &mut *tx,
            )
            .await?;
        let note_2 = note_service
            .create_note_through_executor(
                NewNote {
                    title: "tx_note2".into(),
                    content: "content2".into(),
                    metadata: json!({}),
                },
                &mut *tx,
            )
            .await?;
        let _link = link_service
            .create_link_through_executor(
                NewLink {
                    from_note_id: note_1.id,
                    to_note_id: note_2.id,
                    link_type: LinkType::Reference,
                },
                &mut *tx,
            )
            .await?;

        // Commit the transaction
        tx.commit().await?;

        // Verify notes and link are persisted
        let fetched_note_1 = note_service.get_note_by_id(note_1.id).await?;
        let fetched_note_2 = note_service.get_note_by_id(note_2.id).await?;
        assert_eq!(fetched_note_1, note_1);
        assert_eq!(fetched_note_2, note_2);
        let fetched_link = link_service.create_link_through_executor(
            NewLink {
                from_note_id: note_1.id,
                to_note_id: note_2.id,
                link_type: LinkType::Reference,
            },
            &*pool,
        );
        // The link already exists, so this should fail with a database error
        assert!(matches!(
            fetched_link.await,
            Err(LinkServiceError::Database(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn test_transaction_rollback_discards_changes() -> Result<()> {
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(pool.clone());
        let link_service = LinkService::create(pool.clone());
        let mut tx = pool.begin().await?;

        // Create two notes and a link between them in a transaction
        let note_1 = note_service
            .create_note_through_executor(
                NewNote {
                    title: "tx_rollback1".into(),
                    content: "content1".into(),
                    metadata: json!({}),
                },
                &mut *tx,
            )
            .await?;
        let note_2 = note_service
            .create_note_through_executor(
                NewNote {
                    title: "tx_rollback2".into(),
                    content: "content2".into(),
                    metadata: json!({}),
                },
                &mut *tx,
            )
            .await?;
        let _link = link_service
            .create_link_through_executor(
                NewLink {
                    from_note_id: note_1.id,
                    to_note_id: note_2.id,
                    link_type: LinkType::Reference,
                },
                &mut *tx,
            )
            .await?;

        // Rollback the transaction
        tx.rollback().await?;

        // Verify notes and link are NOT persisted
        let result_1 = note_service.get_note_by_id(note_1.id).await;
        let result_2 = note_service.get_note_by_id(note_2.id).await;
        assert!(matches!(
            result_1,
            Err(crate::db::notes::NoteServiceError::NotFound)
        ));
        assert!(matches!(
            result_2,
            Err(crate::db::notes::NoteServiceError::NotFound)
        ));
        // The link should not exist, so creating it should succeed
        let mut tx2 = pool.begin().await?;
        let link_create_result = link_service
            .create_link_through_executor(
                NewLink {
                    from_note_id: note_1.id,
                    to_note_id: note_2.id,
                    link_type: LinkType::Reference,
                },
                &mut *tx2,
            )
            .await;
        // Should fail with foreign key error because notes do not exist
        assert!(matches!(
            link_create_result,
            Err(LinkServiceError::Database(_))
        ));
        tx2.rollback().await?;
        Ok(())
    }
    #[tokio::test]
    async fn test_create_notes_with_relationship_multiple_success() -> Result<()> {
        let pool = setup_test_db().await?;
        let link_service = LinkService::create(pool.clone());
        let notes = vec![
            NotesWithLinkType::create(
                NewNote {
                    title: "A1".into(),
                    content: "C1".into(),
                    metadata: json!({}),
                },
                NewNote {
                    title: "A2".into(),
                    content: "C2".into(),
                    metadata: json!({}),
                },
                LinkType::Reference,
            ),
            NotesWithLinkType::create(
                NewNote {
                    title: "B1".into(),
                    content: "D1".into(),
                    metadata: json!({}),
                },
                NewNote {
                    title: "B2".into(),
                    content: "D2".into(),
                    metadata: json!({}),
                },
                LinkType::Related,
            ),
        ];
        let links = link_service.create_notes_with_relationship(notes).await?;
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].link_type, LinkType::Reference);
        assert_eq!(links[1].link_type, LinkType::Related);
        Ok(())
    }

    #[tokio::test]
    async fn test_create_notes_with_relationship_empty_vec() -> Result<()> {
        let pool = setup_test_db().await?;
        let link_service = LinkService::create(pool.clone());
        let notes: Vec<NotesWithLinkType> = vec![];
        let links = link_service.create_notes_with_relationship(notes).await?;
        assert!(links.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_create_notes_with_relationship_duplicate_notes() -> Result<()> {
        let pool = setup_test_db().await?;
        let link_service = LinkService::create(pool);
        let notes = vec![NotesWithLinkType::create(
            NewNote {
                title: String::from("Title A"),
                content: String::from("Content A"),
                metadata: json!({}),
            },
            NewNote {
                title: String::from("Title B"),
                content: String::from("Content B"),
                metadata: json!({}),
            },
            LinkType::Parent,
        )];
        let links = link_service.create_notes_with_relationship(notes).await?;
        assert_eq!(links.len(), 1);
        // try to create link for existing notes in opposite direction
        let created_link = link_service
            .create_link(NewLink {
                from_note_id: links[0].to_note_id,
                to_note_id: links[0].from_note_id,
                link_type: LinkType::Child,
            })
            .await?;
        assert_eq!(links[0].link_type, LinkType::Parent);
        assert_eq!(created_link.link_type, LinkType::Child);
        Ok(())
    }

    #[tokio::test]
    async fn test_create_notes_with_relationship_rollback_on_error() {
        let pool = setup_test_db().await.unwrap();
        let link_service = LinkService::create(pool.clone());
        // The second link will fail due to invalid link_type (simulate error by using invalid note id after first insert)
        let notes = vec![
            NotesWithLinkType::create(
                NewNote {
                    title: "ok1".into(),
                    content: "ok1".into(),
                    metadata: json!({}),
                },
                NewNote {
                    title: "ok2".into(),
                    content: "ok2".into(),
                    metadata: json!({}),
                },
                LinkType::Reference,
            ),
            // This will fail because SQLite will reject empty title (simulate error)
            NotesWithLinkType::create(
                NewNote {
                    title: "".into(), // Assuming NOT NULL constraint on title
                    content: "fail".into(),
                    metadata: json!({}),
                },
                NewNote {
                    title: "fail2".into(),
                    content: "fail2".into(),
                    metadata: json!({}),
                },
                LinkType::Related,
            ),
        ];
        let result = link_service.create_notes_with_relationship(notes).await;
        assert!(result.is_err());
        // Ensure nothing was committed
        let _note_service = NoteService::create(pool.clone());
        let all_notes_count: i64 = sqlx::query_scalar("SELECT COUNT(*) as count FROM notes")
            .fetch_one(&*pool)
            .await
            .unwrap();
        assert_eq!(all_notes_count, 0);
    }
}
