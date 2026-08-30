//! Environment-driven configuration for the CripCode Template API.
//!
//! One struct, built once at startup from `CRIPCODE_TEMPLATE_API_*` /
//! `CRIPCODE_TEMPLATES_STORAGE_*` variables (the standalone binary's existing
//! config convention). No secrets are ever read from anywhere but the
//! environment.

use std::path::PathBuf;

/// Which metadata repository backs the API.
#[derive(Debug, Clone)]
pub enum RepositoryConfig {
    /// File-based JSON registry (`templates.json`) — development and tests.
    File { data_dir: PathBuf },
    /// PostgreSQL — production. Requires the `template-postgres` build
    /// feature; the URL is only ever read from the environment.
    Postgres { database_url: String },
}

/// Which object storage backs template ZIPs and thumbnails.
#[derive(Debug, Clone)]
pub enum StorageConfig {
    /// Local directory with HMAC-signed URLs served by this process —
    /// development and tests. This is the default.
    Local {
        data_dir: PathBuf,
        signing_secret: String,
    },
    /// Any S3-compatible provider (AWS S3, Cloudflare R2, MinIO, Backblaze
    /// B2…). Downloads are presigned GETs pointing straight at the provider —
    /// unless a public base URL is configured (R2 public bucket / CDN), in
    /// which case object URLs are `{public_base_url}/{key}` instead.
    S3 {
        endpoint: String,
        region: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        /// Optional public base URL. When set, the operator has explicitly
        /// opted into public object serving, so URLs skip presigning.
        public_base_url: Option<String>,
    },
}

/// Full runtime configuration for the standalone Template API binary.
#[derive(Debug, Clone)]
pub struct TemplateApiConfig {
    pub bind: String,
    pub public_base_url: String,
    pub repository: RepositoryConfig,
    pub storage: StorageConfig,
}

/// Maximum accepted presigned-URL lifetime (S3 caps query auth at 7 days).
pub const MAX_PRESIGN_SECONDS: u64 = 604_800;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

impl TemplateApiConfig {
    /// Build the configuration from the environment. Validation errors name
    /// the missing variable so an operator can fix the deployment without
    /// reading source code.
    pub fn from_env() -> Result<Self, String> {
        let bind = env_or("CRIPCODE_TEMPLATE_API_BIND", "127.0.0.1:8787");
        let public_base_url = std::env::var("CRIPCODE_TEMPLATE_API_PUBLIC_BASE_URL")
            .unwrap_or_else(|_| format!("http://{bind}"));
        validate_base_url(&public_base_url, "CRIPCODE_TEMPLATE_API_PUBLIC_BASE_URL")?;

        let database_url = non_empty_env("CRIPCODE_TEMPLATE_API_DATABASE_URL");
        let storage_provider = std::env::var("CRIPCODE_TEMPLATES_STORAGE_PROVIDER")
            .unwrap_or_default()
            .trim()
            .to_lowercase();

        let repository = match database_url {
            Some(url) => RepositoryConfig::Postgres { database_url: url },
            None => RepositoryConfig::File {
                data_dir: required_dir("CRIPCODE_TEMPLATE_API_DATA_DIR")?,
            },
        };
        // PostgreSQL is the production marker: it must never inherit the
        // development default storage or signing secret by omission.
        let is_postgres = matches!(&repository, RepositoryConfig::Postgres { .. });
        if storage_provider.is_empty() && is_postgres {
            return Err(
                "CRIPCODE_TEMPLATES_STORAGE_PROVIDER must be explicitly set to 'local' or 's3' \
                 when CRIPCODE_TEMPLATE_API_DATABASE_URL is configured — production must not \
                 implicitly fall back to local file storage"
                    .into(),
            );
        }

        let storage = match storage_provider.as_str() {
            "" | "local" => {
                let data_dir = match &repository {
                    RepositoryConfig::File { data_dir } => data_dir.clone(),
                    RepositoryConfig::Postgres { .. } => {
                        required_dir("CRIPCODE_TEMPLATE_API_DATA_DIR")?
                    }
                };
                StorageConfig::Local {
                    data_dir,
                    signing_secret: local_signing_secret(is_postgres)?,
                }
            }
            "s3" => {
                let storage_public_base_url =
                    match std::env::var("CRIPCODE_TEMPLATES_STORAGE_PUBLIC_BASE_URL") {
                        Ok(value) if !value.trim().is_empty() => {
                            let value = value.trim().to_string();
                            validate_base_url(
                                &value,
                                "CRIPCODE_TEMPLATES_STORAGE_PUBLIC_BASE_URL",
                            )?;
                            Some(value)
                        }
                        _ => None,
                    };
                StorageConfig::S3 {
                    endpoint: std::env::var("CRIPCODE_TEMPLATES_STORAGE_ENDPOINT").unwrap_or_else(
                        |_| {
                            // Default to path-style AWS S3 for the region.
                            let region = env_or("CRIPCODE_TEMPLATES_STORAGE_REGION", "us-east-1");
                            format!("https://s3.{region}.amazonaws.com")
                        },
                    ),
                    region: env_or("CRIPCODE_TEMPLATES_STORAGE_REGION", "us-east-1"),
                    bucket: required_value("CRIPCODE_TEMPLATES_STORAGE_BUCKET")?,
                    access_key: required_value("CRIPCODE_TEMPLATES_STORAGE_ACCESS_KEY")?,
                    secret_key: required_value("CRIPCODE_TEMPLATES_STORAGE_SECRET_KEY")?,
                    public_base_url: storage_public_base_url,
                }
            }
            other => {
                return Err(format!(
                    "CRIPCODE_TEMPLATES_STORAGE_PROVIDER must be 'local' or 's3', got '{other}'"
                ));
            }
        };

        Ok(Self {
            bind,
            public_base_url,
            repository,
            storage,
        })
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| {
            let trimmed = value.trim().to_string();
            trimmed
        })
        .filter(|value| !value.is_empty())
}

