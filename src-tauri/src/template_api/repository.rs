use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One template's metadata as stored by a repository.
///
/// `object_size` is optional so the file-based registry (which sizes objects
/// from the filesystem) stays byte-compatible with existing `templates.json`
/// files; the PostgreSQL repository always populates it so listing never
/// needs a network round-trip per object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub category: String,
    pub framework: String,
    pub thumbnail_key: Option<String>,
    pub zip_key: String,
    pub version: String,
    #[serde(default)]
    pub object_size: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

/// The metadata abstraction behind the API: a JSON file for development and
/// tests, PostgreSQL for production. Enum dispatch — both variants are known
/// at startup and expose the same in-memory record view.
#[derive(Debug, Clone)]
pub enum TemplateRepository {
    File(FileTemplateRepository),
    #[cfg(feature = "template-postgres")]
    Postgres(super::postgres::PostgresTemplateRepository),
}

impl TemplateRepository {
    pub fn records(&self) -> &[TemplateRecord] {
        match self {
            Self::File(repository) => repository.records(),
            #[cfg(feature = "template-postgres")]
            Self::Postgres(repository) => repository.records(),
        }
    }

    pub fn find(&self, id: &str) -> Option<&TemplateRecord> {
        match self {
            Self::File(repository) => repository.find(id),
            #[cfg(feature = "template-postgres")]
            Self::Postgres(repository) => repository.find(id),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileTemplateRepository {
    path: PathBuf,
    records: Vec<TemplateRecord>,
}

impl FileTemplateRepository {
    pub fn from_json(path: impl Into<PathBuf>, contents: &str) -> Result<Self, String> {
        let path = path.into();
        let records: Vec<TemplateRecord> = serde_json::from_str(contents)
            .map_err(|e| format!("Invalid template metadata registry: {e}"))?;
        validate_records(&records)?;
        Ok(Self { path, records })
    }

    pub fn load(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read template metadata registry: {e}"))?;
        Self::from_json(path, &contents)
    }

    pub fn load_or_seed(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if path.is_file() {
            return Self::load(path);
        }

        let records = vec![TemplateRecord {
            id: "cripcode-test-starter".into(),
            name: "CripCode Test Starter".into(),
            description: "A development-only template for local API verification.".into(),
            author: "CripCode Development".into(),
            category: "development".into(),
            framework: "HTML".into(),
            thumbnail_key: Some("cripcode-test-starter.svg".into()),
            zip_key: "cripcode-test-starter.zip".into(),
            object_size: None,
            version: "0.0.1-dev".into(),
            created_at: "2026-08-29T00:00:00Z".into(),
            updated_at: "2026-08-29T00:00:00Z".into(),
        }];

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create template metadata directory: {e}"))?;
        }
        let contents = serde_json::to_string_pretty(&records)
            .map_err(|e| format!("Failed to serialize template metadata registry: {e}"))?;
        std::fs::write(&path, contents)
            .map_err(|e| format!("Failed to seed template metadata registry: {e}"))?;
        Ok(Self { path, records })
    }

    pub fn records(&self) -> &[TemplateRecord] {
        &self.records
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn find(&self, id: &str) -> Option<&TemplateRecord> {
        self.records.iter().find(|record| record.id == id)
    }
}

fn validate_records(records: &[TemplateRecord]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for record in records {
        for (value, field) in [
            (&record.id, "id"),
            (&record.name, "name"),
            (&record.description, "description"),
            (&record.author, "author"),
            (&record.category, "category"),
            (&record.framework, "framework"),
            (&record.version, "version"),
            (&record.created_at, "created_at"),
            (&record.updated_at, "updated_at"),
        ] {
            if value.trim().is_empty() {
                return Err(format!(
                    "Template metadata field '{field}' must not be empty"
                ));
            }
        }
        if !record.id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        }) {
            return Err(format!(
                "Template id '{}' is not a safe path segment",
                record.id
            ));
        }
        if !ids.insert(&record.id) {
            return Err(format!("Duplicate template id '{}'", record.id));
        }
        validate_object_key(&record.zip_key, "zip_key")?;
        if let Some(key) = &record.thumbnail_key {
            validate_object_key(key, "thumbnail_key")?;
        }
    }
    Ok(())
}

fn validate_object_key(key: &str, field: &str) -> Result<(), String> {
    if key.is_empty()
        || key == "."
        || key == ".."
        || key.contains("..")
        || !key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
    {
        return Err(format!(
            "Template metadata field '{field}' has an unsafe object key"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_registry() {
        let repo = FileTemplateRepository::from_json("templates.json", "[]").unwrap();
        assert!(repo.records().is_empty());
    }

    #[test]
    fn rejects_malformed_metadata() {
        assert!(FileTemplateRepository::from_json("templates.json", "not-json").is_err());
    }

    #[test]
    fn rejects_duplicate_ids_and_unsafe_object_keys() {
        let record = TemplateRecord {
            id: "one".into(),
            name: "One".into(),
            description: "One".into(),
            author: "Test".into(),
            category: "test".into(),
            framework: "HTML".into(),
            thumbnail_key: None,
            zip_key: "one.zip".into(),
            object_size: None,
            version: "1.0.0".into(),
            created_at: "2026-08-29".into(),
            updated_at: "2026-08-29".into(),
        };
        let duplicate = record.clone();
        assert!(FileTemplateRepository::from_json(
            "templates.json",
            &serde_json::to_string(&[record, duplicate]).unwrap()
        )
        .is_err());

        let mut unsafe_key = TemplateRecord {
            id: "two".into(),
            name: "Two".into(),
            description: "Two".into(),
            author: "Test".into(),
            category: "test".into(),
            framework: "HTML".into(),
            thumbnail_key: None,
            zip_key: "two.zip".into(),
            object_size: None,
            version: "1.0.0".into(),
            created_at: "2026-08-29".into(),
            updated_at: "2026-08-29".into(),
        };
        unsafe_key.zip_key = "../two.zip".into();
        assert!(FileTemplateRepository::from_json(
            "templates.json",
            &serde_json::to_string(&[unsafe_key]).unwrap()
        )
        .is_err());
    }

    #[test]
    fn finds_seed_record_by_id() {
        let contents = serde_json::to_string(&[TemplateRecord {
            id: "one".into(),
            name: "One".into(),
            description: "One".into(),
            author: "Test".into(),
            category: "test".into(),
            framework: "HTML".into(),
            thumbnail_key: None,
            zip_key: "one.zip".into(),
            object_size: None,
            version: "1.0.0".into(),
            created_at: "2026-08-29".into(),
            updated_at: "2026-08-29".into(),
        }])
        .unwrap();
        let repo = FileTemplateRepository::from_json("templates.json", &contents).unwrap();
        assert_eq!(repo.find("one").unwrap().name, "One");
        assert!(repo.find("missing").is_none());
    }
}
