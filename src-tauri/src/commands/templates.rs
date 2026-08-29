//! # Template Gallery Commands
//!
//! Fetches community templates from the configured CripCode API and downloads
//! template zips.

use crate::errors::CommandError;
use crate::template_api::model::{TemplateListResponse, TemplateMetadata};

const TEMPLATES_API_BASE_ENV: &str = "VITE_CRIPCODE_TEMPLATES_API_BASE_URL";

fn configured_api_base(request_base: Option<&str>) -> Result<&str, CommandError> {
    request_base
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CommandError::expected(format!(
                "CripCode template API is not configured ({TEMPLATES_API_BASE_ENV})"
            ))
        })
}

fn template_api_url(
    id: Option<&str>,
    request_base: Option<&str>,
) -> Result<reqwest::Url, CommandError> {
    if let Some(id) = id {
        if id.trim().is_empty() || id.contains('/') || id.contains('\\') {
            return Err(CommandError::Validation {
                field: "id".into(),
                reason: "Template id must be a single non-empty path segment".into(),
            });
        }
    }

    let base = configured_api_base(request_base)?;
    let path = match id {
        Some(id) => format!("{}/api/v1/templates/{id}", base.trim_end_matches('/')),
        None => format!("{}/api/v1/templates", base.trim_end_matches('/')),
    };
    let url = reqwest::Url::parse(&path).map_err(|e| format!("Invalid template API URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CommandError::Validation {
            field: TEMPLATES_API_BASE_ENV.into(),
            reason: "Template API URL must use http or https and include a host".into(),
        });
    }
    Ok(url)
}

fn validate_non_empty(value: &str, field: &str) -> Result<(), CommandError> {
    if value.trim().is_empty() {
        return Err(CommandError::Validation {
            field: field.into(),
            reason: "Value must be a non-empty string".into(),
        });
    }
    Ok(())
}

fn validate_optional_url(value: Option<&str>, field: &str) -> Result<(), CommandError> {
    let Some(value) = value else { return Ok(()) };
    validate_non_empty(value, field)?;
    let url = reqwest::Url::parse(value).map_err(|_| CommandError::Validation {
        field: field.into(),
        reason: "Value must be a valid URL".into(),
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CommandError::Validation {
            field: field.into(),
            reason: "URL must use http or https and include a host".into(),
        });
    }
    Ok(())
}

fn validate_template(template: &TemplateMetadata) -> Result<(), CommandError> {
    for (value, field) in [
        (&template.id, "id"),
        (&template.name, "name"),
        (&template.description, "description"),
        (&template.author, "author"),
        (&template.category, "category"),
        (&template.framework, "framework"),
        (&template.version, "version"),
        (&template.created_at, "created_at"),
        (&template.updated_at, "updated_at"),
    ] {
        validate_non_empty(value, field)?;
    }
    validate_optional_url(template.thumbnail.as_deref(), "thumbnail")?;
    validate_optional_url(template.download.url.as_deref(), "download.url")?;
    Ok(())
}

fn serialize_template_list(body: &str) -> Result<String, CommandError> {
    let response: TemplateListResponse =
        serde_json::from_str(body).map_err(|e| CommandError::Validation {
            field: "template_response".into(),
            reason: format!("Invalid template list response: {e}"),
        })?;
    for template in &response.templates {
        validate_template(template)?;
    }
    serde_json::to_string(&response).map_err(|e| CommandError::Other {
        message: format!("Failed to serialize template list response: {e}"),
    })
}

fn serialize_template_details(body: &str) -> Result<String, CommandError> {
    let template: TemplateMetadata =
        serde_json::from_str(body).map_err(|e| CommandError::Validation {
            field: "template_response".into(),
            reason: format!("Invalid template details response: {e}"),
        })?;
    validate_template(&template)?;
    serde_json::to_string(&template).map_err(|e| CommandError::Other {
        message: format!("Failed to serialize template details response: {e}"),
    })
}

/// Render a reqwest error with its full source chain. reqwest's `Display` only
/// prints the top-level context ("error sending request for url (...)"), while
/// the actionable detail — DNS failure, connection refused, timed out, TLS —
/// lives in the `source()` chain (issue #255).
fn describe_reqwest_error(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        msg.push_str(&format!(": {s}"));
        source = s.source();
    }
    msg
}

