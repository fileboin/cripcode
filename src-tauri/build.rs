fn main() {
    expose_android_version_code();
    ensure_askpass_placeholder();
    tauri_build::build()
}

/// Android-only: forward the generated `versionCode` into the binary so the
/// in-app updater can compare numeric version codes (`android_update`). The
/// value is written by the Tauri CLI into `gen/android/app/tauri.properties`
/// before cargo runs; a missing file simply leaves the fallback in place.
fn expose_android_version_code() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("android") {
        return;
    }
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => dir,
        Err(_) => return,
    };
    let properties = std::path::Path::new(&manifest_dir).join("gen/android/app/tauri.properties");
    println!("cargo:rerun-if-changed={}", properties.display());
    let Ok(contents) = std::fs::read_to_string(&properties) else {
        return;
    };
    if let Some(code) = contents
        .lines()
        .find_map(|line| line.strip_prefix("tauri.android.versionCode="))
    {
        println!("cargo:rustc-env=ANDROID_VERSION_CODE={}", code.trim());
    }
}

/// `bundle.externalBin` declares the ssh-askpass sidecar, and tauri-build
/// refuses to build when the declared path does not exist. On a fresh clone
/// nothing has produced the helper yet, so place a zero-byte placeholder for
/// the current target before the validation runs.
///
/// The real helper is built by cargo (`src/bin/ssh-askpass.rs`) and copied
/// over the placeholder by `pnpm copy:askpass` in `beforeBundleCommand`
/// (see `scripts/copy-askpass.js`), so a shipped bundle always carries the
/// working binary. In dev, the app resolves the helper next to the running
/// executable in `target/debug`, which cargo produces directly.
fn ensure_askpass_placeholder() {
    let target = match std::env::var("TARGET") {
        Ok(t) => t,
        Err(_) => return, // no target known — leave validation to tauri-build
    };
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(d) => d,
        Err(_) => return,
    };
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let placeholder = std::path::Path::new(&manifest_dir)
        .join("binaries")
        .join(format!("ssh-askpass-{target}{ext}"));
    if !placeholder.exists() {
        if let Some(parent) = placeholder.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&placeholder, b"");
    }
}
