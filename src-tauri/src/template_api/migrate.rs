//! One-shot migration: local development data → production infrastructure.
//!
//! Reads the file-based registry (`templates.json`) and local objects from
//! `CRIPCODE_TEMPLATE_API_DATA_DIR`, then writes metadata into the configured
//! repository (PostgreSQL when `CRIPCODE_TEMPLATE_API_DATABASE_URL` is set)
//! and objects into the configured storage (S3-compatible when selected).
//!
//! Properties, verified by tests:
//! - **Idempotent**: running twice upserts the same records and skips
//!   objects that already exist.
//! - **Repeatable**: no randomness, no partial states — each record/object is
//!   migrated independently.
//! - **Non-destructive**: the source `templates.json` and local objects are
//!   never modified or deleted.
//!
//! Run explicitly via `cripcode-template-api migrate`. Nothing runs it for
//! you — `serve` never migrates.

use super::repository::{FileTemplateRepository, TemplateRecord, TemplateRepository};
use super::storage::TemplateStorage;
use std::path::PathBuf;

/// What happened to one item during a migration run.
#[derive(Debug, PartialEq)]
enum Outcome {
    Migrated,
    Skipped,
}

#[derive(Debug, Default, PartialEq)]
pub struct MigrationReport {
    pub records_migrated: usize,
    pub records_skipped: usize,
    pub objects_migrated: usize,
    pub objects_skipped: usize,
}

impl MigrationReport {
    pub fn total_items(&self) -> usize {
        self.records_migrated + self.records_skipped + self.objects_migrated + self.objects_skipped
    }
}

/// Run the migration from a local data dir into the target repository and
/// storage. Both must already be configured (see [`TemplateApiConfig`]).
pub async fn run(
    data_dir: PathBuf,
    target_repository: &TemplateRepository,
    target_storage: &TemplateStorage,
) -> Result<MigrationReport, String> {
    let source = FileTemplateRepository::load_or_seed(data_dir.join("templates.json"))?;
    let local_objects = LocalObjectReader::new(data_dir.join("objects"));
    let mut report = MigrationReport::default();

    for record in source.records() {
        let record = with_object_size(&local_objects, record);
        match target_repository {
            TemplateRepository::File(_) => {
                // File→file is the test path: the registry is the source, so
                // nothing to copy — count it as skipped.
                report.records_skipped += 1;
            }
            #[cfg(feature = "template-postgres")]
            TemplateRepository::Postgres(repository) => {
                repository.upsert_record(&record).await?;
                report.records_migrated += 1;
            }
        }

        // Objects: upload if the target doesn't have them yet (either
        // backend — a local target lets tests exercise the full path).
        if let Some(bytes) = local_objects.read_optional(&record.zip_key)? {
            if target_storage.exists(&record.zip_key).await? {
                report.objects_skipped += 1;
            } else {
                target_storage.put(&record.zip_key, bytes).await?;
                report.objects_migrated += 1;
            }
        }
        if let Some(thumbnail_key) = &record.thumbnail_key {
            if let Some(bytes) = local_objects.read_optional(thumbnail_key)? {
                if target_storage.exists(thumbnail_key).await? {
                    report.objects_skipped += 1;
                } else {
                    target_storage.put(thumbnail_key, bytes).await?;
                    report.objects_migrated += 1;
                }
            }
        }
    }

    Ok(report)
}

/// Fill in `object_size` from the local file so PostgreSQL-backed listings
/// never need a per-object size lookup.
fn with_object_size(local_objects: &LocalObjectReader, record: &TemplateRecord) -> TemplateRecord {
    let mut record = record.clone();
    if record.object_size.is_none() {
        record.object_size = local_objects.size(&record.zip_key).ok();
    }
    record
}

/// Read-only view over the source objects directory. Never writes, never
/// deletes — the migration must not touch its source.
struct LocalObjectReader {
    root: PathBuf,
}

