use ship_studio_lib::template_api::config::TemplateApiConfig;

#[tokio::main]
async fn main() {
    let command = std::env::args().nth(1).unwrap_or_else(|| "serve".into());
    let result = match command.as_str() {
        "serve" => ship_studio_lib::template_api::serve_from_env().await,
        "migrate" => run_migration().await,
        other => Err(format!(
            "Unknown command '{other}'. Usage: cripcode-template-api [serve|migrate]"
        )),
    };
    if let Err(error) = result {
        eprintln!("CripCode Template API failed: {error}");
        std::process::exit(1);
    }
}

/// Migrate local development data into the configured production target
/// (PostgreSQL metadata + S3-compatible object storage, per the environment).
/// Idempotent and non-destructive: the source `templates.json` and local
/// objects are only ever read.
async fn run_migration() -> Result<(), String> {
    let config = TemplateApiConfig::from_env()?;
    let source_dir = match &config.repository {
        ship_studio_lib::template_api::config::RepositoryConfig::File { data_dir } => {
            data_dir.clone()
        }
        ship_studio_lib::template_api::config::RepositoryConfig::Postgres { .. } => {
            // The migration source is always the local development dir.
            std::env::var("CRIPCODE_TEMPLATE_API_DATA_DIR")
                .map(std::path::PathBuf::from)
                .map_err(|_| {
                    "CRIPCODE_TEMPLATE_API_DATA_DIR must point at the local data dir to \
                     migrate from"
                        .to_string()
                })?
        }
    };

    let repository = match &config.repository {
        ship_studio_lib::template_api::config::RepositoryConfig::File { data_dir } => {
            ship_studio_lib::template_api::repository::TemplateRepository::File(
                ship_studio_lib::template_api::repository::FileTemplateRepository::load_or_seed(
                    data_dir.join("templates.json"),
                )?,
            )
        }
        #[allow(unreachable_patterns)]
        ship_studio_lib::template_api::config::RepositoryConfig::Postgres { database_url } => {
            #[cfg(feature = "template-postgres")]
            {
                ship_studio_lib::template_api::repository::TemplateRepository::Postgres(
                    ship_studio_lib::template_api::postgres::PostgresTemplateRepository::connect(
                        database_url,
                    )
                    .await?,
                )
            }
            #[cfg(not(feature = "template-postgres"))]
            {
                let _ = database_url;
                return Err(
                    "CRIPCODE_TEMPLATE_API_DATABASE_URL is set but this build lacks the \
                     template-postgres feature — rebuild with --features template-postgres"
                        .into(),
                );
            }
        }
    };

    let storage =
        ship_studio_lib::template_api::storage::TemplateStorage::from_config(&config.storage)?;
    if let Some(root) = storage.local_root() {
        std::fs::create_dir_all(root)
            .map_err(|e| format!("Failed to create local template storage: {e}"))?;
    }

    let report =
        ship_studio_lib::template_api::migrate::run(source_dir, &repository, &storage).await?;
    println!(
        "Migration complete: {} record(s) migrated, {} record(s) skipped, \
         {} object(s) uploaded, {} object(s) already present.",
        report.records_migrated,
        report.records_skipped,
        report.objects_migrated,
        report.objects_skipped
    );
    Ok(())
}
