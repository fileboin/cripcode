//! In-app Android update: check, verified download, installer handoff.
//!
//! Android has no Tauri updater support, so this module implements the same
//! user-confirmed flow with the platform's own package installer:
//!
//! 1. fetch a fixed, trusted metadata JSON (`android-latest.json`),
//! 2. compare its numeric `versionCode` with the running app's versionCode,
//! 3. download the APK and verify its SHA-256 before touching it,
//! 4. hand the file to Android's installer Intent (`ACTION_VIEW` +
//!    `application/vnd.android.package-archive` via the existing FileProvider).
//!
//! Silent installation is never attempted and no root/ADB is required — the
//! OS shows its own confirmation UI, and Android itself refuses any APK that
//! is not signed with the same release key.
//!
//! @module commands/android_update

use crate::errors::CommandError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Fixed, trusted metadata source (public GitLab generic package).
pub(crate) const UPDATE_METADATA_URL: &str =
    "https://gitlab.com/api/v4/projects/eurokrater%2Fcripcode/packages/generic/cripcode/android-latest/android-latest.json";

/// Every APK URL we accept must come from this distribution prefix. The URL is
/// never taken from user input — only from the trusted metadata document.
pub(crate) const APK_URL_PREFIX: &str =
    "https://gitlab.com/api/v4/projects/eurokrater%2Fcripcode/packages/generic/cripcode/";

/// Downloaded APK file name inside the app cache dir.
const APK_FILE_NAME: &str = "CripCode-update.apk";

/// Metadata document shape published alongside each release.
#[derive(Debug, Deserialize)]
struct UpdateManifest {
    version_name: String,
    version_code: u64,
    apk_url: String,
    sha256: String,
    #[serde(default)]
    notes: Option<String>,
}

/// Result of an update check, shaped for the frontend (camelCase).
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AndroidUpdateStatus {
    /// True only when the remote versionCode is strictly greater.
    pub available: bool,
    /// versionCode of the running app (from the Android build, not the label).
    pub local_version_code: u64,
    pub version_name: Option<String>,
    pub version_code: Option<u64>,
    pub apk_url: Option<String>,
    pub sha256: Option<String>,
    pub notes: Option<String>,
    /// Set when the check itself failed (offline, malformed metadata). The UI
    /// treats this as "no update" and never blocks startup.
    pub error: Option<String>,
}

/// The running app's numeric versionCode. The Android build writes it into
/// `gen/android/app/tauri.properties`; `build.rs` forwards it here at compile
/// time. Falls back to the semver formula Tauri uses when the value is absent
/// (e.g. desktop builds, unit tests).
fn local_version_code() -> u64 {
    if let Some(code) = option_env!("ANDROID_VERSION_CODE") {
        if let Ok(parsed) = code.trim().parse::<u64>() {
            return parsed;
        }
    }
    semver_to_version_code(env!("CARGO_PKG_VERSION"))
}

/// Tauri's default Android versionCode math: major * 1_000_000 + minor * 1_000 + patch.
fn semver_to_version_code(version: &str) -> u64 {
    let mut parts = version.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    major * 1_000_000 + minor * 1_000 + patch
}

/// Strictly-greater comparison — equal or lower remote versions never prompt.
fn is_newer(local: u64, remote: u64) -> bool {
    remote > local
}

/// A SHA-256 digest as published: exactly 64 lowercase/uppercase hex chars.
fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Only APK files from our own distribution prefix are accepted.
fn is_trusted_apk_url(url: &str) -> bool {
    url.starts_with(APK_URL_PREFIX) && url.ends_with(".apk")
}

fn unavailable(local: u64, error: impl Into<String>) -> AndroidUpdateStatus {
    AndroidUpdateStatus {
        available: false,
        local_version_code: local,
        version_name: None,
        version_code: None,
        apk_url: None,
        sha256: None,
        notes: None,
        error: Some(error.into()),
    }
}

