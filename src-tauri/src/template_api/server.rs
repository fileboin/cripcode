use super::config::TemplateApiConfig;
use super::model::{TemplateDownloadInfo, TemplateListResponse, TemplateMetadata};
use super::repository::{FileTemplateRepository, TemplateRecord, TemplateRepository};
use super::storage::{unix_seconds, LocalTemplateStorage, TemplateStorage, SIGNED_URL_TTL_SECONDS};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::cmp::Ordering;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::net::TcpListener;
use url::form_urlencoded;

type ServerBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

#[derive(Clone)]
struct AppState {
    repository: Arc<TemplateRepository>,
    storage: Arc<TemplateStorage>,
    public_base_url: String,
}

pub async fn serve_from_env() -> Result<(), String> {
    let config = TemplateApiConfig::from_env()?;
    serve(config).await
}

pub async fn serve(config: TemplateApiConfig) -> Result<(), String> {
    let storage = TemplateStorage::from_config(&config.storage)?;
    if let Some(root) = storage.local_root() {
        std::fs::create_dir_all(root)
            .map_err(|e| format!("Failed to create local template storage: {e}"))?;
    }

    let repository = match &config.repository {
        super::config::RepositoryConfig::File { data_dir } => {
            let repository = FileTemplateRepository::load_or_seed(data_dir.join("templates.json"))?;
            seed_development_objects(&repository, &config.storage)?;
            TemplateRepository::File(repository)
        }
        super::config::RepositoryConfig::Postgres { database_url } => {
            #[cfg(feature = "template-postgres")]
            {
                TemplateRepository::Postgres(
                    super::postgres::PostgresTemplateRepository::connect(database_url).await?,
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

    guard_s3_storage_with_file_repository(&repository, &storage)?;
    let listener = TcpListener::bind(&config.bind)
        .await
        .map_err(|e| format!("Failed to bind template API at {}: {e}", config.bind))?;
    let state = AppState {
        repository: Arc::new(repository),
        storage: Arc::new(storage),
        public_base_url: config.public_base_url.clone(),
    };

    tracing::info!("CripCode template API listening on {}", config.bind);
    serve_listener(listener, state).await
}

async fn serve_listener(listener: TcpListener, state: AppState) -> Result<(), String> {
    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| format!("Template API accept failed: {e}"))?;
        let state = state.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |request| handle_request(request, state.clone()));
            if let Err(error) = http1::Builder::new().serve_connection(io, service).await {
                tracing::debug!("Template API connection failed: {error}");
            }
        });
    }
}

/// S3 object storage cannot be sized from the filesystem, so the file-based
/// registry (whose records may omit `object_size`) would 500 every listing.
/// Refuse that misconfiguration at startup, while still allowing it when the
/// operator's registry explicitly carries sizes for every record.
fn guard_s3_storage_with_file_repository(
    repository: &TemplateRepository,
    storage: &TemplateStorage,
) -> Result<(), String> {
    let (TemplateRepository::File(registry), TemplateStorage::S3(_)) = (repository, storage) else {
        return Ok(());
    };
    let missing_sizes: Vec<String> = registry
        .records()
        .iter()
        .filter(|record| record.object_size.is_none())
        .map(|record| record.id.clone())
        .collect();
    if missing_sizes.is_empty() {
        tracing::warn!(
            "File-based template registry with S3 storage: upload objects with \
             'cripcode-template-api migrate' before serving."
        );
        return Ok(());
    }
    Err(format!(
        "S3 object storage with the file-based registry requires every record to \
         carry object_size (remote objects cannot be sized). Records without \
         object_size: {missing_sizes:?}. Use PostgreSQL metadata (set \
         CRIPCODE_TEMPLATE_API_DATABASE_URL) or set \
         CRIPCODE_TEMPLATES_STORAGE_PROVIDER=local."
    ))
}

