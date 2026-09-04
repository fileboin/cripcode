//! Host-key fingerprint confirmation (explicit TOFU gate).
//!
//! Cripcode shells out to the OpenSSH CLI, which auto-accepts first
//! connections via `StrictHostKeyChecking=accept-new`. These commands let the
//! frontend display the host's fingerprint BEFORE the first connection and
//! record explicit user confirmation — and block cleanly on changed keys.
//!
//! All detection is out-of-band: `ssh-keyscan` retrieves the host's public
//! keys, `ssh-keygen -F` looks the host up in the user's `known_hosts`, and
//! `ssh-keygen -lf` renders a displayable fingerprint. If `ssh-keyscan` is
//! missing or the host is unreachable, the probe reports `probeUnavailable`
//! and the frontend falls back to the previous silent-TOFU behavior, so the
//! existing UX is never broken by a missing tool.

use super::config;
use crate::errors::CommandError;
use serde::Serialize;
use std::path::PathBuf;
use uuid::Uuid;

/// Timeout for the network host-key probe (`ssh-keyscan`).
const HOST_KEY_PROBE_TIMEOUT_SECS: u64 = 15;
/// Timeout for local tool invocations (`ssh-keygen`).
const HOST_KEY_TOOL_TIMEOUT_SECS: u64 = 10;

/// Host-key verification state for a server, from the user's perspective.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyStatus {
    /// `known` | `unknown` | `changed` | `probe-unavailable`
    pub state: String,
    /// Displayable fingerprint of the probed host key (`SHA256:…`), if any.
    pub fingerprint: Option<String>,
    /// SSH key type of the probed key (`ed25519`, `ecdsa`, …), if any.
    pub key_type: Option<String>,
}

/// Look up a server by ID from the config file.
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

/// argv for the network probe. Port 22 is the default and must be omitted so
/// the scan output uses the plain `host` prefix (matching `known_hosts`).
fn build_keyscan_args(host: &str, port: Option<u16>) -> Vec<String> {
    let mut args = vec!["-t".to_string(), "rsa,ecdsa,ed25519".to_string()];
    if let Some(p) = port {
        if p != 22 {
            args.push("-p".into());
            args.push(p.to_string());
        }
    }
    args.push(host.into());
    args
}

/// `known_hosts` lookup reference: entries for non-default ports are stored
/// as `[host]:port`.
fn known_hosts_hostref(host: &str, port: Option<u16>) -> String {
    match port {
        Some(p) if p != 22 => format!("[{host}]:{p}"),
        _ => host.to_string(),
    }
}

fn build_known_hosts_lookup_args(hostref: &str) -> Vec<String> {
    vec!["-F".into(), hostref.into()]
}

fn build_fingerprint_args(key_file: &std::path::Path) -> Vec<String> {
    vec!["-lf".into(), key_file.to_string_lossy().to_string()]
}

/// Parse one `ssh-keyscan` output line into `(hostref, key_type, blob)`.
/// Comment and blank lines yield None.
fn parse_scan_line(line: &str) -> Option<(String, String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut tokens = line.split_whitespace();
    let host = tokens.next()?.to_string();
    let key_type = tokens.next()?.to_string();
    let blob = tokens.next()?.to_string();
    Some((host, key_type, blob))
}

/// Extract a displayable fingerprint from `ssh-keygen -lf` output.
/// Line format: `<bits> <fingerprint> <comment> (keytype)`, e.g.
/// `256 SHA256:AbC… example.com (ED25519)`.
fn parse_keygen_output(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.split_whitespace()
            .nth(1)
            .filter(|f| f.starts_with("SHA256:") || f.starts_with("MD5:"))
            .map(String::from)
    })
}

/// True when any known_hosts line printed by `ssh-keygen -F` carries one of
/// the probed key blobs. `-F` prints `# Host … found` comments plus the
/// matched entries; entries may start with a marker like `@cert-authority`.
fn known_hosts_match(known_output: &str, scan_blobs: &[String]) -> bool {
    known_output.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let mut tokens = line.split_whitespace();
        let first = match tokens.next() {
            Some(t) => t,
            None => return false,
        };
        // Line shape: `[marker] host key_type blob`. Skip the marker's host,
        // then the key type; the blob follows.
        if first.starts_with('@') {
            tokens.next();
        }
        let _key_type = tokens.next();
        match tokens.next() {
            Some(blob) => scan_blobs.iter().any(|b| b == blob),
            None => false,
        }
    })
}

