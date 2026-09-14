//! Android-only process-global app data directory.
//!
//! Path helpers without an `AppHandle` cannot call `app.path()` themselves.
//! `.setup()` stores the resolved directory here before the Android-only
//! startup initialization that needs those helpers.

use once_cell::sync::OnceCell;
use std::path::PathBuf;

static APP_DATA_DIR: OnceCell<PathBuf> = OnceCell::new();

pub fn set_app_data_dir(dir: PathBuf) {
    let _ = APP_DATA_DIR.set(dir);
}

pub fn app_data_dir() -> Option<&'static PathBuf> {
    APP_DATA_DIR.get()
}
