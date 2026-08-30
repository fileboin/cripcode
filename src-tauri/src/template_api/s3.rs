//! S3-compatible object storage adapter with hand-rolled AWS Signature V4.
//!
//! Zero new dependencies: signing uses the crate's existing `hmac`, `sha2`,
//! `hex` and `reqwest` crates. Path-style addressing
//! (`{endpoint}/{bucket}/{key}`) keeps one code path compatible with AWS S3,
//! Cloudflare R2, MinIO, and Backblaze B2. All request payloads use
//! `UNSIGNED-PAYLOAD` (supported by every S3-compatible provider), and
//! downloads use presigned GET URLs so the API never proxies object bytes in
//! production.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

/// An S3-compatible object store configuration. Cheap to clone per request.
#[derive(Debug, Clone)]
pub struct S3CompatibleStorage {
    endpoint: url::Url,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    client: reqwest::Client,
    /// Optional public base URL (R2 public bucket / CDN). When set, object
    /// URLs are `{base}/{key}` — the operator explicitly opted into public
    /// serving, so downloads and thumbnails skip presigning.
    public_base_url: Option<String>,
}

/// Errors surface as plain strings: the standalone binary prints them and
/// exits non-zero, and no caller needs structured types.
type StorageResult<T> = Result<T, String>;

