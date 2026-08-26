//! Background mode — prevents the system from sleeping when active tasks are running.
//!
//! Three modes:
//! - `Off`: Never prevent sleep. The OS manages power normally.
//! - `Smart` (default): Prevent sleep only when active tasks (dev servers, builds,
//!   SSH tunnels, agent sessions) are running. When all tasks complete, allow sleep.
//! - `AlwaysOn`: Always prevent sleep while Cripcode is running.
//!
//! Implementation: On macOS, uses `caffeinate -i` (prevents idle sleep).
//! On Linux, uses `systemd-inhibit` or `xdg-screensaver` (best-effort).
//! On Windows, uses `powercfg` request override (best-effort).
//! The power assertion is held in a tracked process that is killed when
//! sleep should be allowed again.

use crate::commands::setup::{read_app_state, write_app_state};
use crate::errors::CommandError;
use crate::utils::create_command;
use std::process::{Child, Stdio};
use std::sync::{LazyLock, Mutex};
use tracing::{info, warn};

/// Valid background modes.
const VALID_MODES: &[&str] = &["off", "smart", "always_on"];

/// In-memory holder for the current power assertion process (e.g. caffeinate).
static POWER_PROCESS: LazyLock<Mutex<Option<Child>>> = LazyLock::new(|| Mutex::new(None));

/// Get the current background mode from persisted AppState.
/// Returns "smart" if unset (the default).
#[tauri::command]
#[tracing::instrument]
pub fn get_background_mode() -> Result<String, CommandError> {
    Ok(read_app_state()
        .background_mode
        .unwrap_or_else(|| "smart".to_string()))
}

/// Set the background mode. Persists to AppState and immediately applies.
#[tauri::command]
#[tracing::instrument]
pub fn set_background_mode(mode: String) -> Result<(), CommandError> {
    let mode_trimmed = mode.trim().to_lowercase();
    if !VALID_MODES.contains(&mode_trimmed.as_str()) {
        return Err(CommandError::Validation {
            field: "mode".into(),
            reason: format!(
                "Invalid background mode `{mode_trimmed}` — expected one of: {}",
                VALID_MODES.join(", ")
            ),
        });
    }

    let mut state = read_app_state();
    state.background_mode = Some(mode_trimmed.clone());
    write_app_state(&state).map_err(CommandError::from)?;

    info!(mode = %mode_trimmed, "Background mode set");

    // Re-evaluate power state
    reevaluate_power();

    Ok(())
}

/// Report the number of active tasks (dev servers, builds, tunnels, agents).
/// Called by the frontend or internally to determine if SMART mode should
/// prevent sleep. When > 0, the system stays awake.
#[tauri::command]
#[tracing::instrument]
pub fn report_active_task_count(count: u32) -> Result<(), CommandError> {
    reevaluate_power_with_count(count);
    Ok(())
}

/// Check if the system is currently prevented from sleeping.
#[tauri::command]
#[tracing::instrument]
pub fn is_preventing_sleep() -> Result<bool, CommandError> {
    let is_holding = POWER_PROCESS
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false);
    Ok(is_holding)
}

/// Re-evaluate whether to hold or release the power assertion.
/// Uses a task count of 0 (the frontend should call `report_active_task_count`
/// before this if it knows the count).
fn reevaluate_power() {
    reevaluate_power_with_count(0);
}

/// Re-evaluate power state given an active task count.
fn reevaluate_power_with_count(count: u32) {
    let mode = read_app_state()
        .background_mode
        .unwrap_or_else(|| "smart".to_string());

    let should_prevent = match mode.as_str() {
        "off" => false,
        "always_on" => true,
        "smart" => count > 0,
        _ => false,
    };

    if should_prevent {
        acquire_power_assertion();
    } else {
        release_power_assertion();
    }
}

/// Start a power assertion process to prevent system sleep.
/// On macOS: `caffeinate -i` (prevent idle sleep).
/// On Linux: `systemd-inhibit --what=idle:sleep <wait>` (best-effort).
/// On Windows: no standard tool — we skip (Windows doesn't idle-sleep
/// as aggressively, and powercfg requires admin).
fn acquire_power_assertion() {
    // Already holding?
    if let Ok(mut guard) = POWER_PROCESS.lock() {
        if guard.is_some() {
            return; // Already preventing sleep
        }
    }

    let child = spawn_power_process();
    if let Some(child) = child {
        if let Ok(mut guard) = POWER_PROCESS.lock() {
            *guard = Some(child);
            info!("Power assertion acquired — system sleep prevented");
        }
    }
}

/// Release the power assertion — kill the process.
fn release_power_assertion() {
    if let Ok(mut guard) = POWER_PROCESS.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            info!("Power assertion released — system sleep allowed");
        }
    }
}

/// Spawn the platform-specific power assertion process.
fn spawn_power_process() -> Option<Child> {
    #[cfg(target_os = "macos")]
    {
        let result = create_command("caffeinate")
            .args(["-i"]) // Prevent idle sleep only (display can still dim)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match result {
            Ok(child) => Some(child),
            Err(e) => {
                warn!("Failed to start caffeinate: {e}");
                None
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Try systemd-inhibit first, fall back to nothing
        let result = create_command("systemd-inhibit")
            .args(["--what=idle:sleep", "--mode=block", "sleep", "infinity"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match result {
            Ok(child) => Some(child),
            Err(e) => {
                warn!("Failed to start systemd-inhibit: {e} — sleep prevention unavailable");
                None
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: SetThreadExecutionState is the proper API but requires
        // a native call. For MVP, we skip — Windows desktop apps generally
        // don't trigger idle sleep as aggressively as macOS.
        warn!("Background mode: Windows does not support power assertion via CLI in this version");
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_modes_are_correct() {
        assert!(VALID_MODES.contains(&"off"));
        assert!(VALID_MODES.contains(&"smart"));
        assert!(VALID_MODES.contains(&"always_on"));
        assert!(!VALID_MODES.contains(&"invalid"));
    }

    #[test]
    fn default_mode_is_smart() {
        let state = crate::types::AppState::default();
        assert_eq!(state.background_mode, None);
        // The get_background_mode command returns "smart" when None
        assert_eq!(
            state.background_mode.unwrap_or_else(|| "smart".to_string()),
            "smart"
        );
    }
}