/// Check the trusted metadata endpoint and compare versionCodes.
///
/// Never returns `Err` for network/metadata problems: the frontend treats any
/// failure as "no update available" so startup and the rest of the app are
/// never blocked by an unreachable endpoint.
#[tauri::command]
#[tracing::instrument]
pub async fn android_update_check() -> Result<AndroidUpdateStatus, CommandError> {
    let local = local_version_code();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| CommandError::Io {
            message: format!("Could not create the update client: {e}"),
        })?;

    let response = match client.get(UPDATE_METADATA_URL).send().await {
        Ok(response) => response,
        Err(e) => {
            return Ok(unavailable(
                local,
                format!("Update metadata unreachable: {e}"),
            ))
        }
    };
    let response = match response.error_for_status() {
        Ok(response) => response,
        Err(e) => {
            return Ok(unavailable(
                local,
                format!("Update metadata request failed: {e}"),
            ))
        }
    };
    let manifest: UpdateManifest = match response.json().await {
        Ok(manifest) => manifest,
        Err(e) => {
            return Ok(unavailable(
                local,
                format!("Update metadata is malformed: {e}"),
            ))
        }
    };

    if !is_trusted_apk_url(&manifest.apk_url) {
        return Ok(unavailable(
            local,
            "Update metadata rejected: APK is not from the trusted distribution source",
        ));
    }
    if !is_valid_sha256(&manifest.sha256) {
        return Ok(unavailable(
            local,
            "Update metadata rejected: invalid SHA-256",
        ));
    }

    if !is_newer(local, manifest.version_code) {
        return Ok(AndroidUpdateStatus {
            available: false,
            local_version_code: local,
            version_name: None,
            version_code: None,
            apk_url: None,
            sha256: None,
            notes: None,
            error: None,
        });
    }

    Ok(AndroidUpdateStatus {
        available: true,
        local_version_code: local,
        version_name: Some(manifest.version_name),
        version_code: Some(manifest.version_code),
        apk_url: Some(manifest.apk_url),
        sha256: Some(manifest.sha256.to_lowercase()),
        notes: manifest.notes,
        error: None,
    })
}

/// Download the APK into the app cache and verify its SHA-256 before it is
/// ever handed to the installer. A digest mismatch is a hard rejection: the
/// partial file is deleted and an error is returned.
#[tauri::command]
#[tracing::instrument(skip(app))]
pub async fn android_update_download(
    app: tauri::AppHandle,
    apk_url: String,
    sha256: String,
) -> Result<String, CommandError> {
    if !is_trusted_apk_url(&apk_url) {
        return Err(CommandError::expected(
            "Refusing to download an APK from an untrusted source.",
        ));
    }
    if !is_valid_sha256(&sha256) {
        return Err(CommandError::expected(
            "Refusing to download: the update has no valid SHA-256.",
        ));
    }

    use tauri::Manager;
    let cache_dir = app.path().app_cache_dir().map_err(|e| CommandError::Io {
        message: format!("Could not resolve the app cache directory: {e}"),
    })?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| CommandError::Io {
        message: format!("Could not create the app cache directory: {e}"),
    })?;

    let final_path = cache_dir.join(APK_FILE_NAME);
    let temp_path = cache_dir.join(format!(".{APK_FILE_NAME}.tmp"));
    let _ = std::fs::remove_file(&temp_path);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| CommandError::Io {
            message: format!("Could not create the download client: {e}"),
        })?;

    let mut response = client
        .get(&apk_url)
        .send()
        .await
        .map_err(|e| CommandError::Io {
            message: format!("Download failed: {e}"),
        })?;
    if !response.status().is_success() {
        return Err(CommandError::Io {
            message: format!("Download failed with HTTP {}", response.status()),
        });
    }

    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| CommandError::Io {
            message: format!("Could not create the download file: {e}"),
        })?;
    let mut hasher = Sha256::new();

    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = response.chunk().await.map_err(|e| CommandError::Io {
        message: format!("Download interrupted: {e}"),
    })? {
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| CommandError::Io {
            message: format!("Could not write the download file: {e}"),
        })?;
    }
    file.flush().await.map_err(|e| CommandError::Io {
        message: format!("Could not flush the download file: {e}"),
    })?;
    drop(file);

    let actual = format!("{:x}", hasher.finalize());
    if actual != sha256.to_lowercase() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(CommandError::expected(
            "Update rejected: the downloaded file does not match the published SHA-256.",
        ));
    }

    std::fs::rename(&temp_path, &final_path).map_err(|e| CommandError::Io {
        message: format!("Could not finalize the downloaded APK: {e}"),
    })?;

    Ok(final_path.to_string_lossy().to_string())
}