impl S3CompatibleStorage {
    pub fn new(
        endpoint: &str,
        region: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
    ) -> StorageResult<Self> {
        let endpoint = url::Url::parse(endpoint)
            .map_err(|e| format!("Invalid S3 endpoint '{endpoint}': {e}"))?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
            return Err("S3 endpoint must use http or https and include a host".into());
        }
        if bucket.is_empty() || bucket.contains('/') {
            return Err("S3 bucket must be a single non-empty path segment".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Failed to create S3 HTTP client: {e}"))?;
        Ok(Self {
            endpoint,
            region: region.to_string(),
            bucket: bucket.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            client,
            public_base_url: None,
        })
    }

    /// Attach an optional public base URL (validated in
    /// [`super::config`], which also documents the environment variable).
    pub fn with_public_base_url(mut self, public_base_url: Option<&str>) -> Self {
        self.public_base_url = public_base_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        self
    }

    /// The configured public base URL, when objects are served publicly.
    pub fn public_base_url(&self) -> Option<&str> {
        self.public_base_url.as_deref()
    }

    /// `{public_base_url}/{key}` — only meaningful when a public base URL is
    /// configured.
    pub fn public_object_url(&self, key: &str) -> StorageResult<String> {
        self.validate_key(key)?;
        let base = self.public_base_url.as_deref().unwrap_or_default();
        Ok(format!("{}/{}", base.trim_end_matches('/'), key))
    }

    /// Presigned GET URL — object key bound, expiring after `expires_seconds`.
    /// No credentials appear in the URL beyond the public access key id,
    /// which is by design in AWS SigV4 query authentication.
    pub fn presigned_get_url(&self, key: &str, expires_seconds: u64) -> StorageResult<String> {
        self.validate_key(key)?;
        if expires_seconds == 0 || expires_seconds > super::config::MAX_PRESIGN_SECONDS {
            return Err(format!(
                "Presign expiry must be between 1 and {} seconds",
                super::config::MAX_PRESIGN_SECONDS
            ));
        }
        let amz_date = amz_timestamp();
        let scope = format!("{}/{}/s3/aws4_request", amz_date.date, self.region);
        let credential = format!("{}/{}", self.access_key, scope);

        // Canonical query: every value URI-encoded, params sorted by name.
        // X-Amz-Signature is appended after signing and is not part of the
        // canonical query.
        let canonical_query = canonical_query_string(&[
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            ("X-Amz-Credential", credential),
            ("X-Amz-Date", amz_date.value.clone()),
            ("X-Amz-Expires", expires_seconds.to_string()),
            ("X-Amz-SignedHeaders", "host".to_string()),
        ]);

        let canonical_request = format!(
            "GET\n{path}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD",
            path = self.canonical_path(key),
            query = canonical_query,
            host = self.host_header(),
        );
        let signature = self.sign_request(&amz_date, &canonical_request);

        Ok(format!(
            "{origin}{path}?{query}&X-Amz-Signature={signature}",
            origin = self.origin(),
            path = self.canonical_path(key),
            query = canonical_query,
        ))
    }

    /// PUT an object. Uses `UNSIGNED-PAYLOAD`, valid on every S3-compatible
    /// provider, so no body hashing is needed.
    pub async fn put(&self, key: &str, bytes: Vec<u8>) -> StorageResult<()> {
        self.validate_key(key)?;
        let response = self
            .signed_request(reqwest::Method::PUT, key, Some(bytes))
            .await?;
        Self::check_status(response, "upload").await
    }

    /// GET an object's bytes.
    pub async fn get(&self, key: &str) -> StorageResult<Vec<u8>> {
        self.validate_key(key)?;
        let response = self.signed_request(reqwest::Method::GET, key, None).await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "S3 download '{key}' failed with status {status}: {body}"
            ));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|e| format!("Failed to read S3 object '{key}': {e}"))
    }

    /// Whether an object exists (HEAD).
    pub async fn exists(&self, key: &str) -> StorageResult<bool> {
        self.validate_key(key)?;
        let response = self
            .signed_request(reqwest::Method::HEAD, key, None)
            .await?;
        match response.status() {
            reqwest::StatusCode::OK => Ok(true),
            reqwest::StatusCode::NOT_FOUND => Ok(false),
            status => Err(format!("S3 HEAD '{key}' failed with status {status}")),
        }
    }

    /// DELETE an object. Missing objects are treated as success so repeated
    /// migrations stay idempotent.
    pub async fn delete(&self, key: &str) -> StorageResult<()> {
        self.validate_key(key)?;
        let response = self
            .signed_request(reqwest::Method::DELETE, key, None)
            .await?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(format!(
            "S3 DELETE '{key}' failed with status {}",
            response.status()
        ))
    }

    // ── Internals ──────────────────────────────────────────────────────

    async fn signed_request(
        &self,
        method: reqwest::Method,
        key: &str,
        body: Option<Vec<u8>>,
    ) -> StorageResult<reqwest::Response> {
        let amz_date = amz_timestamp();
        let scope = format!("{}/{}/s3/aws4_request", amz_date.date, self.region);
        let canonical_headers = format!(
            "host:{host}\nx-amz-content-sha256:UNSIGNED-PAYLOAD\nx-amz-date:{date}\n",
            host = self.host_header(),
            date = amz_date.value,
        );
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{method}\n{path}\n\n{headers}\n{signed_headers}\nUNSIGNED-PAYLOAD",
            method = method,
            path = self.canonical_path(key),
            headers = canonical_headers,
        );
        let signature = self.sign_request(&amz_date, &canonical_request);
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );

        let url = format!(
            "{origin}{path}",
            origin = self.origin(),
            path = self.canonical_path(key)
        );
        let mut request = self
            .client
            .request(method, url)
            .header("x-amz-content-sha256", "UNSIGNED-PAYLOAD")
            .header("x-amz-date", &amz_date.value)
            .header("Authorization", authorization);
        if let Some(bytes) = body {
            request = request.body(bytes);
        }
        request
            .send()
            .await
            .map_err(|e| format!("S3 request for '{key}' failed: {e}"))
    }

    async fn check_status(response: reqwest::Response, operation: &str) -> StorageResult<()> {
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(format!(
            "S3 {operation} failed with status {status}: {body}"
        ))
    }

    fn sign_request(&self, amz_date: &AmzTimestamp, canonical_request: &str) -> String {
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{payload_hash}",
            amz_date = amz_date.value,
            scope = format!("{}/{}/s3/aws4_request", amz_date.date, self.region),
            payload_hash = hex::encode(Sha256::digest(canonical_request.as_bytes())),
        );
        let key = signing_key(&self.secret_key, &amz_date.date, &self.region, "s3");
        hex::encode(hmac_bytes(&key, string_to_sign.as_bytes()))
    }

    fn canonical_path(&self, key: &str) -> String {
        format!(
            "/{}/{}",
            encode_uri_component(&self.bucket),
            encode_uri_component(key)
        )
    }

    fn origin(&self) -> String {
        let mut origin = format!("{}://{}", self.endpoint.scheme(), self.host_header());
        if let Some(port) = self.endpoint.port() {
            let default_port = match self.endpoint.scheme() {
                "https" => 443,
                _ => 80,
            };
            if port != default_port {
                origin = format!(
                    "{}://{}:{}",
                    self.endpoint.scheme(),
                    self.endpoint.host_str().unwrap_or_default(),
                    port
                );
            }
        }
        origin
    }

    fn host_header(&self) -> String {
        let host = self.endpoint.host_str().unwrap_or_default();
        match self.endpoint.port() {
            Some(port) => {
                let default_port = match self.endpoint.scheme() {
                    "https" => 443,
                    _ => 80,
                };
                if port == default_port {
                    host.to_string()
                } else {
                    format!("{host}:{port}")
                }
            }
            None => host.to_string(),
        }
    }

    fn validate_key(&self, key: &str) -> StorageResult<()> {
        if key.is_empty()
            || key == "."
            || key == ".."
            || key.contains("..")
            || !key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
        {
            return Err(format!("Invalid S3 object key '{key}'"));
        }
        Ok(())
    }
}

struct AmzTimestamp {
    /// `YYYYMMDD` — the date stamp portion.
    date: String,
    /// `YYYYMMDDThhmmssZ` — the full timestamp.
    value: String,
}

fn amz_timestamp() -> AmzTimestamp {
    let now = chrono::Utc::now();
    let date = now.format("%Y%m%d").to_string();
    let value = now.format("%Y%m%dT%H%M%SZ").to_string();
    AmzTimestamp { date, value }
}

