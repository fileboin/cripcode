use super::s3::S3CompatibleStorage;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::path::{Path, PathBuf};
use url::form_urlencoded;

type HmacSha256 = Hmac<Sha256>;

/// How long a presigned S3 download link lives by default. Short-lived on
/// purpose: the API mints a fresh one on every list/details response.
pub const SIGNED_URL_TTL_SECONDS: u64 = 900;

/// The storage abstraction behind the API: local development files or any
/// S3-compatible provider. Enum dispatch (no trait objects, no new
/// dependencies) — exactly two backends exist and both are known at startup.
#[derive(Debug, Clone)]
pub enum TemplateStorage {
    Local(LocalTemplateStorage),
    S3(S3CompatibleStorage),
}

impl TemplateStorage {
    /// Build from validated configuration pieces.
    pub fn from_config(config: &super::config::StorageConfig) -> Result<Self, String> {
        match config {
            super::config::StorageConfig::Local {
                data_dir,
                signing_secret,
            } => Ok(Self::Local(LocalTemplateStorage::new(
                data_dir.join("objects"),
                signing_secret,
            ))),
            super::config::StorageConfig::S3 {
                endpoint,
                region,
                bucket,
                access_key,
                secret_key,
                public_base_url,
            } => Ok(Self::S3(
                S3CompatibleStorage::new(endpoint, region, bucket, access_key, secret_key)?
                    .with_public_base_url(public_base_url.as_deref()),
            )),
        }
    }

    /// Signed download URL for a template ZIP.
    /// - Local: HMAC-signed `/storage/` URL served by this process.
    /// - S3: provider presigned GET — the API never proxies object bytes —
    ///   or the plain public URL when a public base URL is configured.
    ///
    /// `expires_seconds` is the absolute unix timestamp after which the URL
    /// stops working (the Local backend embeds it verbatim; the S3 backend
    /// converts it to a relative presign TTL).
    pub fn signed_download_url(
        &self,
        public_base_url: &str,
        key: &str,
        expires_seconds: u64,
    ) -> Result<String, String> {
        match self {
            Self::Local(storage) => storage.signed_url(public_base_url, key, expires_seconds),
            Self::S3(storage) => match storage.public_base_url() {
                Some(_) => storage.public_object_url(key),
                None => {
                    let ttl = expires_seconds.saturating_sub(unix_seconds());
                    storage.presigned_get_url(key, ttl)
                }
            },
        }
    }

    /// Public URL for a thumbnail.
    /// - Local: `/thumbnails/` route on this process.
    /// - S3: presigned GET (thumbnails live in the same bucket), or the plain
    ///   public URL when a public base URL is configured.
    pub fn thumbnail_url(&self, public_base_url: &str, key: &str) -> Result<String, String> {
        match self {
            Self::Local(_) => Ok(format!(
                "{}/thumbnails/{key}",
                public_base_url.trim_end_matches('/')
            )),
            Self::S3(storage) => match storage.public_base_url() {
                Some(_) => storage.public_object_url(key),
                None => storage.presigned_get_url(key, SIGNED_URL_TTL_SECONDS),
            },
        }
    }

    /// Object size, when it can be determined synchronously (local FS only;
    /// S3 sizes are expected to come from the database record).
    pub fn size(&self, key: &str) -> Result<u64, String> {
        match self {
            Self::Local(storage) => storage.size(key),
            Self::S3(_) => Err("S3 object size must come from the metadata record".into()),
        }
    }