/// Hand the verified APK to Android's package installer.
///
/// The path must be the file produced by [`android_update_download`] inside the
/// app cache dir — nothing else is ever passed to the installer.
#[tauri::command]
#[tracing::instrument(skip(app))]
pub fn android_update_install(app: tauri::AppHandle, path: String) -> Result<(), CommandError> {
    use tauri::Manager;
    let cache_dir = app.path().app_cache_dir().map_err(|e| CommandError::Io {
        message: format!("Could not resolve the app cache directory: {e}"),
    })?;
    let expected = cache_dir.join(APK_FILE_NAME);
    let provided = std::path::PathBuf::from(&path);
    let same_file = std::fs::canonicalize(&provided)
        .ok()
        .zip(std::fs::canonicalize(&expected).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if !same_file {
        return Err(CommandError::expected(
            "Refusing to install: unexpected APK path.",
        ));
    }
    if !provided.exists() {
        return Err(CommandError::expected(
            "The downloaded update file is missing. Download it again.",
        ));
    }

    start_installer(&provided.to_string_lossy())
}

/// Android-only installer handoff. Uses the FileProvider declared in the
/// manifest (`${applicationId}.fileprovider`, cache-path root) so the APK can
/// be exposed as a `content://` URI to the package installer.
#[cfg(target_os = "android")]
fn start_installer(apk_path: &str) -> Result<(), CommandError> {
    use jni::objects::{JObject, JValue};
    use jni::sys::jobject;

    fn jerr(context: &str) -> impl Fn(jni::errors::Error) -> CommandError + '_ {
        move |e| CommandError::Io {
            message: format!("{context}: {e}"),
        }
    }

    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| CommandError::Io {
        message: format!("Android JNI VM is unavailable: {e}"),
    })?;
    let mut env = vm.attach_current_thread().map_err(|e| CommandError::Io {
        message: format!("Could not attach to the Android JNI thread: {e}"),
    })?;

    let activity = unsafe { JObject::from_raw(ctx.context() as jobject) };

    let path = env
        .new_string(apk_path)
        .map_err(jerr("Could not pass the APK path to the installer"))?;
    let file_class = env
        .find_class("java/io/File")
        .map_err(jerr("Could not find java.io.File"))?;
    let file = env
        .new_object(
            &file_class,
            "(Ljava/lang/String;)V",
            &[JValue::Object(&path)],
        )
        .map_err(jerr("Could not open the APK file"))?;

    // Resolve FileProvider through the activity's class loader: a bare
    // find_class from a native thread can miss app/library classes.
    let loader = env
        .call_method(
            &activity,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .map_err(jerr("Could not read the activity class loader"))?
        .l()
        .map_err(jerr("Could not read the activity class loader"))?;
    let provider_name = env
        .new_string("androidx.core.content.FileProvider")
        .map_err(jerr("Could not pass the FileProvider name"))?;
    let provider_class: jni::objects::JClass = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&provider_name)],
        )
        .map_err(jerr("Could not load FileProvider"))?
        .l()
        .map_err(jerr("Could not load FileProvider"))?
        .into();
    let authority = env
        .new_string("com.fileboin.cripcode.fileprovider")
        .map_err(jerr("Could not pass the FileProvider authority"))?;
    let uri = env
        .call_static_method(
            &provider_class,
            "getUriForFile",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
            &[
                JValue::Object(&activity),
                JValue::Object(&authority),
                JValue::Object(&file),
            ],
        )
        .map_err(jerr("Could not create the APK content URI"))?
        .l()
        .map_err(jerr("Could not create the APK content URI"))?;

    let action = env
        .new_string("android.intent.action.VIEW")
        .map_err(jerr("Could not build the installer intent"))?;
    let intent_class = env
        .find_class("android/content/Intent")
        .map_err(jerr("Could not find android.content.Intent"))?;
    let intent = env
        .new_object(
            &intent_class,
            "(Ljava/lang/String;)V",
            &[JValue::Object(&action)],
        )
        .map_err(jerr("Could not build the installer intent"))?;
    let mime = env
        .new_string("application/vnd.android.package-archive")
        .map_err(jerr("Could not build the installer intent"))?;
    let intent = env
        .call_method(
            &intent,
            "setDataAndType",
            "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&uri), JValue::Object(&mime)],
        )
        .map_err(jerr("Could not set the installer intent data"))?
        .l()
        .map_err(jerr("Could not set the installer intent data"))?;

    // FLAG_ACTIVITY_NEW_TASK | FLAG_GRANT_READ_URI_PERMISSION
    const INSTALL_FLAGS: i32 = 0x1000_0000 | 0x0000_0001;
    let intent = env
        .call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(INSTALL_FLAGS)],
        )
        .map_err(jerr("Could not set the installer intent flags"))?
        .l()
        .map_err(jerr("Could not set the installer intent flags"))?;

    env.call_method(
        &activity,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent)],
    )
    .map_err(jerr("Could not launch the Android installer"))?;

    Ok(())
}

