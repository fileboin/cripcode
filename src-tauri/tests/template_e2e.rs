//! Real-process E2E for the CripCode Community Templates flow.
//!
//! Spawns the standalone `cripcode-template-api` binary as a real child
//! process and drives the *actual* Tauri command functions end to end:
//! list -> details -> signed ZIP download -> `extract_template_zip` -> a
//! real project directory on disk. Negative cases assert fail-safe errors
//! (no panics, no crashes) for an unavailable API, an empty registry, a
//! registry that refuses to load, missing/invalid ZIP objects, a missing
//! thumbnail, and a tampered signed URL.

use ship_studio_lib::commands::projects::extract_template_zip;
use ship_studio_lib::commands::templates::{
    download_template_zip, fetch_community_templates, fetch_template_details,
};
use ship_studio_lib::template_api::model::{TemplateListResponse, TemplateMetadata};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ServerGuard {
    child: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind for free port");
    listener.local_addr().expect("local addr").port()
}

fn temp_data_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cripcode-tpl-e2e-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create E2E data dir");
    dir
}

fn spawn_server(data_dir: &Path, port: u16) -> ServerGuard {
    let child = Command::new(env!("CARGO_BIN_EXE_cripcode-template-api"))
        .env("CRIPCODE_TEMPLATE_API_BIND", format!("127.0.0.1:{port}"))
        .env(
            "CRIPCODE_TEMPLATE_API_PUBLIC_BASE_URL",
            format!("http://127.0.0.1:{port}"),
        )
        .env("CRIPCODE_TEMPLATE_API_DATA_DIR", data_dir)
        .env("CRIPCODE_TEMPLATE_API_SIGNING_SECRET", "e2e-test-secret")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cripcode-template-api");
    ServerGuard { child }
}