async fn handle_request(
    request: Request<Incoming>,
    state: AppState,
) -> Result<Response<ServerBody>, Infallible> {
    let path = request.uri().path();
    let response = if request.method() != hyper::Method::GET {
        response(
            StatusCode::METHOD_NOT_ALLOWED,
            "method not allowed",
            "text/plain",
        )
    } else if path == "/api/v1/templates" {
        list_templates(request.uri().query(), &state)
    } else if let Some(id) = path.strip_prefix("/api/v1/templates/") {
        template_details(id, &state)
    } else if let Some(key) = path.strip_prefix("/storage/") {
        storage_object(key, request.uri().query(), &state)
    } else if let Some(key) = path.strip_prefix("/thumbnails/") {
        thumbnail_object(key, &state)
    } else {
        response(StatusCode::NOT_FOUND, "not found", "text/plain")
    };
    Ok(response)
}

fn list_templates(query: Option<&str>, state: &AppState) -> Response<ServerBody> {
    let values: std::collections::HashMap<_, _> =
        form_urlencoded::parse(query.unwrap_or_default().as_bytes())
            .into_owned()
            .collect();
    let search = values.get("search").map(|value| value.to_lowercase());
    let category = values.get("category");
    let framework = values.get("framework");
    let sort = values.get("sort").map(|value| value.to_ascii_lowercase());
    let offset = parse_query_number(&values, "offset").unwrap_or(0);
    let limit = parse_query_number(&values, "limit").unwrap_or(20).min(100);

    let mut filtered: Vec<_> = state
        .repository
        .records()
        .iter()
        .filter(|record| {
            let text_matches = search.as_ref().is_none_or(|needle| {
                record.name.to_lowercase().contains(needle)
                    || record.description.to_lowercase().contains(needle)
            });
            let category_matches = category.is_none_or(|value| record.category == **value);
            let framework_matches = framework.is_none_or(|value| record.framework == **value);
            text_matches && category_matches && framework_matches
        })
        .collect();
    sort_records(&mut filtered, sort.as_deref());
    let total = filtered.len() as u64;
    let templates = match filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|record| metadata(record, state))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(templates) => templates,
        Err(error) => return response(StatusCode::INTERNAL_SERVER_ERROR, &error, "text/plain"),
    };

    json_response(&TemplateListResponse { templates, total })
}

fn template_details(id: &str, state: &AppState) -> Response<ServerBody> {
    match state.repository.find(id) {
        Some(record) => match metadata(record, state) {
            Ok(template) => json_response(&template),
            Err(error) => response(StatusCode::INTERNAL_SERVER_ERROR, &error, "text/plain"),
        },
        None => response(StatusCode::NOT_FOUND, "template not found", "text/plain"),
    }
}

fn metadata(record: &TemplateRecord, state: &AppState) -> Result<TemplateMetadata, String> {
    // Size comes from the record when the repository knows it (PostgreSQL),
    // otherwise from the local filesystem — S3-backed listings never make a
    // per-object network call.
    let size_bytes = match record.object_size {
        Some(size) => size,
        None => state.storage.size(&record.zip_key)?,
    };
    let expires = unix_seconds().saturating_add(SIGNED_URL_TTL_SECONDS);
    let url =
        state
            .storage
            .signed_download_url(&state.public_base_url, &record.zip_key, expires)?;
    let thumbnail = record
        .thumbnail_key
        .as_ref()
        .map(|key| state.storage.thumbnail_url(&state.public_base_url, key))
        .transpose()?
        .map(|url| url);
    Ok(TemplateMetadata {
        id: record.id.clone(),
        name: record.name.clone(),
        description: record.description.clone(),
        author: record.author.clone(),
        category: record.category.clone(),
        framework: record.framework.clone(),
        thumbnail,
        version: record.version.clone(),
        download: TemplateDownloadInfo {
            url: Some(url),
            size_bytes: Some(size_bytes),
        },
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    })
}