/// Desktop builds have no Android installer; the commands are Android-only in
/// `lib.rs`, so this exists purely to keep the module compiling everywhere.
#[cfg(not(target_os = "android"))]
fn start_installer(_apk_path: &str) -> Result<(), CommandError> {
    Err(CommandError::expected(
        "In-app updates are only available on Android.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_maps_to_tauri_version_code() {
        assert_eq!(semver_to_version_code("0.18.8"), 18008);
        assert_eq!(semver_to_version_code("0.18.9"), 18009);
        assert_eq!(semver_to_version_code("1.2.3"), 1_002_003);
        assert_eq!(semver_to_version_code("garbage"), 0);
    }

    #[test]
    fn only_strictly_newer_remote_versions_are_offered() {
        assert!(!is_newer(18008, 18008), "same version must not prompt");
        assert!(!is_newer(18008, 18007), "lower version must not prompt");
        assert!(is_newer(18008, 18009), "higher version must prompt");
    }

    #[test]
    fn sha256_validation_accepts_only_hex_digests() {
        assert!(is_valid_sha256(&"a".repeat(64)));
        assert!(is_valid_sha256(&"A1".repeat(32)));
        assert!(!is_valid_sha256(&"a".repeat(63)));
        assert!(!is_valid_sha256(&"z".repeat(64)));
        assert!(!is_valid_sha256(""));
    }

    #[test]
    fn only_our_distribution_prefix_is_trusted() {
        assert!(is_trusted_apk_url(&format!(
            "{APK_URL_PREFIX}0.18.11/CripCode-0.18.11.apk"
        )));
        assert!(!is_trusted_apk_url("https://evil.example.com/CripCode.apk"));
        assert!(!is_trusted_apk_url(&format!(
            "{APK_URL_PREFIX}0.18.11/readme.txt"
        )));
        assert!(!is_trusted_apk_url(""));
    }

    #[test]
    fn failed_checks_never_report_an_update() {
        let status = unavailable(18008, "offline");
        assert!(!status.available);
        assert_eq!(status.local_version_code, 18008);
        assert_eq!(status.error.as_deref(), Some("offline"));
    }
}
