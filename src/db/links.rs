use serde::{Deserialize, Serialize};
use sqlx::{
    Pool, Sqlite,
    prelude::{FromRow, Type},
    types::chrono,
};
use thiserror::Error;

use crate::db::{connection::PoolError, notes::NoteServiceError};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
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
    from_note_id: i64,
    to_note_id: i64,
    link_type: LinkType,
    created_at: chrono::NaiveDateTime,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct NewLink {
    from_note_id: i64,
    to_note_id: i64,
    link_type: LinkType,
}

pub struct LinkService<'a> {
    /// Reference to the SQLite connection pool.
    pool: &'a Pool<Sqlite>,
}

impl<'a> LinkService<'a> {
    /// Creates a new LinkService with a reference to the database pool.
    pub fn create(pool: &'a Pool<Sqlite>) -> Self {
        LinkService { pool: pool }
    }

    pub async fn create_link(&self, new_link: NewLink) -> Result<Link> {
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
        .fetch_one(self.pool)
        .await?;
        Ok(created_link)
    }

    pub async fn update_link(&self, updated_link: Link) -> Result<Link> {
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
        .fetch_optional(self.pool)
        .await?;
        match maybe_link {
            Some(link) => Ok(link),
            None => Err(LinkServiceError::NotFound),
        }
    }

    pub async fn delete_link(&self, link_to_delete: Link) -> Result<()> {
        let result = sqlx::query(
            r#"
            DELETE FROM links
            WHERE from_note_id = ?1 AND to_note_id = ?2
            "#,
        )
        .bind(link_to_delete.from_note_id)
        .bind(link_to_delete.to_note_id)
        .execute(self.pool)
        .await?;
        if result.rows_affected() == 0 {
            Err(LinkServiceError::NotFound)
        } else {
            Ok(())
        }
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
    use sqlx::SqlitePool;

    // helper function to setup in-memory database (SQLite) for testing purposes
    async fn setup_test_db() -> Result<SqlitePool> {
        let pool = SqlitePool::connect(":memory:").await?;
        run_migrations(&pool).await?;
        Ok(pool)
    }

    #[tokio::test]
    async fn test_create_link_between_notes() -> Result<()> {
        // create sut
        let pool = setup_test_db().await?;
        let note_service = NoteService::create(&pool);
        let link_service = LinkService::create(&pool);
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
        let note_service = NoteService::create(&pool);
        let link_service = LinkService::create(&pool);
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
        let note_service = NoteService::create(&pool);
        let link_service = LinkService::create(&pool);
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
        let link_service = LinkService::create(&pool);
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
        let note_service = NoteService::create(&pool);
        let link_service = LinkService::create(&pool);
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
        let link_service = LinkService::create(&pool);
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
        let note_service = NoteService::create(&pool);
        let link_service = LinkService::create(&pool);
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
        let link_service = LinkService::create(&pool);
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
        let note_service = NoteService::create(&pool);
        let link_service = LinkService::create(&pool);
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
}
