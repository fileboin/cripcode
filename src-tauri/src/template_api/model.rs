use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateDownloadInfo {
    pub url: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub category: String,
    pub framework: String,
    pub thumbnail: Option<String>,
    pub version: String,
    pub download: TemplateDownloadInfo,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateListResponse {
    pub templates: Vec<TemplateMetadata>,
    pub total: u64,
}