/// Decide the verification state from the probed keys and the user's
/// known_hosts lookup output. Pure — no I/O — so it's directly testable.
fn evaluate_host_key_state(
    scan_keys: &[(String, String, String)],
    known_output: &str,
) -> (&'static str, Option<String>) {
    if scan_keys.is_empty() {
        return ("probe-unavailable", None);
    }
    let blobs: Vec<String> = scan_keys.iter().map(|(_, _, blob)| blob.clone()).collect();
    if known_hosts_match(known_output, &blobs) {
        return ("known", None);
    }
    if !known_output.trim().is_empty() {
        // The host IS in known_hosts but none of its keys match.
        let key_type = scan_keys.first().map(|(_, t, _)| t.clone());
        return ("changed", key_type);
    }
    let key_type = scan_keys.first().map(|(_, t, _)| t.clone());
    ("unknown", key_type)
}

/// Probe the host's public keys and compare them against the user's
/// `known_hosts` WITHOUT connecting. Purely out-of-band, so calling it never
/// mutates trust state.
#[tauri::command]
#[tracing::instrument]
pub async fn check_remote_host_key(server_id: String) -> Result<HostKeyStatus, CommandError> {
    let server = get_server(&server_id)?;

    let mut scan_cmd = tokio::process::Command::new("ssh-keyscan");
    scan_cmd.args(&build_keyscan_args(&server.host, server.port));
    let scan_output = match crate::external_command::run_with_timeout(
        scan_cmd,
        &format!("ssh-keyscan {}", server.name),
        HOST_KEY_PROBE_TIMEOUT_SECS,
    )
    .await
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
        // Missing tool / unreachable host → frontend falls back to silent TOFU.
        Err(_) => {
            return Ok(HostKeyStatus {
                state: "probe-unavailable".into(),
                fingerprint: None,
                key_type: None,
            });
        }
    };

    let scan_keys: Vec<(String, String, String)> =
        scan_output.lines().filter_map(parse_scan_line).collect();
    if scan_keys.is_empty() {
        return Ok(HostKeyStatus {
            state: "probe-unavailable".into(),
            fingerprint: None,
            key_type: None,
        });
    }

    let hostref = known_hosts_hostref(&server.host, server.port);
    let mut known_cmd = tokio::process::Command::new("ssh-keygen");
    known_cmd.args(&build_known_hosts_lookup_args(&hostref));
    // `ssh-keygen -F` exits 1 when the host is not found — that's "unknown",
    // not an error. Any other probe failure also degrades to "not found":
    // the authoritative rejection still happens at connect time.
    let known_output = match crate::external_command::run_with_timeout(
        known_cmd,
        &format!("ssh-keygen -F {}", server.name),
        HOST_KEY_TOOL_TIMEOUT_SECS,
    )
    .await
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
        Err(_) => String::new(),
    };

    // Render a displayable fingerprint for the probed keys.
    let temp_path: PathBuf =
        std::env::temp_dir().join(format!("cripcode-hostkey-{}.keys", Uuid::new_v4()));
    std::fs::write(&temp_path, &scan_output).map_err(|e| CommandError::Io {
        message: format!("Failed to buffer host key probe: {e}"),
    })?;
    let fingerprint = {
        let mut fingerprint_cmd = tokio::process::Command::new("ssh-keygen");
        fingerprint_cmd.args(&build_fingerprint_args(&temp_path));
        match crate::external_command::run_with_timeout(
            fingerprint_cmd,
            &format!("ssh-keygen -lf {}", server.name),
            HOST_KEY_TOOL_TIMEOUT_SECS,
        )
        .await
        {
            Ok(output) => parse_keygen_output(&String::from_utf8_lossy(&output.stdout)),
            Err(_) => None,
        }
    };
    let _ = std::fs::remove_file(&temp_path);

    let (state, key_type) = evaluate_host_key_state(&scan_keys, &known_output);
    Ok(HostKeyStatus {
        state: state.into(),
        fingerprint,
        key_type,
    })
}