/// SigV4 signing key derivation:
/// `HMAC(HMAC(HMAC(HMAC("AWS4" + secret, date), region), service), "aws4_request")`
fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let date_key = hmac_bytes(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let region_key = hmac_bytes(&date_key, region.as_bytes());
    let service_key = hmac_bytes(&region_key, service.as_bytes());
    hmac_bytes(&service_key, b"aws4_request")
}

fn hmac_bytes(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Encode a URI component per SigV4 rules: keep `A-Za-z0-9-._~`, encode
/// everything else (including `/`, which is why object keys are validated to
/// a single safe segment).
fn encode_uri_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Canonical query string: params sorted by name, both name and value
/// URI-encoded, joined with `&`.
fn canonical_query_string(params: &[(&str, String)]) -> String {
    let mut encoded: Vec<(String, String)> = params
        .iter()
        .map(|(name, value)| (encode_uri_component(name), encode_uri_component(value)))
        .collect();
    encoded.sort();
    encoded
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── End-to-end against the documented AWS S3 SigV4 example ──────────
    //
    // AWS docs, "Authenticating Requests: Using the Authorization Header
    // (AWS Signature Version 4)" — Example: GET Object. Access key
    // AKIAIOSFODNN7EXAMPLE, secret wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY,
    // us-east-1, 20130524T000000Z. Verifies signing-key derivation,
    // string-to-sign construction, and the final signature.

    fn aws_example_storage() -> S3CompatibleStorage {
        S3CompatibleStorage::new(
            "https://s3.us-east-1.amazonaws.com",
            "us-east-1",
            "examplebucket",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        )
        .unwrap()
    }

    #[test]
    fn signature_matches_the_documented_aws_s3_example() {
        let storage = aws_example_storage();
        // sha256("") — the empty-payload hash used by the GET example.
        let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            hex::encode(Sha256::digest(b"")),
            empty_hash,
            "sanity: sha256 of the empty payload"
        );

        let canonical_request = format!(
            "GET\n/test.txt\n\nhost:examplebucket.s3.amazonaws.com\nrange:bytes=0-9\n\
             x-amz-content-sha256:{empty_hash}\nx-amz-date:20130524T000000Z\n\n\
             host;range;x-amz-content-sha256;x-amz-date\n{empty_hash}"
        );
        // sha256(canonical request) per the documented example.
        assert_eq!(
            hex::encode(Sha256::digest(canonical_request.as_bytes())),
            "7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972"
        );

        let timestamp = AmzTimestamp {
            date: "20130524".into(),
            value: "20130524T000000Z".into(),
        };
        assert_eq!(
            storage.sign_request(&timestamp, &canonical_request),
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41",
            "signature must match the documented AWS example"
        );
    }

    #[test]
    fn signing_key_is_deterministic() {
        let key = signing_key("secret", "20260829", "us-east-1", "s3");
        assert_eq!(key.len(), 32);
        assert_eq!(key, signing_key("secret", "20260829", "us-east-1", "s3"));
        assert_ne!(key, signing_key("secret", "20260830", "us-east-1", "s3"));
        assert_ne!(key, signing_key("other", "20260829", "us-east-1", "s3"));
    }

    #[test]
    fn uri_encoding_follows_sigv4_rules() {
        assert_eq!(encode_uri_component("a.zip"), "a.zip");
        assert_eq!(encode_uri_component("a b.zip"), "a%20b.zip");
        assert_eq!(encode_uri_component("a/b"), "a%2Fb");
    }

    #[test]
    fn canonical_query_is_sorted_and_encoded() {
        let query = canonical_query_string(&[
            ("X-Amz-Date", "20260829T000000Z".into()),
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".into()),
            (
                "X-Amz-Credential",
                "AKID/20260829/us-east-1/s3/aws4_request".into(),
            ),
        ]);
        assert!(query.starts_with(
            "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKID%2F20260829%2Fus-east-1%2Fs3%2Faws4_request&"
        ));
    }

    #[test]
    fn presigned_url_is_well_formed_and_expires_capped() {
        let storage = S3CompatibleStorage::new(
            "http://127.0.0.1:9000",
            "us-east-1",
            "bucket",
            "AKIDEXAMPLE",
            "secret",
        )
        .unwrap();

        let url = storage.presigned_get_url("one.zip", 900).unwrap();
        assert!(url.starts_with("http://127.0.0.1:9000/bucket/one.zip?"));
        for fragment in [
            "X-Amz-Algorithm=AWS4-HMAC-SHA256",
            "X-Amz-Credential=AKIDEXAMPLE%2F",
            "X-Amz-Expires=900",
            "X-Amz-SignedHeaders=host",
            "X-Amz-Signature=",
        ] {
            assert!(url.contains(fragment), "missing {fragment} in {url}");
        }

        assert!(storage.presigned_get_url("one.zip", 0).is_err());
        assert!(storage
            .presigned_get_url("one.zip", super::super::config::MAX_PRESIGN_SECONDS + 1)
            .is_err());
        assert!(storage.presigned_get_url("../evil.zip", 900).is_err());
    }
}