fn wait_for_tcp(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        if Instant::now() > deadline {
            panic!("template API did not start within 20s");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Wait for the child to exit. Returns `Some(success)` once it has exited,
/// or `None` if it is still running at the deadline (which for these tests
/// means the server *started* — a failure the caller must flag).
fn wait_for_exit(child: &mut Child) -> Option<bool> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            return Some(status.success());
        }
        if Instant::now() > deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start_seeded_server() -> (ServerGuard, String, PathBuf) {
    let data_dir = temp_data_dir("seeded");
    let port = free_port();
    let guard = spawn_server(&data_dir, port);
    wait_for_tcp(port);
    (guard, format!("http://127.0.0.1:{port}"), data_dir)
}

fn remove_dir_quiet(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

/// The full happy-path chain, every hop against a real server process:
/// `fetch_community_templates` -> `fetch_template_details` ->
/// `download_template_zip` (signed URL) -> `extract_template_zip` ->
/// a real project directory with the seeded template's files.
#[tokio::test]
async fn full_flow_creates_a_project_from_a_community_template() {
    let (guard, base_url, data_dir) = start_seeded_server();

    let raw_list =
        fetch_community_templates(Some(base_url.clone()), None, None, None, None, None, None)
            .await
            .expect("fetch community templates");
    let list: TemplateListResponse = serde_json::from_str(&raw_list).expect("parse template list");
    assert_eq!(list.total, 1);
    let template = list.templates.first().expect("seeded template");
    assert_eq!(template.id, "cripcode-test-starter");
    assert!(template
        .download
        .url
        .as_deref()
        .is_some_and(|url| url.starts_with(&format!("{base_url}/storage/"))));

    let raw_details = fetch_template_details(template.id.clone(), Some(base_url.clone()))
        .await
        .expect("fetch template details");
    let details: TemplateMetadata =
        serde_json::from_str(&raw_details).expect("parse template details");
    assert_eq!(details.id, template.id);
    assert!(details.download.size_bytes.is_some_and(|size| size > 0));

    let download_url = details.download.url.expect("signed download URL");
    let zip_path = download_template_zip(download_url)
        .await
        .expect("download template zip");
    let zip_bytes = std::fs::read(&zip_path).expect("read downloaded zip");
    assert_eq!(&zip_bytes[..2], b"PK", "downloaded file must be a zip");

    let project_name = format!(
        "e2e-community-template-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let project_path = extract_template_zip(project_name.clone(), None, Some(zip_path.clone()))
        .await
        .expect("extract template zip into a project");
    let project_dir = PathBuf::from(&project_path);
    assert!(project_dir.join("index.html").is_file());
    let html = std::fs::read_to_string(project_dir.join("index.html")).expect("read index.html");
    assert!(html.contains("CripCode test template"));

    let _ = std::fs::remove_file(&zip_path);
    remove_dir_quiet(&project_dir);
    remove_dir_quiet(&data_dir);
    drop(guard);
}

/// Every negative case must surface as a clean error — never a panic or a
/// crash — at each layer of the chain.
#[tokio::test]
async fn negative_api_cases_fail_safe() {
    // 1. API unavailable: unreachable base URL -> clean command error.
    let unreachable = fetch_community_templates(
        Some("http://127.0.0.1:9".into()),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await;
    assert!(unreachable.is_err(), "unreachable API must error, not hang");
    assert!(unreachable
        .unwrap_err()
        .to_string()
        .contains("Failed to fetch templates"));

    // 2. Empty registry: seeds nothing, lists zero, details 404.
    let empty_dir = temp_data_dir("empty");
    std::fs::write(empty_dir.join("templates.json"), "[]").expect("write empty registry");
    let empty_port = free_port();
    let empty_guard = spawn_server(&empty_dir, empty_port);
    wait_for_tcp(empty_port);
    let empty_base = format!("http://127.0.0.1:{empty_port}");

    let raw_list =
        fetch_community_templates(Some(empty_base.clone()), None, None, None, None, None, None)
            .await
            .expect("empty registry must still list");
    let list: TemplateListResponse = serde_json::from_str(&raw_list).expect("parse empty list");
    assert_eq!(list.total, 0);
    assert!(list.templates.is_empty());

    let missing = fetch_template_details("does-not-exist".into(), Some(empty_base.clone())).await;
    assert!(missing.is_err());
    assert!(missing
        .unwrap_err()
        .to_string()
        .contains("API returned status 404"));
    drop(empty_guard);
    remove_dir_quiet(&empty_dir);

    // 3. Seeded server: tampered signed URL -> 403, missing id -> 404.
    let (guard, base_url, data_dir) = start_seeded_server();

    let missing_seeded =
        fetch_template_details("missing-template".into(), Some(base_url.clone())).await;
    assert!(missing_seeded.is_err());
    assert!(missing_seeded
        .unwrap_err()
        .to_string()
        .contains("API returned status 404"));

    let raw_details =
        fetch_template_details("cripcode-test-starter".into(), Some(base_url.clone()))
            .await
            .expect("details for tamper test");
    let details: TemplateMetadata =
        serde_json::from_str(&raw_details).expect("parse details for tamper test");
    let signed_url = details.download.url.expect("signed URL");

    let tampered =
        download_template_zip(signed_url.replace("signature=", "signature=tampered")).await;
    assert!(tampered.is_err());
    assert!(tampered
        .unwrap_err()
        .to_string()
        .contains("Download failed with status 403"));

    drop(guard);
    remove_dir_quiet(&data_dir);
}

/// A registry that cannot be trusted (malformed JSON, or records with unsafe
/// object keys) must refuse to start the server at all.
#[test]
fn malformed_registry_refuses_to_start() {
    for (label, contents) in [
        ("not-json", "not json at all"),
        (
            "unsafe-key",
            r#"[{"id":"bad","name":"Bad","description":"Bad","author":"X","category":"dev","framework":"HTML","thumbnail_key":null,"zip_key":"../evil.zip","version":"1.0.0","created_at":"2026-08-29T00:00:00Z","updated_at":"2026-08-29T00:00:00Z"}]"#,
        ),
    ] {
        let data_dir = temp_data_dir(label);
        std::fs::write(data_dir.join("templates.json"), contents).expect("write bad registry");
        let port = free_port();
        let mut guard = spawn_server(&data_dir, port);
        let outcome = wait_for_exit(&mut guard.child);
        assert_eq!(
            outcome,
            Some(false),
            "server with {label} registry must exit with an error"
        );
        remove_dir_quiet(&data_dir);
    }
}

/// Deleting the ZIP/thumbnail objects out from under a running server must
/// degrade into clean errors: listing reports the storage failure, the signed
/// download 404s, and the thumbnail URL 404s. No crashes.
#[tokio::test]
async fn missing_zip_and_thumbnail_fail_safe() {
    let (guard, base_url, data_dir) = start_seeded_server();

    let raw_details =
        fetch_template_details("cripcode-test-starter".into(), Some(base_url.clone()))
            .await
            .expect("details before objects are removed");
    let details: TemplateMetadata =
        serde_json::from_str(&raw_details).expect("parse details before removal");
    let signed_url = details.download.url.expect("signed URL");
    let thumbnail_url = details.thumbnail.expect("thumbnail URL");

    std::fs::remove_file(data_dir.join("objects").join("cripcode-test-starter.zip"))
        .expect("remove zip object");
    std::fs::remove_file(data_dir.join("objects").join("cripcode-test-starter.svg"))
        .expect("remove thumbnail object");

    // Listing now fails with a 500 because metadata can't resolve the zip
    // size — a clean command error, not a crash.
    let broken_list =
        fetch_community_templates(Some(base_url.clone()), None, None, None, None, None, None).await;
    assert!(
        broken_list.is_err(),
        "list must fail cleanly on missing zip"
    );
    assert!(broken_list
        .unwrap_err()
        .to_string()
        .contains("API returned status 500"));

    // The previously signed URL now 404s.
    let gone = download_template_zip(signed_url).await;
    assert!(gone.is_err());
    assert!(gone
        .unwrap_err()
        .to_string()
        .contains("Download failed with status 404"));

    // The thumbnail URL 404s; the UI placeholder covers this in the app.
    let client = reqwest::Client::new();
    let thumbnail = client
        .get(&thumbnail_url)
        .send()
        .await
        .expect("thumbnail request must complete");
    assert_eq!(
        thumbnail.status(),
        reqwest::StatusCode::NOT_FOUND,
        "missing thumbnail must 404"
    );

    drop(guard);
    remove_dir_quiet(&data_dir);
}

/// A corrupt ZIP that still downloads fine must fail extraction with a clean
/// error and leave no half-extracted project behind.
#[tokio::test]
async fn invalid_zip_fails_extraction_without_leaving_a_project() {
    let data_dir = temp_data_dir("corrupt");
    // Pre-place a garbage zip so the server's seeding step skips it.
    std::fs::create_dir_all(data_dir.join("objects")).expect("create objects dir");
    std::fs::write(
        data_dir.join("objects").join("corrupt.zip"),
        b"this is not a zip file",
    )
    .expect("write corrupt zip");
    let registry = r#"[{"id":"corrupt","name":"Corrupt","description":"Corrupt zip","author":"E2E","category":"dev","framework":"HTML","thumbnail_key":null,"zip_key":"corrupt.zip","version":"1.0.0","created_at":"2026-08-29T00:00:00Z","updated_at":"2026-08-29T00:00:00Z"}]"#;
    std::fs::write(data_dir.join("templates.json"), registry).expect("write corrupt registry");

    let port = free_port();
    let guard = spawn_server(&data_dir, port);
    wait_for_tcp(port);
    let base_url = format!("http://127.0.0.1:{port}");

    let raw_details = fetch_template_details("corrupt".into(), Some(base_url.clone()))
        .await
        .expect("details for corrupt zip");
    let details: TemplateMetadata =
        serde_json::from_str(&raw_details).expect("parse corrupt details");
    let zip_path = download_template_zip(details.download.url.expect("signed URL"))
        .await
        .expect("garbage bytes still download over a signed URL");

    let project_name = format!(
        "e2e-corrupt-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let extraction = extract_template_zip(project_name.clone(), None, Some(zip_path.clone())).await;
    assert!(extraction.is_err(), "corrupt zip must fail extraction");

    // No half-extracted project directory may survive the failure.
    let projects_root = ship_studio_lib::utils::projects_root().expect("resolve projects root");
    assert!(!projects_root.join(&project_name).exists());

    let _ = std::fs::remove_file(&zip_path);
    drop(guard);
    remove_dir_quiet(&data_dir);
}