/// Record the user's explicit trust decision by appending the probed host
/// keys to the user's `known_hosts` — exactly what OpenSSH would have done
/// silently on first connect, just gated behind a confirmation.
#[tauri::command]
#[tracing::instrument]
pub async fn accept_remote_host_key(server_id: String) -> Result<(), CommandError> {
    let server = get_server(&server_id)?;

    let mut scan_cmd = tokio::process::Command::new("ssh-keyscan");
    scan_cmd.args(&build_keyscan_args(&server.host, server.port));
    let scan_output = crate::external_command::run_with_timeout(
        scan_cmd,
        &format!("ssh-keyscan {}", server.name),
        HOST_KEY_PROBE_TIMEOUT_SECS,
    )
    .await?;
    let scan_text = String::from_utf8_lossy(&scan_output.stdout).to_string();
    if scan_text.trim().is_empty() {
        return Err(CommandError::expected(format!(
            "Could not retrieve the host key for {} — host unreachable?",
            server.name
        )));
    }

    let home = dirs::home_dir().ok_or_else(|| CommandError::Validation {
        field: "known_hosts".into(),
        reason: "Could not resolve the home directory".into(),
    })?;
    let ssh_dir = home.join(".ssh");
    std::fs::create_dir_all(&ssh_dir).map_err(|e| CommandError::Io {
        message: format!("Failed to create ~/.ssh directory: {e}"),
    })?;
    let known_hosts_path = ssh_dir.join("known_hosts");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&known_hosts_path)
        .map_err(|e| CommandError::Io {
            message: format!("Failed to open known_hosts: {e}"),
        })?;
    use std::io::Write;
    let text = if scan_text.ends_with('\n') {
        scan_text
    } else {
        format!("{scan_text}\n")
    };
    file.write_all(text.as_bytes())
        .map_err(|e| CommandError::Io {
            message: format!("Failed to update known_hosts: {e}"),
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyscan_args_omit_default_port() {
        let args = build_keyscan_args("example.com", Some(22));
        assert_eq!(args, vec!["-t", "rsa,ecdsa,ed25519", "example.com"]);
    }

    #[test]
    fn keyscan_args_include_non_default_port() {
        let args = build_keyscan_args("example.com", Some(2222));
        assert_eq!(
            args,
            vec!["-t", "rsa,ecdsa,ed25519", "-p", "2222", "example.com"]
        );
    }

    #[test]
    fn known_hosts_hostref_brackets_non_default_port() {
        assert_eq!(known_hosts_hostref("example.com", Some(22)), "example.com");
        assert_eq!(known_hosts_hostref("example.com", None), "example.com");
        assert_eq!(
            known_hosts_hostref("example.com", Some(2222)),
            "[example.com]:2222"
        );
    }

    #[test]
    fn known_hosts_lookup_args_use_f_flag() {
        assert_eq!(
            build_known_hosts_lookup_args("example.com"),
            vec!["-F", "example.com"]
        );
    }

    #[test]
    fn fingerprint_args_use_lf_flag() {
        let args = build_fingerprint_args(std::path::Path::new("/tmp/keys"));
        assert_eq!(args, vec!["-lf", "/tmp/keys"]);
    }

    #[test]
    fn parse_scan_line_handles_entries_comments_and_garbage() {
        let (host, key_type, blob) =
            parse_scan_line("example.com ssh-ed25519 AAAAB3NzaC1").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(key_type, "ssh-ed25519");
        assert_eq!(blob, "AAAAB3NzaC1");

        let (host, _, _) = parse_scan_line("[example.com]:2222 ssh-ed25519 AAAAB3NzaC1").unwrap();
        assert_eq!(host, "[example.com]:2222");

        assert!(parse_scan_line("# comment").is_none());
        assert!(parse_scan_line("").is_none());
        assert!(parse_scan_line("only-one-token").is_none());
    }

    #[test]
    fn parse_keygen_output_extracts_sha256_fingerprint() {
        let output = "256 SHA256:AbCdEf123 example.com (ED25519)\n";
        assert_eq!(
            parse_keygen_output(output).as_deref(),
            Some("SHA256:AbCdEf123")
        );
    }

    #[test]
    fn parse_keygen_output_accepts_md5_fingerprints() {
        let output = "3072 MD5:aa:bb:cc example.com (RSA)\n";
        assert_eq!(parse_keygen_output(output).as_deref(), Some("MD5:aa:bb:cc"));
    }

    #[test]
    fn parse_keygen_output_returns_none_for_unexpected_output() {
        assert_eq!(parse_keygen_output("total garbage"), None);
        assert_eq!(parse_keygen_output(""), None);
    }

    #[test]
    fn known_hosts_match_finds_matching_blob() {
        let scan = vec!["AAAAB3NzaC1".to_string()];
        let known = "# Host example.com found: line 1\nexample.com ssh-ed25519 AAAAB3NzaC1\n";
        assert!(known_hosts_match(known, &scan));
    }

    #[test]
    fn known_hosts_match_handles_marker_entries() {
        let scan = vec!["AAAAB3NzaC1".to_string()];
        let known = "@cert-authority example.com ssh-ed25519 AAAAB3NzaC1\n";
        assert!(known_hosts_match(known, &scan));
    }

    #[test]
    fn known_hosts_match_rejects_different_blob() {
        let scan = vec!["AAAAB3NzaNEW".to_string()];
        let known = "example.com ssh-ed25519 AAAAB3NzaC1\n";
        assert!(!known_hosts_match(known, &scan));
    }

    #[test]
    fn known_hosts_match_ignores_comments_and_blanks() {
        let scan = vec!["AAAAB3NzaC1".to_string()];
        assert!(!known_hosts_match(
            "# Host example.com found: line 5\n",
            &scan
        ));
        assert!(!known_hosts_match("", &scan));
    }

    fn scan_entry(host: &str, key_type: &str, blob: &str) -> (String, String, String) {
        (host.to_string(), key_type.to_string(), blob.to_string())
    }

    #[test]
    fn evaluate_reports_probe_unavailable_without_keys() {
        let (state, key_type) = evaluate_host_key_state(&[], "example.com ssh-ed25519 AAAA");
        assert_eq!(state, "probe-unavailable");
        assert_eq!(key_type, None);
    }

    #[test]
    fn evaluate_reports_known_when_blob_matches() {
        let scan = vec![scan_entry("example.com", "ssh-ed25519", "AAAAB3NzaC1")];
        let known = "example.com ssh-ed25519 AAAAB3NzaC1\n";
        let (state, key_type) = evaluate_host_key_state(&scan, known);
        assert_eq!(state, "known");
        assert_eq!(key_type, None);
    }

    #[test]
    fn evaluate_reports_changed_when_host_known_with_different_key() {
        let scan = vec![scan_entry("example.com", "ssh-ed25519", "AAAANEW")];
        let known = "example.com ssh-ed25519 AAAAB3NzaC1\n";
        let (state, key_type) = evaluate_host_key_state(&scan, known);
        assert_eq!(state, "changed");
        assert_eq!(key_type.as_deref(), Some("ssh-ed25519"));
    }

    #[test]
    fn evaluate_reports_unknown_when_host_not_in_known_hosts() {
        let scan = vec![scan_entry("example.com", "ssh-ed25519", "AAAAB3NzaC1")];
        let (state, key_type) = evaluate_host_key_state(&scan, "");
        assert_eq!(state, "unknown");
        assert_eq!(key_type.as_deref(), Some("ssh-ed25519"));
    }

    #[test]
    fn host_key_status_serializes_camel_case() {
        let status = HostKeyStatus {
            state: "unknown".into(),
            fingerprint: Some("SHA256:AbC".into()),
            key_type: Some("ed25519".into()),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"keyType\":\"ed25519\""));
        assert!(json.contains("\"fingerprint\":\"SHA256:AbC\""));
    }
}