fn storage_object(key: &str, query: Option<&str>, state: &AppState) -> Response<ServerBody> {
    // S3-backed deployments never hit this route: their download URLs point
    // straight at the provider.
    let TemplateStorage::Local(storage) = state.storage.as_ref() else {
        return response(
            StatusCode::NOT_FOUND,
            "objects are served by the storage provider",
            "text/plain",
        );
    };
    if storage.verify_query(key, query, unix_seconds()).is_err() {
        return response(StatusCode::FORBIDDEN, "invalid signed URL", "text/plain");
    }
    match storage.read(key) {
        Ok(bytes) => bytes_response(StatusCode::OK, bytes, "application/zip"),
        Err(error) => response(StatusCode::NOT_FOUND, &error, "text/plain"),
    }
}

fn thumbnail_object(key: &str, state: &AppState) -> Response<ServerBody> {
    let TemplateStorage::Local(storage) = state.storage.as_ref() else {
        return response(
            StatusCode::NOT_FOUND,
            "thumbnails are served by the storage provider",
            "text/plain",
        );
    };
    match storage.read(key) {
        Ok(bytes) => bytes_response(StatusCode::OK, bytes, "image/svg+xml"),
        Err(error) => response(StatusCode::NOT_FOUND, &error, "text/plain"),
    }
}

fn seed_development_objects(
    repository: &FileTemplateRepository,
    storage: &super::config::StorageConfig,
) -> Result<(), String> {
    // Seeding is a local-storage development affordance only — production
    // (S3) objects arrive via the migration tool.
    let super::config::StorageConfig::Local { data_dir, .. } = storage else {
        return Ok(());
    };
    let storage = LocalTemplateStorage::new(data_dir.join("objects"), "");
    for record in repository.records() {
        let zip_path = storage.object_path(&record.zip_key)?;
        if !zip_path.exists() {
            let file = std::fs::File::create(&zip_path)
                .map_err(|e| format!("Failed to create development template ZIP: {e}"))?;
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("index.html", zip::write::SimpleFileOptions::default())
                .map_err(|e| format!("Failed to seed development template ZIP: {e}"))?;
            use std::io::Write;
            zip.write_all(
                b"<!doctype html><html><body><h1>CripCode test template</h1></body></html>",
            )
            .map_err(|e| format!("Failed to write development template ZIP: {e}"))?;
            zip.finish()
                .map_err(|e| format!("Failed to finalize development template ZIP: {e}"))?;
        }
        if let Some(key) = &record.thumbnail_key {
            let path = storage.object_path(key)?;
            if !path.exists() {
                std::fs::write(
                    path,
                    br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="20"><rect width="32" height="20" fill="#111827"/><text x="3" y="13" fill="#fff" font-size="6">CripCode</text></svg>"##,
                )
                .map_err(|e| format!("Failed to seed development thumbnail: {e}"))?;
            }
        }
    }
    Ok(())
}

fn parse_query_number(
    values: &std::collections::HashMap<String, String>,
    key: &str,
) -> Option<usize> {
    values.get(key).and_then(|value| value.parse().ok())
}

fn sort_records(records: &mut Vec<&TemplateRecord>, sort: Option<&str>) {
    let Some(sort) = sort else { return };
    let (sort, explicitly_descending) = sort
        .strip_prefix('-')
        .map_or((sort, false), |value| (value, true));
    let (field, default_descending) = match sort {
        "name" | "name_asc" => ("name", false),
        "name_desc" => ("name", true),
        "created_at" | "oldest" | "created_at_asc" => ("created_at", false),
        "newest" | "created_at_desc" => ("created_at", true),
        "updated_at" | "updated_at_asc" => ("updated_at", false),
        "recent" | "updated_at_desc" => ("updated_at", true),
        _ => return,
    };
    let descending = if explicitly_descending {
        !default_descending
    } else {
        default_descending
    };
    records.sort_by(|left, right| {
        let order = match field {
            "name" => left.name.cmp(&right.name),
            "created_at" => left.created_at.cmp(&right.created_at),
            "updated_at" => left.updated_at.cmp(&right.updated_at),
            _ => Ordering::Equal,
        };
        if descending {
            order.reverse()
        } else {
            order
        }
    });
}

