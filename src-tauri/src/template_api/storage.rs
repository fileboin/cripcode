use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::path::{Path, PathBuf};
use url::form_urlencoded;

type HmacSha256 = Hmac<Sha256>;

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
}