/// Fetch community templates from the Cripcode API.
/// Accepts optional query parameters that map to the API spec.
/// Returns the raw JSON string so the frontend can parse it.
#[tauri::command]
#[tracing::instrument]
pub async fn fetch_community_templates(
    api_base_url: Option<String>,
    search: Option<String>,
    category: Option<String>,
    sort: Option<String>,
    framework: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<String, CommandError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut url = template_api_url(None, api_base_url.as_deref())?;

    {
        let mut params = url.query_pairs_mut();
        if let Some(s) = &search {
            if !s.is_empty() {
                params.append_pair("search", s);
            }
        }
        if let Some(c) = &category {
            params.append_pair("category", c);
        }
        if let Some(s) = &sort {
            params.append_pair("sort", s);
        }
        if let Some(f) = &framework {
            params.append_pair("framework", f);
        }
        if let Some(l) = limit {
            params.append_pair("limit", &l.to_string());
        }
        if let Some(o) = offset {
            params.append_pair("offset", &o.to_string());
        }
    }

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch templates: {}", describe_reqwest_error(&e)))?;

    if !response.status().is_success() {
        return Err((format!("API returned status {}", response.status())).into());
    }

    let body = response.text().await.map_err(|e| CommandError::Other {
        message: format!("Failed to read response: {e}"),
    })?;
    serialize_template_list(&body)
}

/// Fetch one template from the CripCode API.
#[tauri::command]
#[tracing::instrument]
pub async fn fetch_template_details(
    id: String,
    api_base_url: Option<String>,
) -> Result<String, CommandError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .get(template_api_url(Some(&id), api_base_url.as_deref())?)
        .send()
        .await
        .map_err(|e| {
            format!(
                "Failed to fetch template details: {}",
                describe_reqwest_error(&e)
            )
        })?;

    if !response.status().is_success() {
        return Err((format!("API returned status {}", response.status())).into());
    }

    let body = response.text().await.map_err(|e| CommandError::Other {
        message: format!("Failed to read response: {e}"),
    })?;
    serialize_template_details(&body)
}

/// Download a template zip from a signed URL to a temporary file.
/// Returns the path to the downloaded file.
#[tauri::command]
#[tracing::instrument]
pub async fn download_template_zip(url: String) -> Result<String, CommandError> {
    validate_optional_url(Some(&url), "url")?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client.get(&url).send().await.map_err(|e| {
        format!(
            "Failed to download template: {}",
            describe_reqwest_error(&e)
        )
    })?;

    if !response.status().is_success() {
        return Err((format!("Download failed with status {}", response.status())).into());
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {e}"))?;

    let tmp_dir = std::env::temp_dir().join("cripcode-templates");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("Failed to create temp dir: {e}"))?;

    let file_name = format!("{}.zip", uuid::Uuid::new_v4());
    let file_path = tmp_dir.join(&file_name);

    std::fs::write(&file_path, &bytes).map_err(|e| format!("Failed to write template zip: {e}"))?;

    file_path
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| CommandError::Other {
            message: "Invalid temp file path".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template_api::model::TemplateDownloadInfo;

    fn valid_template() -> TemplateMetadata {
        TemplateMetadata {
            id: "template-1".into(),
            name: "Starter".into(),
            description: "A starter project".into(),
            author: "CripCode".into(),
            category: "marketing".into(),
            framework: "Astro".into(),
            thumbnail: None,
            version: "1.0.0".into(),
            download: TemplateDownloadInfo {
                url: Some("https://storage.example/template.zip".into()),
                size_bytes: Some(1234),
            },
            created_at: "2026-08-29T00:00:00Z".into(),
            updated_at: "2026-08-29T00:00:00Z".into(),
        }
    }

    #[test]
    fn parses_template_list() {
        let response = TemplateListResponse {
            templates: vec![valid_template()],
            total: 1,
        };
        let raw = serde_json::to_string(&response).unwrap();
        assert!(serialize_template_list(&raw).is_ok());
    }

    #[test]
    fn parses_template_details() {
        let raw = serde_json::to_string(&valid_template()).unwrap();
        assert!(serialize_template_details(&raw).is_ok());
    }

    #[test]
    fn accepts_empty_template_list() {
        assert!(serialize_template_list(r#"{"templates":[],"total":0}"#).is_ok());
    }

    #[test]
    fn rejects_malformed_response() {
        assert!(serialize_template_list("not-json").is_err());
    }

    #[test]
    fn rejects_invalid_template_metadata() {
        let mut template = valid_template();
        template.author.clear();
        let raw = serde_json::to_string(&template).unwrap();
        assert!(serialize_template_details(&raw).is_err());
    }

    #[test]
    fn rejects_invalid_template_id() {
        assert!(template_api_url(Some("nested/id"), None).is_err());
    }

    #[test]
    fn refuses_to_fetch_without_the_frontend_api_base() {
        let error = template_api_url(None, None).unwrap_err();
        assert!(error
            .to_string()
            .contains("VITE_CRIPCODE_TEMPLATES_API_BASE_URL"));
    }

    #[test]
    fn builds_native_list_and_details_urls() {
        assert_eq!(
            template_api_url(None, Some("https://api.example"))
                .unwrap()
                .as_str(),
            "https://api.example/api/v1/templates"
        );
        assert_eq!(
            template_api_url(Some("template-1"), Some("https://api.example"))
                .unwrap()
                .as_str(),
            "https://api.example/api/v1/templates/template-1"
        );
    }
}