fn json_response<T: serde::Serialize>(value: &T) -> Response<ServerBody> {
    match serde_json::to_vec(value) {
        Ok(body) => bytes_response(StatusCode::OK, body, "application/json"),
        Err(error) => response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to serialize response: {error}"),
            "text/plain",
        ),
    }
}

fn response(status: StatusCode, body: &str, content_type: &str) -> Response<ServerBody> {
    bytes_response(status, body.as_bytes().to_vec(), content_type)
}

fn bytes_response(status: StatusCode, body: Vec<u8>, content_type: &str) -> Response<ServerBody> {
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .body(
            Full::new(Bytes::from(body))
                .map_err(|never| match never {})
                .boxed(),
        )
        .unwrap_or_else(|_| {
            Response::new(
                Full::new(Bytes::new())
                    .map_err(|never| match never {})
                    .boxed(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template_api::s3::S3CompatibleStorage;

    fn state() -> AppState {
        let root = std::env::temp_dir().join(format!(
            "cripcode-template-api-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let registry = root.join("templates.json");
        let repository = FileTemplateRepository::load_or_seed(registry).unwrap();
        let local = LocalTemplateStorage::new(root.join("objects"), "test-secret");
        std::fs::create_dir_all(local.root()).unwrap();
        let storage_config = super::super::config::StorageConfig::Local {
            data_dir: root.clone(),
            signing_secret: "test-secret".into(),
        };
        seed_development_objects(&repository, &storage_config).unwrap();
        AppState {
            repository: Arc::new(TemplateRepository::File(repository)),
            storage: Arc::new(TemplateStorage::Local(local)),
            public_base_url: "http://127.0.0.1:8787".into(),
        }
    }

    /// An S3-backed state whose registry record carries `object_size`, so the
    /// metadata path never touches the filesystem or the network.
    fn s3_state(public_base_url: Option<&str>) -> AppState {
        let root = std::env::temp_dir().join(format!(
            "cripcode-template-api-s3-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let registry_json = r#"[{"id":"one","name":"One","description":"One","author":"CripCode","category":"development","framework":"HTML","thumbnail_key":"one.svg","zip_key":"one.zip","object_size":183,"version":"1.0.0","created_at":"2026-08-29T00:00:00Z","updated_at":"2026-08-29T00:00:00Z"}]"#;
        let repository =
            FileTemplateRepository::from_json(root.join("templates.json"), registry_json).unwrap();
        let storage = TemplateStorage::S3(
            S3CompatibleStorage::new(
                "http://127.0.0.1:9000",
                "us-east-1",
                "cripcode-templates",
                "AKIDEXAMPLE",
                "secret",
            )
            .unwrap()
            .with_public_base_url(public_base_url),
        );
        AppState {
            repository: Arc::new(TemplateRepository::File(repository)),
            storage: Arc::new(storage),
            public_base_url: "http://127.0.0.1:8787".into(),
        }
    }

    #[tokio::test]
    async fn s3_backed_details_serve_presigned_urls_with_record_sizes() {
        let state = s3_state(None);
        let response = template_details("one", &state);
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let template: TemplateMetadata = serde_json::from_slice(&body).unwrap();

        // Size comes straight from the record — no filesystem, no network.
        assert_eq!(template.download.size_bytes, Some(183));
        let download_url = template.download.url.unwrap();
        assert!(
            download_url.starts_with("http://127.0.0.1:9000/cripcode-templates/one.zip?"),
            "{download_url}"
        );
        assert!(download_url.contains("X-Amz-Signature="), "{download_url}");
        let thumbnail = template.thumbnail.unwrap();
        assert!(
            thumbnail.starts_with("http://127.0.0.1:9000/cripcode-templates/one.svg?"),
            "{thumbnail}"
        );
        assert!(thumbnail.contains("X-Amz-Signature="), "{thumbnail}");
    }

    #[tokio::test]
    async fn s3_public_base_url_replaces_presigned_urls() {
        let state = s3_state(Some("https://cdn.example.com/"));
        let response = template_details("one", &state);
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let template: TemplateMetadata = serde_json::from_slice(&body).unwrap();

        assert_eq!(
            template.download.url.unwrap(),
            "https://cdn.example.com/one.zip"
        );
        assert_eq!(
            template.thumbnail.unwrap(),
            "https://cdn.example.com/one.svg"
        );
    }

    #[test]
    fn s3_storage_with_file_registry_requires_record_sizes() {
        let sized = s3_state(None);
        assert!(
            guard_s3_storage_with_file_repository(&sized.repository, &sized.storage).is_ok(),
            "a registry whose records all carry object_size is allowed"
        );

        let root = std::env::temp_dir().join(format!(
            "cripcode-template-api-s3-guard-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let registry_json = r#"[{"id":"one","name":"One","description":"One","author":"CripCode","category":"development","framework":"HTML","thumbnail_key":null,"zip_key":"one.zip","version":"1.0.0","created_at":"2026-08-29T00:00:00Z","updated_at":"2026-08-29T00:00:00Z"}]"#;
        let repository = TemplateRepository::File(
            FileTemplateRepository::from_json(root.join("templates.json"), registry_json).unwrap(),
        );
        let storage = TemplateStorage::S3(
            S3CompatibleStorage::new(
                "http://127.0.0.1:9000",
                "us-east-1",
                "bucket",
                "AKIDEXAMPLE",
                "secret",
            )
            .unwrap(),
        );
        let error = guard_s3_storage_with_file_repository(&repository, &storage).unwrap_err();
        assert!(error.contains("one"), "{error}");
        assert!(error.contains("object_size"), "{error}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn local_storage_is_never_guarded() {
        let state = state();
        assert!(guard_s3_storage_with_file_repository(&state.repository, &state.storage).is_ok());
    }

    #[tokio::test]
    async fn local_http_round_trip_lists_details_and_serves_signed_zip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut state = state();
        state.public_base_url = format!("http://{address}");
        let server = tokio::spawn(serve_listener(listener, state));
        let client = reqwest::Client::new();
        let base_url = format!("http://{address}");

        let list_response = client
            .get(format!("{base_url}/api/v1/templates?search=cripcode"))
            .send()
            .await
            .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);
        let list: TemplateListResponse = list_response.json().await.unwrap();
        assert_eq!(list.total, 1);
        let template = list.templates.first().unwrap();

        let details_response = client
            .get(format!("{base_url}/api/v1/templates/{}", template.id))
            .send()
            .await
            .unwrap();
        assert_eq!(details_response.status(), StatusCode::OK);
        let details: TemplateMetadata = details_response.json().await.unwrap();
        assert_eq!(details, *template);

        let download_url = details.download.url.unwrap();
        let zip_response = client.get(&download_url).send().await.unwrap();
        assert_eq!(zip_response.status(), StatusCode::OK);
        assert_eq!(zip_response.headers()["content-type"], "application/zip");
        assert_eq!(&zip_response.bytes().await.unwrap()[..2], b"PK");

        let downloaded_path =
            crate::commands::templates::download_template_zip(download_url.clone())
                .await
                .unwrap();
        let downloaded_bytes = std::fs::read(&downloaded_path).unwrap();
        assert_eq!(&downloaded_bytes[..2], b"PK");
        std::fs::remove_file(downloaded_path).unwrap();

        let tampered_url = download_url.replace("signature=", "signature=invalid-");
        let tampered_response = client.get(tampered_url).send().await.unwrap();
        assert_eq!(tampered_response.status(), StatusCode::FORBIDDEN);

        server.abort();
    }

    #[test]
    fn list_supports_search_category_framework_and_pagination() {
        let state = state();
        let response = list_templates(
            Some("search=CripCode&category=development&framework=HTML&limit=1&offset=0"),
            &state,
        );
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn details_returns_not_found_for_missing_template() {
        let response = template_details("missing", &state());
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
