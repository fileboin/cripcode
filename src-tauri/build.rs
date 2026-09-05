fn main() {
    ensure_askpass_placeholder();
    tauri_build::build()
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