fn required_value(name: &str) -> Result<String, String> {
    non_empty_env(name).ok_or_else(|| format!("{name} is required when S3 storage is selected"))
}

fn required_dir(name: &str) -> Result<PathBuf, String> {
    non_empty_env(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} is required for the local file-based adapter"))
}

/// The signing secret for local-storage download URLs. PostgreSQL-backed
/// deployments are production: the development default must never sign their
/// URLs. Whitespace-only values count as missing.
fn local_signing_secret(is_postgres: bool) -> Result<String, String> {
    match non_empty_env("CRIPCODE_TEMPLATE_API_SIGNING_SECRET") {
        Some(secret) => Ok(secret),
        None if is_postgres => Err("CRIPCODE_TEMPLATE_API_SIGNING_SECRET is required when \
             CRIPCODE_TEMPLATE_API_DATABASE_URL is configured — the development \
             default must not sign production URLs"
            .into()),
        None => Ok("cripcode-development-only-secret".to_string()),
    }
}

fn validate_base_url(value: &str, variable: &str) -> Result<(), String> {
    let url = url::Url::parse(value).map_err(|_| format!("Invalid {variable}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(format!(
            "{variable} must use http or https and include a host"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_base_urls_and_names_the_variable() {
        assert!(validate_base_url("https://api.cripcode.dev", "VAR").is_ok());
        assert!(validate_base_url("http://127.0.0.1:8787", "VAR").is_ok());
        assert!(validate_base_url("ftp://example.com", "VAR").is_err());
        assert!(validate_base_url("not-a-url", "VAR").is_err());
        let error = validate_base_url("not-a-url", "CRIPCODE_TEMPLATES_STORAGE_PUBLIC_BASE_URL")
            .unwrap_err();
        assert!(error.contains("CRIPCODE_TEMPLATES_STORAGE_PUBLIC_BASE_URL"));
    }

    // ── Production configuration guards ─────────────────────────────────
    //
    // `from_env` reads the process environment, so env-mutating tests share
    // a lock and restore every touched variable on drop (no new dependency).

    const DATABASE_URL: &str = "CRIPCODE_TEMPLATE_API_DATABASE_URL";
    const PROVIDER: &str = "CRIPCODE_TEMPLATES_STORAGE_PROVIDER";
    const DATA_DIR: &str = "CRIPCODE_TEMPLATE_API_DATA_DIR";
    const SIGNING_SECRET: &str = "CRIPCODE_TEMPLATE_API_SIGNING_SECRET";
    const BUCKET: &str = "CRIPCODE_TEMPLATES_STORAGE_BUCKET";
    const ACCESS_KEY: &str = "CRIPCODE_TEMPLATES_STORAGE_ACCESS_KEY";
    const SECRET_KEY: &str = "CRIPCODE_TEMPLATES_STORAGE_SECRET_KEY";
    const ALL_TEMPLATE_ENV: [&str; 7] = [
        DATABASE_URL,
        PROVIDER,
        DATA_DIR,
        SIGNING_SECRET,
        BUCKET,
        ACCESS_KEY,
        SECRET_KEY,
    ];

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn configure_env(vars: &[(&'static str, &str)]) {
        for name in ALL_TEMPLATE_ENV {
            std::env::remove_var(name);
        }
        for (name, value) in vars {
            std::env::set_var(name, value);
        }
    }

    /// 1. PostgreSQL configured + no explicit storage provider → FAIL.
    #[test]
    fn production_postgres_without_explicit_storage_provider_is_refused() {
        let _lock = env_lock();
        let _cleanup = AllVarsRemoved;
        configure_env(&[(DATABASE_URL, "postgres://db.example/internal")]);

        let error = TemplateApiConfig::from_env().unwrap_err();
        assert!(
            error.contains("CRIPCODE_TEMPLATES_STORAGE_PROVIDER"),
            "{error}"
        );

        // A whitespace-only provider counts as unset.
        std::env::set_var(PROVIDER, "   ");
        assert!(TemplateApiConfig::from_env().is_err());
    }

    /// 2. PostgreSQL configured + explicit S3 storage → validation PASS.
    #[test]
    fn production_postgres_with_explicit_s3_storage_passes_validation() {
        let _lock = env_lock();
        let _cleanup = AllVarsRemoved;
        configure_env(&[
            (DATABASE_URL, "postgres://db.example/internal"),
            (PROVIDER, "s3"),
            (BUCKET, "templates"),
            (ACCESS_KEY, "AKIDEXAMPLE"),
            (SECRET_KEY, "secret"),
        ]);

        let config = TemplateApiConfig::from_env().expect("explicit S3 must validate");
        assert!(matches!(config.storage, StorageConfig::S3 { .. }));
    }

    /// 3. PostgreSQL configured + local storage without a signing secret → FAIL.
    #[test]
    fn production_local_storage_requires_an_explicit_signing_secret() {
        let _lock = env_lock();
        let _cleanup = AllVarsRemoved;
        configure_env(&[
            (DATABASE_URL, "postgres://db.example/internal"),
            (PROVIDER, "local"),
            (DATA_DIR, "template-data"),
        ]);

        let error = TemplateApiConfig::from_env().unwrap_err();
        assert!(
            error.contains("CRIPCODE_TEMPLATE_API_SIGNING_SECRET"),
            "{error}"
        );

        // A whitespace-only secret counts as missing.
        std::env::set_var(SIGNING_SECRET, "   ");
        assert!(TemplateApiConfig::from_env().is_err());
    }

    /// 4. Development local mode needs only DATA_DIR and keeps the default.
    #[test]
    fn development_local_mode_keeps_the_default_signing_secret() {
        let _lock = env_lock();
        let _cleanup = AllVarsRemoved;
        configure_env(&[(DATA_DIR, "template-data")]);

        let config = TemplateApiConfig::from_env().expect("local dev mode needs only DATA_DIR");
        match config.storage {
            StorageConfig::Local { signing_secret, .. } => {
                assert_eq!(signing_secret, "cripcode-development-only-secret");
            }
            StorageConfig::S3 { .. } => panic!("expected the local dev storage default"),
        }
    }

    /// Explicit production local storage stays allowed with a real secret.
    #[test]
    fn production_local_storage_with_explicit_secret_is_allowed() {
        let _lock = env_lock();
        let _cleanup = AllVarsRemoved;
        configure_env(&[
            (DATABASE_URL, "postgres://db.example/internal"),
            (PROVIDER, "local"),
            (DATA_DIR, "template-data"),
            (SIGNING_SECRET, "operator-provided-secret"),
        ]);

        let config =
            TemplateApiConfig::from_env().expect("explicit local + secret must be allowed");
        match config.storage {
            StorageConfig::Local { signing_secret, .. } => {
                assert_eq!(signing_secret, "operator-provided-secret");
            }
            StorageConfig::S3 { .. } => panic!("expected the explicitly chosen local storage"),
        }
    }

    /// Removes every template env variable touched by the tests above, even
    /// when a test panics mid-run.
    struct AllVarsRemoved;
    impl Drop for AllVarsRemoved {
        fn drop(&mut self) {
            for name in ALL_TEMPLATE_ENV {
                std::env::remove_var(name);
            }
        }
    }
}