    /// Upload an object (used by the migration tool).
    pub async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<(), String> {
        match self {
            Self::Local(storage) => {
                let path = storage.object_path(key)?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create storage dir: {e}"))?;
                }
                std::fs::write(&path, bytes)
                    .map_err(|e| format!("Failed to write object '{key}': {e}"))
            }
            Self::S3(storage) => storage.put(key, bytes).await,
        }
    }

    /// Download an object's bytes (used by the migration tool's verification).
    pub async fn get(&self, key: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Local(storage) => storage.read(key),
            Self::S3(storage) => storage.get(key).await,
        }
    }

    /// Whether an object exists.
    pub async fn exists(&self, key: &str) -> Result<bool, String> {
        match self {
            Self::Local(storage) => Ok(storage.object_path(key)?.exists()),
            Self::S3(storage) => storage.exists(key).await,
        }
    }

    /// Delete an object (never called by the API; migration safety valve).
    pub async fn delete(&self, key: &str) -> Result<(), String> {
        match self {
            Self::Local(storage) => {
                let path = storage.object_path(key)?;
                if path.exists() {
                    std::fs::remove_file(&path)
                        .map_err(|e| format!("Failed to delete object '{key}': {e}"))?;
                }
                Ok(())
            }
            Self::S3(storage) => storage.delete(key).await,
        }
    }

    /// The local storage root, when this backend is local (development
    /// seeding needs it; S3 needs no seeding).
    pub fn local_root(&self) -> Option<&Path> {
        match self {
            Self::Local(storage) => Some(storage.root()),
            Self::S3(_) => None,
        }
    }
}

/// Seconds since the unix epoch. Shared by the server (expiry checks) and
/// the storage dispatch (absolute expiry → relative presign TTL).
pub(crate) fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone)]
pub struct LocalTemplateStorage {
    root: PathBuf,
    signing_secret: Vec<u8>,
}

