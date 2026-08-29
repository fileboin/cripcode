use super::model::{TemplateDownloadInfo, TemplateListResponse, TemplateMetadata};
use super::repository::{FileTemplateRepository, TemplateRecord};
use super::storage::LocalTemplateStorage;
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
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use url::form_urlencoded;

type ServerBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

#[derive(Clone)]
struct AppState {
    repository: Arc<FileTemplateRepository>,
    storage: Arc<LocalTemplateStorage>,
    public_base_url: String,
}

pub async fn serve_from_env() -> Result<(), String> {
    let bind =
        std::env::var("CRIPCODE_TEMPLATE_API_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let public_base_url = std::env::var("CRIPCODE_TEMPLATE_API_PUBLIC_BASE_URL")
        .unwrap_or_else(|_| format!("http://{bind}"));
    validate_public_base_url(&public_base_url)?;
    let data_dir = std::env::var("CRIPCODE_TEMPLATE_API_DATA_DIR")
        .map(std::path::PathBuf::from)
        .map_err(|_| {
            "CRIPCODE_TEMPLATE_API_DATA_DIR is required for the local adapter".to_string()
        })?;
    let secret = std::env::var("CRIPCODE_TEMPLATE_API_SIGNING_SECRET")
        .unwrap_or_else(|_| "cripcode-development-only-secret".into());

    let repository = FileTemplateRepository::load_or_seed(data_dir.join("templates.json"))?;
    let storage = LocalTemplateStorage::new(data_dir.join("objects"), secret);
    std::fs::create_dir_all(storage.root())
        .map_err(|e| format!("Failed to create local template storage: {e}"))?;
    seed_development_objects(&repository, &storage)?;

    let listener = TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("Failed to bind template API at {bind}: {e}"))?;
    let state = AppState {
        repository: Arc::new(repository),
        storage: Arc::new(storage),
        public_base_url,
    };

    tracing::info!("CripCode template API listening on {bind}");
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
    let size_bytes = state.storage.size(&record.zip_key)?;
    let expires = unix_seconds().saturating_add(900);
    let url = state
        .storage
        .signed_url(&state.public_base_url, &record.zip_key, expires)?;
    let thumbnail = record.thumbnail_key.as_ref().map(|key| {
        format!(
            "{}/thumbnails/{}",
            state.public_base_url.trim_end_matches('/'),
            key
        )
    });
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
    if state
        .storage
        .verify_query(key, query, unix_seconds())
        .is_err()
    {
        return response(StatusCode::FORBIDDEN, "invalid signed URL", "text/plain");
    }
    match state.storage.read(key) {
        Ok(bytes) => bytes_response(StatusCode::OK, bytes, "application/zip"),
        Err(error) => response(StatusCode::NOT_FOUND, &error, "text/plain"),
    }
}

fn thumbnail_object(key: &str, state: &AppState) -> Response<ServerBody> {
    match state.storage.read(key) {
        Ok(bytes) => bytes_response(StatusCode::OK, bytes, "image/svg+xml"),
        Err(error) => response(StatusCode::NOT_FOUND, &error, "text/plain"),
    }
}

fn seed_development_objects(
    repository: &FileTemplateRepository,
    storage: &LocalTemplateStorage,
) -> Result<(), String> {
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

fn validate_public_base_url(value: &str) -> Result<(), String> {
    let url =
        url::Url::parse(value).map_err(|_| "Invalid template API public base URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(
            "Template API public base URL must use http or https and include a host".into(),
        );
    }
    Ok(())
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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

    fn state() -> AppState {
        let root = std::env::temp_dir().join(format!(
            "cripcode-template-api-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let registry = root.join("templates.json");
        let repository = FileTemplateRepository::load_or_seed(registry).unwrap();
        let storage = LocalTemplateStorage::new(root.join("objects"), "test-secret");
        std::fs::create_dir_all(storage.root()).unwrap();
        seed_development_objects(&repository, &storage).unwrap();
        AppState {
            repository: Arc::new(repository),
            storage: Arc::new(storage),
            public_base_url: "http://127.0.0.1:8787".into(),
        }
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