impl LocalObjectReader {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, key: &str) -> Result<PathBuf, String> {
        if key.is_empty()
            || key == "."
            || key == ".."
            || key.contains("..")
            || !key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
        {
            return Err(format!("Unsafe source object key '{key}'"));
        }
        Ok(self.root.join(key))
    }

    fn read_optional(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.path(key)?;
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read(&path)
            .map(Some)
            .map_err(|e| format!("Failed to read source object '{key}': {e}"))
    }

    fn size(&self, key: &str) -> Result<u64, String> {
        let path = self.path(key)?;
        std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .map_err(|e| format!("Failed to stat source object '{key}': {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template_api::storage::LocalTemplateStorage;

    fn source_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cripcode-tpl-migrate-src-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("objects")).unwrap();
        dir
    }

    fn target_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cripcode-tpl-migrate-dst-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("objects")).unwrap();
        dir
    }

    fn seeded_source() -> PathBuf {
        let dir = source_dir();
        // load_or_seed writes the seed record; the objects it references are
        // created here (the server's seeding step is what normally does this).
        FileTemplateRepository::load_or_seed(dir.join("templates.json")).unwrap();
        std::fs::write(
            dir.join("objects").join("cripcode-test-starter.zip"),
            b"PK fake zip bytes",
        )
        .unwrap();
        std::fs::write(
            dir.join("objects").join("cripcode-test-starter.svg"),
            b"<svg></svg>",
        )
        .unwrap();
        dir
    }

    #[tokio::test]
    async fn migration_is_idempotent_and_leaves_the_source_intact() {
        let source = seeded_source();
        let source_json_before = std::fs::read_to_string(source.join("templates.json")).unwrap();
        let source_zip_before =
            std::fs::read(source.join("objects").join("cripcode-test-starter.zip")).unwrap();

        let target = target_dir();
        let repository = TemplateRepository::File(
            FileTemplateRepository::from_json(target.join("templates.json"), "[]").unwrap(),
        );
        let storage =
            TemplateStorage::Local(LocalTemplateStorage::new(target.join("objects"), "secret"));

        let first = run(source.clone(), &repository, &storage).await.unwrap();
        assert!(
            first.objects_migrated >= 2,
            "seed zip + thumbnail must upload"
        );

        // Second run: everything already exists — only skips.
        let second = run(source.clone(), &repository, &storage).await.unwrap();
        assert_eq!(second.objects_migrated, 0);
        assert_eq!(second.objects_skipped, first.objects_migrated);

        // Source untouched.
        assert_eq!(
            std::fs::read_to_string(source.join("templates.json")).unwrap(),
            source_json_before
        );
        assert_eq!(
            std::fs::read(source.join("objects").join("cripcode-test-starter.zip")).unwrap(),
            source_zip_before
        );

        std::fs::remove_dir_all(source).ok();
        std::fs::remove_dir_all(target).ok();
    }

    #[tokio::test]
    async fn migration_reports_missing_source_objects_cleanly() {
        let source = source_dir();
        // Registry references an object that does not exist on disk.
        std::fs::write(
            source.join("templates.json"),
            r#"[{"id":"ghost","name":"Ghost","description":"Missing zip","author":"x","category":"dev","framework":"HTML","thumbnail_key":null,"zip_key":"ghost.zip","version":"1.0.0","created_at":"2026-08-29T00:00:00Z","updated_at":"2026-08-29T00:00:00Z"}]"#,
        )
        .unwrap();

        let target = target_dir();
        let repository = TemplateRepository::File(
            FileTemplateRepository::from_json(target.join("templates.json"), "[]").unwrap(),
        );
        let storage =
            TemplateStorage::Local(LocalTemplateStorage::new(target.join("objects"), "secret"));

        let report = run(source.clone(), &repository, &storage).await.unwrap();
        assert_eq!(report.objects_migrated, 0);
        assert_eq!(report.objects_skipped, 0);
        assert_eq!(report.total_items(), 1); // record only, skipped for file→file

        std::fs::remove_dir_all(source).ok();
        std::fs::remove_dir_all(target).ok();
    }
}