impl LocalTemplateStorage {
    pub fn new(root: impl Into<PathBuf>, signing_secret: impl AsRef<[u8]>) -> Self {
        Self {
            root: root.into(),
            signing_secret: signing_secret.as_ref().to_vec(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn object_path(&self, key: &str) -> Result<PathBuf, String> {
        if key.is_empty()
            || key == "."
            || key == ".."
            || key.contains("..")
            || !key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
        {
            return Err("Invalid storage object key".into());
        }
        Ok(self.root.join(key))
    }

    pub fn size(&self, key: &str) -> Result<u64, String> {
        let path = self.object_path(key)?;
        std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .map_err(|e| format!("Template object is unavailable: {e}"))
    }

    pub fn read(&self, key: &str) -> Result<Vec<u8>, String> {
        let path = self.object_path(key)?;
        std::fs::read(path).map_err(|e| format!("Template object is unavailable: {e}"))
    }

    pub fn signed_url(&self, base_url: &str, key: &str, expires: u64) -> Result<String, String> {
        let base = url::Url::parse(base_url).map_err(|_| "Invalid storage base URL".to_string())?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            return Err("Storage base URL must use http or https and include a host".into());
        }
        self.object_path(key)?;
        let signature = self.signature(key, expires)?;
        Ok(format!(
            "{}/storage/{}?expires={expires}&signature={signature}",
            base_url.trim_end_matches('/'),
            key
        ))
    }

    pub fn verify_query(&self, key: &str, query: Option<&str>, now: u64) -> Result<(), String> {
        self.object_path(key)?;
        let values: std::collections::HashMap<_, _> =
            form_urlencoded::parse(query.unwrap_or_default().as_bytes())
                .into_owned()
                .collect();
        let expires = values
            .get("expires")
            .ok_or_else(|| "Signed URL is missing expires".to_string())?
            .parse::<u64>()
            .map_err(|_| "Signed URL has invalid expires".to_string())?;
        if expires < now {
            return Err("Signed URL has expired".into());
        }
        let signature = values
            .get("signature")
            .ok_or_else(|| "Signed URL is missing signature".to_string())?;
        if signature != &self.signature(key, expires)? {
            return Err("Signed URL signature is invalid".into());
        }
        Ok(())
    }

    fn signature(&self, key: &str, expires: u64) -> Result<String, String> {
        let mut mac = HmacSha256::new_from_slice(&self.signing_secret)
            .map_err(|_| "Invalid template signing secret".to_string())?;
        mac.update(format!("{key}:{expires}").as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies_a_storage_url() {
        let storage = LocalTemplateStorage::new("storage", "test-secret");
        let url = storage
            .signed_url("http://127.0.0.1:8787", "one.zip", 2_000)
            .unwrap();
        let query = url.split_once('?').unwrap().1;
        assert!(storage.verify_query("one.zip", Some(query), 1_000).is_ok());
        assert!(storage.verify_query("one.zip", Some(query), 2_001).is_err());
    }

    #[test]
    fn rejects_unsafe_object_keys() {
        let storage = LocalTemplateStorage::new("storage", "test-secret");
        assert!(storage.object_path("../secret.zip").is_err());
        assert!(storage.object_path("nested/secret.zip").is_err());
    }

    fn s3_config(public_base_url: Option<String>) -> super::super::config::StorageConfig {
        super::super::config::StorageConfig::S3 {
            endpoint: "http://127.0.0.1:9000".into(),
            region: "us-east-1".into(),
            bucket: "bucket".into(),
            access_key: "AKIDEXAMPLE".into(),
            secret_key: "secret".into(),
            public_base_url,
        }
    }

    /// The `X-Amz-Expires` value of a presigned URL (0 when absent).
    fn presigned_ttl(url: &str) -> u64 {
        url.split("X-Amz-Expires=")
            .nth(1)
            .unwrap_or_default()
            .split('&')
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    }

    #[test]
    fn s3_presigned_urls_remain_the_default() {
        let storage = TemplateStorage::from_config(&s3_config(None)).unwrap();
        // The contract: an absolute expiry timestamp, like the server passes.
        let expires = unix_seconds() + SIGNED_URL_TTL_SECONDS;
        let download = storage
            .signed_download_url("http://127.0.0.1:8787", "one.zip", expires)
            .unwrap();
        let thumbnail = storage
            .thumbnail_url("http://127.0.0.1:8787", "one.svg")
            .unwrap();
        for url in [download, thumbnail] {
            assert!(url.starts_with("http://127.0.0.1:9000/bucket/"), "{url}");
            let ttl = presigned_ttl(&url);
            // A second boundary may pass between the expiry being minted and
            // the URL being signed, so assert a range instead of equality.
            assert!(
                (SIGNED_URL_TTL_SECONDS - 10..=SIGNED_URL_TTL_SECONDS).contains(&ttl),
                "expected a TTL near {SIGNED_URL_TTL_SECONDS}, got {ttl} in {url}"
            );
            assert!(url.contains("X-Amz-Signature="), "{url}");
        }
    }

    /// Regression: `metadata()` passes an absolute unix expiry timestamp, but
    /// S3 presigning expects a relative TTL. Before the fix the S3 branch
    /// forwarded the timestamp verbatim, so every listing failed with
    /// "Presign expiry must be between 1 and 604800 seconds" and 500'd.
    #[test]
    fn s3_presign_converts_the_absolute_expiry_to_a_relative_ttl() {
        let storage = TemplateStorage::from_config(&s3_config(None)).unwrap();
        let expires = unix_seconds() + SIGNED_URL_TTL_SECONDS;
        let url = storage
            .signed_download_url("http://127.0.0.1:8787", "one.zip", expires)
            .expect("an absolute expiry must convert to a valid presign TTL");
        let ttl = presigned_ttl(&url);
        assert!(
            (SIGNED_URL_TTL_SECONDS - 10..=SIGNED_URL_TTL_SECONDS).contains(&ttl),
            "expected a TTL near {SIGNED_URL_TTL_SECONDS}, got {ttl} in {url}"
        );
    }

    #[test]
    fn s3_public_base_url_serves_public_object_urls_without_presigning() {
        let storage =
            TemplateStorage::from_config(&s3_config(Some("https://cdn.example.com/".into())))
                .unwrap();
        assert_eq!(
            storage
                .signed_download_url("http://127.0.0.1:8787", "one.zip", 900)
                .unwrap(),
            "https://cdn.example.com/one.zip"
        );
        assert_eq!(
            storage
                .thumbnail_url("http://127.0.0.1:8787", "one.svg")
                .unwrap(),
            "https://cdn.example.com/one.svg"
        );
    }

    #[test]
    fn s3_public_base_url_is_trimmed_of_empty_values() {
        let storage = TemplateStorage::from_config(&s3_config(Some("   ".into()))).unwrap();
        match storage {
            TemplateStorage::S3(inner) => assert_eq!(inner.public_base_url(), None),
            TemplateStorage::Local(_) => panic!("expected the S3 backend"),
        }
    }
}
