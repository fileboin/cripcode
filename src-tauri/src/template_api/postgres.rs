//! PostgreSQL metadata repository — production backend.
//!
//! Enabled by the `template-postgres` cargo feature so the desktop app build
//! never links sqlx. Records are loaded once at startup into memory (the same
//! in-memory view `FileTemplateRepository` provides), which keeps the HTTP
//! handlers synchronous and identical across backends. The write path
//! (`upsert_record`) exists for the migration tool.
//!
//! Tests require a live database via `CRIPCODE_TEMPLATE_TEST_DATABASE_URL`
//! (same opt-in pattern as the repo's `SHIPSTUDIO_LIVE_ANDROID` tests) and
//! skip otherwise — no Docker dependency in the normal test run.

use super::repository::TemplateRecord;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;

/// The schema, embedded at compile time. It is idempotent (`IF NOT EXISTS`
/// throughout) so every connect re-applies it safely — the same guarantee
/// sqlx's migrator gives us, without pulling in the `macros` feature.
const SCHEMA_SQL: &str = include_str!("../../migrations/postgres/0001_create_templates.sql");

/// How long to wait for the database on startup.
const CONNECT_TIMEOUT_SECONDS: u64 = 15;

#[derive(Debug, Clone)]
pub struct PostgresTemplateRepository {
    pool: PgPool,
    records: Vec<TemplateRecord>,
}

impl PostgresTemplateRepository {
    /// Connect, run pending migrations, and load all records.
    pub async fn connect(database_url: &str) -> Result<Self, String> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECONDS))
            .connect(database_url)
            .await
            .map_err(|e| format!("Failed to connect to the template database: {e}"))?;

        sqlx::raw_sql(SCHEMA_SQL)
            .execute(&pool)
            .await
            .map_err(|e| format!("Failed to apply the template schema: {e}"))?;

        let records = Self::load_records(&pool).await?;
        Ok(Self { pool, records })
    }

    pub fn records(&self) -> &[TemplateRecord] {
        &self.records
    }

    pub fn find(&self, id: &str) -> Option<&TemplateRecord> {
        self.records.iter().find(|record| record.id == id)
    }

    /// The write pool (used by the migration tool).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Insert or update one record. Idempotent: re-running with the same
    /// record leaves the table in the same state (no duplicates — `id` is the
    /// primary key and `object_key` is unique).
    pub async fn upsert_record(&self, record: &TemplateRecord) -> Result<(), String> {
        sqlx::query(
            r#"
            INSERT INTO templates (
                id, name, description, author, category, framework,
                thumbnail_key, version, object_key, object_size,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8, $9, $10,
                $11::timestamptz, $12::timestamptz
            )
            ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                author = EXCLUDED.author,
                category = EXCLUDED.category,
                framework = EXCLUDED.framework,
                thumbnail_key = EXCLUDED.thumbnail_key,
                version = EXCLUDED.version,
                object_key = EXCLUDED.object_key,
                object_size = EXCLUDED.object_size,
                updated_at = now()
            "#,
        )
        .bind(&record.id)
        .bind(&record.name)
        .bind(&record.description)
        .bind(&record.author)
        .bind(&record.category)
        .bind(&record.framework)
        .bind(&record.thumbnail_key)
        .bind(&record.version)
        .bind(&record.zip_key)
        .bind(record.object_size.map(|size| size as i64))
        .bind(&record.created_at)
        .bind(&record.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to upsert template '{}': {e}", record.id))?;
        Ok(())
    }

    async fn load_records(pool: &PgPool) -> Result<Vec<TemplateRecord>, String> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, description, author, category, framework,
                   thumbnail_key, version, object_key, object_size,
                   created_at, updated_at
            FROM templates
            ORDER BY created_at, id
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("Failed to load templates from the database: {e}"))?;

        rows.into_iter()
            .map(|row| {
                let created_at: chrono::DateTime<chrono::Utc> = row
                    .try_get("created_at")
                    .map_err(|e| format!("Invalid created_at: {e}"))?;
                let updated_at: chrono::DateTime<chrono::Utc> = row
                    .try_get("updated_at")
                    .map_err(|e| format!("Invalid updated_at: {e}"))?;
                Ok(TemplateRecord {
                    id: row.try_get("id").map_err(|e| format!("Invalid id: {e}"))?,
                    name: row
                        .try_get("name")
                        .map_err(|e| format!("Invalid name: {e}"))?,
                    description: row
                        .try_get("description")
                        .map_err(|e| format!("Invalid description: {e}"))?,
                    author: row
                        .try_get("author")
                        .map_err(|e| format!("Invalid author: {e}"))?,
                    category: row
                        .try_get("category")
                        .map_err(|e| format!("Invalid category: {e}"))?,
                    framework: row
                        .try_get("framework")
                        .map_err(|e| format!("Invalid framework: {e}"))?,
                    thumbnail_key: row
                        .try_get("thumbnail_key")
                        .map_err(|e| format!("Invalid thumbnail_key: {e}"))?,
                    version: row
                        .try_get("version")
                        .map_err(|e| format!("Invalid version: {e}"))?,
                    zip_key: row
                        .try_get("object_key")
                        .map_err(|e| format!("Invalid object_key: {e}"))?,
                    object_size: row
                        .try_get::<Option<i64>, _>("object_size")
                        .map_err(|e| format!("Invalid object_size: {e}"))?
                        .map(|size| size.max(0) as u64),
                    created_at: created_at.to_rfc3339(),
                    updated_at: updated_at.to_rfc3339(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database_url() -> Option<String> {
        std::env::var("CRIPCODE_TEMPLATE_TEST_DATABASE_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn sample_record(id: &str) -> TemplateRecord {
        TemplateRecord {
            id: id.into(),
            name: format!("Template {id}"),
            description: "A migrated template".into(),
            author: "CripCode".into(),
            category: "development".into(),
            framework: "HTML".into(),
            thumbnail_key: None,
            zip_key: format!("{id}.zip"),
            version: "1.0.0".into(),
            object_size: Some(183),
            created_at: "2026-08-29T00:00:00Z".into(),
            updated_at: "2026-08-29T00:00:00Z".into(),
        }
    }

    /// Live-database test — skips without `CRIPCODE_TEMPLATE_TEST_DATABASE_URL`.
    /// Verifies connect+migrate, upsert idempotency, and reload.
    #[tokio::test]
    async fn postgres_round_trip_is_idempotent() {
        let Some(url) = test_database_url() else {
            eprintln!("skipping: set CRIPCODE_TEMPLATE_TEST_DATABASE_URL to run");
            return;
        };
        let unique = format!("pg-test-{}", uuid::Uuid::new_v4().simple());
        let repository = PostgresTemplateRepository::connect(&url)
            .await
            .expect("connect + migrate");

        let record = sample_record(&unique);
        repository
            .upsert_record(&record)
            .await
            .expect("first upsert");
        // Same record again — must not duplicate or fail.
        repository
            .upsert_record(&record)
            .await
            .expect("idempotent upsert");

        let reloaded = PostgresTemplateRepository::connect(&url)
            .await
            .expect("reconnect");
        let found = reloaded.find(&unique).expect("record after reload");
        assert_eq!(found.name, record.name);
        assert_eq!(found.zip_key, record.zip_key);
        assert_eq!(found.object_size, Some(183));

        // Cleanup so repeated runs stay green.
        sqlx::query("DELETE FROM templates WHERE id = $1")
            .bind(&unique)
            .execute(reloaded.pool())
            .await
            .expect("cleanup");
    }
}
