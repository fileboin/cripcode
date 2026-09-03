//! Remote preview via SSH port forwarding.
//!
//! Creates an SSH tunnel (`ssh -L local_port:localhost:remote_port user@host`)
//! so the remote dev server's port is accessible on localhost. The existing
//! preview proxy can then connect to the forwarded local port as if the dev
//! server were running locally.
//!
//! The tunnel process is tracked by PID so it can be stopped when the preview
//! is closed. A probe command checks whether the forwarded port is actually
//! serving HTTP (i.e., the remote dev server is up).

use super::config;
use super::connection::build_ssh_connection_args;
use crate::errors::CommandError;
use crate::utils::create_command;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Timeout for the SSH tunnel spawn.
const SSH_TUNNEL_TIMEOUT_SECS: u64 = 10;

/// In-memory registry of active SSH tunnels, keyed by `(server_id, remote_port)`.
/// Maps to the local port and the PID of the `ssh -L` process.
static TUNNELS: LazyLock<Mutex<HashMap<String, TunnelInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
struct TunnelInfo {
    local_port: u16,
    remote_port: u16,
    pid: u32,
}

/// Look up a server by ID.
fn get_server(server_id: &str) -> Result<crate::types::SshServer, CommandError> {
    let config = config::load_config_pub().map_err(CommandError::from)?;
    config
        .servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| CommandError::Validation {
            field: "server_id".into(),
            reason: format!("No SSH server found with id `{server_id}`"),
        })
}

fn tunnel_key(server_id: &str, remote_port: u16) -> String {
    format!("{}:{}", server_id, remote_port)
}

/// Remote preview status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePreviewStatus {
    /// Whether the SSH tunnel is active.
    pub tunnel_active: bool,
    /// The local port the tunnel is listening on.
    pub local_port: Option<u16>,
    /// The remote port being forwarded.
    pub remote_port: Option<u16>,
    /// Whether the remote dev server is responding on the forwarded port.
    pub server_responding: bool,
    /// HTTP status code from the probe (None if unreachable).
    pub http_status: Option<u16>,
    /// Error message if the tunnel or probe failed.
    pub error: Option<String>,
}

/// Start an SSH port-forwarding tunnel for a remote dev server.
///
/// Runs `ssh -L local_port:localhost:remote_port -N user@host` in the
/// background. `-N` means "no remote command" — the tunnel is the only
/// purpose. The process stays alive until explicitly killed.
///
/// If a tunnel for the same `(server_id, remote_port)` already exists,
/// this is a no-op (returns the existing local port).
#[tauri::command]
#[tracing::instrument]
pub async fn start_remote_preview_tunnel(
    server_id: String,
    remote_port: u16,
    local_port: Option<u16>,
) -> Result<u16, CommandError> {
    let server = get_server(&server_id)?;
    let key = tunnel_key(&server_id, remote_port);

    // Check if tunnel already exists
    if let Ok(tunnels) = TUNNELS.lock() {
        if let Some(info) = tunnels.get(&key) {
            return Ok(info.local_port);
        }
    }

    // Pick a local port: use the provided one, or default to remote_port + 10000
    // (e.g., remote 3000 → local 13000) to avoid conflicts.
    let chosen_local = local_port.unwrap_or(remote_port + 10000);

    // Build SSH args with -L forwarding and -N (no command)
    let mut args = vec![
        "-L".into(),
        format!("{}:localhost:{}", chosen_local, remote_port),
        "-N".into(),
    ];

    // Add the standard connection args (port, key, etc.) without a remote
    // command. The tunnel itself uses -N.
    let conn_args = build_ssh_connection_args(&server);
    args.extend(conn_args);

    // Spawn the SSH tunnel process
    let pid: Option<u32>;

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = create_command("ssh");
        cmd.args(&args);
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let child = cmd.spawn().map_err(|e| CommandError::Io {
            message: format!("Failed to start SSH tunnel: {e}"),
        })?;

        pid = Some(child.id());
        std::mem::forget(child);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut cmd = create_command("ssh");
        cmd.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);

        let child = cmd.spawn().map_err(|e| CommandError::Io {
            message: format!("Failed to start SSH tunnel: {e}"),
        })?;

        pid = Some(child.id());
        std::mem::forget(child);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        pid = None;
    }

    tracing::info!(
        server = %server.name,
        local_port = chosen_local,
        remote_port,
        "SSH tunnel started"
    );

    // Register the tunnel
    if let Ok(mut tunnels) = TUNNELS.lock() {
        tunnels.insert(
            key,
            TunnelInfo {
                local_port: chosen_local,
                remote_port,
                pid: pid.unwrap_or(0),
            },
        );
    }

    // Give the tunnel a moment to establish
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    Ok(chosen_local)
}

/// Stop the SSH tunnel for a remote dev server.
#[tauri::command]
#[tracing::instrument]
pub async fn stop_remote_preview_tunnel(
    server_id: String,
    remote_port: u16,
) -> Result<(), CommandError> {
    let key = tunnel_key(&server_id, remote_port);

    let info = if let Ok(mut tunnels) = TUNNELS.lock() {
        tunnels.remove(&key)
    } else {
        None
    };

    if let Some(info) = info {
        // Kill the SSH tunnel process
        #[cfg(unix)]
        {
            let _ = create_command("kill")
                .arg("-9")
                .arg(info.pid.to_string())
                .output();
        }
        #[cfg(windows)]
        {
            let _ = create_command("taskkill")
                .args(["/F", "/T", "/PID", &info.pid.to_string()])
                .output();
        }
        tracing::info!(
            local_port = info.local_port,
            remote_port = info.remote_port,
            "SSH tunnel stopped"
        );
    }

    Ok(())
}

/// Check the status of the remote preview: tunnel active? server responding?
#[tauri::command]
#[tracing::instrument]
pub async fn get_remote_preview_status(
    server_id: String,
    remote_port: u16,
) -> Result<RemotePreviewStatus, CommandError> {
    let key = tunnel_key(&server_id, remote_port);

    let info = if let Ok(tunnels) = TUNNELS.lock() {
        tunnels.get(&key).cloned()
    } else {
        None
    };

    match info {
        Some(info) => {
            // Probe the local port to see if the dev server is responding
            let url = format!("http://localhost:{}", info.local_port);
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .map_err(|e| CommandError::Io {
                    message: format!("probe client: {e}"),
                })?;

            let (responding, http_status) = match client.get(&url).send().await {
                Ok(resp) => (true, Some(resp.status().as_u16())),
                Err(_) => (false, None),
            };

            Ok(RemotePreviewStatus {
                tunnel_active: true,
                local_port: Some(info.local_port),
                remote_port: Some(info.remote_port),
                server_responding: responding,
                http_status,
                error: if responding {
                    None
                } else {
                    Some("Tunnel active but remote dev server not responding".into())
                },
            })
        }
        None => Ok(RemotePreviewStatus {
            tunnel_active: false,
            local_port: None,
            remote_port: None,
            server_responding: false,
            http_status: None,
            error: Some("No active tunnel for this server/port".into()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_key_is_unique_per_server_and_port() {
        assert_ne!(tunnel_key("server-1", 3000), tunnel_key("server-1", 3001));
        assert_ne!(tunnel_key("server-1", 3000), tunnel_key("server-2", 3000));
    }

    #[test]
    fn remote_preview_status_serializes_camel_case() {
        let status = RemotePreviewStatus {
            tunnel_active: true,
            local_port: Some(13000),
            remote_port: Some(3000),
            server_responding: true,
            http_status: Some(200),
            error: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"tunnelActive\":true"));
        assert!(json.contains("\"serverResponding\":true"));
        assert!(json.contains("\"httpStatus\":200"));
    }
}
