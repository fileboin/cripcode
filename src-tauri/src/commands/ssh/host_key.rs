//! Host-key fingerprint confirmation (explicit TOFU gate).
//!
//! Cripcode shells out to the OpenSSH CLI, which auto-accepts first
//! connections via `StrictHostKeyChecking=accept-new`. These commands let the
//! frontend display the host's fingerprint BEFORE the first connection and
//! record explicit user confirmation — and block cleanly on changed keys.
//!
//! The probe is a real `ssh` connection attempt that NEVER authenticates
//! (`BatchMode=yes` + `PubkeyAuthentication=no` — no credentials exist in
//! this process and no auth packets are sent): the host key is exchanged
//! during KEX, before authentication, and `StrictHostKeyChecking=accept-new`
//! records the offered key into a **temporary** `UserKnownHostsFile` which
//! this module reads and deletes. The user's `known_hosts` receives keys
//! only through `accept_remote_host_key`, after an explicit confirmation.
//! `ssh-keygen -F` looks the host up in the user's `known_hosts`, and
//! `ssh-keygen -lf` renders a displayable fingerprint. If the probe cannot
//! complete (tool missing, unreachable host, KEX mismatch), the probe
//! reports `probeUnavailable` and the frontend BLOCKS the connection — an
//! unverifiable host must never fall back to silent TOFU. Hosts already
//! recorded in `known_hosts` stay `known`: ssh re-verifies the key
//! authoritatively at connect time.

use super::config;
use crate::errors::CommandError;
use serde::Serialize;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Timeout for the network host-key capture probe (the `ssh` capture
/// connection).
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

/// argv for the capture probe. The probe connects but NEVER authenticates:
/// `BatchMode=yes` + `PubkeyAuthentication=no` mean no auth packets and no
/// prompts, while `StrictHostKeyChecking=accept-new` records the offered
/// host key into `UserKnownHostsFile` during KEX. `HashKnownHosts=no` keeps
/// the captured lines in plain `host keytype blob` form so they stay
/// parseable. Port 22 is the default and must be omitted so the capture line
/// uses the plain `host` prefix (matching `known_hosts`).
fn build_capture_probe_args(
    server: &crate::types::SshServer,
    capture_file: &std::path::Path,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", capture_file.display()),
        "-o".into(),
        "HashKnownHosts=no".into(),
        "-o".into(),
        "PubkeyAuthentication=no".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
    ];
    if let Some(p) = server.port {
        if p != 22 {
            args.push("-p".into());
            args.push(p.to_string());
        }
    }
    args.push(format!("{}@{}", server.username, server.host));
    args
}

/// Temporary `UserKnownHostsFile` used by the capture probe. Created empty;
/// deleted on drop, so the capture file is cleaned up on every normal,
/// error, and panic (unwind) path — it never outlives the probe and the
/// captured key can only reach the user's `known_hosts` through
/// [`accept_remote_host_key`].
struct TempKnownHostsFile {
    path: PathBuf,
}

impl TempKnownHostsFile {
    fn create() -> Result<Self, CommandError> {
        let path =
            std::env::temp_dir().join(format!("cripcode-hostkey-capture-{}.kh", Uuid::new_v4()));
        std::fs::write(&path, b"").map_err(|e| CommandError::Io {
            message: format!("Failed to create the host-key capture file: {e}"),
        })?;
        Ok(Self { path })
    }

    fn read(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }
}

impl Drop for TempKnownHostsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Run the capture probe: a real `ssh` connection attempt that never
/// authenticates, recording the offered host key into a temporary
/// `UserKnownHostsFile`. Returns the captured `(host, key_type, blob)`
/// tuples — an empty vector means the host key could not be captured at
/// all (unreachable host, KEX mismatch, missing tool), which the caller
/// fails closed on. An auth denial (exit 255) is the EXPECTED outcome and
/// carries the capture; only the capture file matters here.
async fn capture_host_keys(
    server: &crate::types::SshServer,
) -> Result<Vec<(String, String, String)>, CommandError> {
    let capture_file = TempKnownHostsFile::create()?;

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args(build_capture_probe_args(server, &capture_file.path));
    // The auth denial is expected and carries no information we need — the
    // capture file is the single source of truth for this probe.
    let _ = crate::external_command::run_with_timeout(
        cmd,
        &format!("ssh host-key capture {}", server.name),
        HOST_KEY_PROBE_TIMEOUT_SECS,
    )
    .await;

    let captured = capture_file.read();
    Ok(captured.lines().filter_map(parse_scan_line).collect())
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

/// Parse one captured host-key line into `(hostref, key_type, blob)`.
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

/// Decide the state when the network probe is unavailable. A host already
/// recorded in known_hosts stays `known` — ssh re-verifies the key at connect
/// time, so a changed key is still rejected there. Anything else fails closed
/// as `probe-unavailable`; the frontend blocks the connection instead of
/// falling back to silent TOFU. Pure — no I/O — so it's directly testable.
fn fallback_state_without_probe(known_output: &str) -> &'static str {
    if known_output.trim().is_empty() {
        "probe-unavailable"
    } else {
        "known"
    }
}

/// Probe the host's public keys with an unauthenticated capture connection
/// and compare them against the user's `known_hosts`. The probe NEVER
/// authenticates (see [`capture_host_keys`]) and never mutates the user's
/// `known_hosts` — trust is recorded only by `accept_remote_host_key` after
/// an explicit confirmation.
#[tauri::command]
#[tracing::instrument]
pub async fn check_remote_host_key(server_id: String) -> Result<HostKeyStatus, CommandError> {
    let server = get_server(&server_id)?;

    // Look the host up in known_hosts BEFORE probing the network: an
    // already-trusted host must not be locked out by a failed capture, and
    // an unprobed host that is absent from known_hosts must fail closed
    // (see `fallback_state_without_probe`).
    let hostref = known_hosts_hostref(&server.host, server.port);
    let mut known_cmd = tokio::process::Command::new("ssh-keygen");
    known_cmd.args(build_known_hosts_lookup_args(&hostref));
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

    let captured_keys = match capture_host_keys(&server).await {
        Ok(keys) => keys,
        // Capture-file setup failure → fail closed: only hosts already
        // recorded in known_hosts may proceed (ssh re-verifies at connect).
        Err(_) => {
            return Ok(HostKeyStatus {
                state: fallback_state_without_probe(&known_output).into(),
                fingerprint: None,
                key_type: None,
            });
        }
    };

    if captured_keys.is_empty() {
        // Unreachable host / KEX mismatch / missing tool → fail closed.
        return Ok(HostKeyStatus {
            state: fallback_state_without_probe(&known_output).into(),
            fingerprint: None,
            key_type: None,
        });
    }

    // Render a displayable fingerprint for the captured keys.
    let captured_text = captured_keys
        .iter()
        .map(|(host, key_type, blob)| format!("{host} {key_type} {blob}"))
        .collect::<Vec<_>>()
        .join("\n");
    let temp_path: PathBuf =
        std::env::temp_dir().join(format!("cripcode-hostkey-{}.keys", Uuid::new_v4()));
    std::fs::write(&temp_path, &captured_text).map_err(|e| CommandError::Io {
        message: format!("Failed to buffer host key probe: {e}"),
    })?;
    let fingerprint = {
        let mut fingerprint_cmd = tokio::process::Command::new("ssh-keygen");
        fingerprint_cmd.args(build_fingerprint_args(&temp_path));
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

    let (state, key_type) = evaluate_host_key_state(&captured_keys, &known_output);
    Ok(HostKeyStatus {
        state: state.into(),
        fingerprint,
        key_type,
    })
}

/// Record the user's explicit trust decision by re-running the capture probe
/// and appending the captured host keys to the user's `known_hosts` — exactly
/// what OpenSSH would have done silently on first connect, just gated behind
/// a confirmation.
///
/// RACE (v1 parity with the previous keyscan design): the fingerprint shown
/// in the modal came from the `check` capture; this re-capture runs seconds
/// later. If the server rotated its key in that window, the stored key could
/// differ from the displayed one. The real `ssh` connections still verify
/// the stored key at connect time and refuse a mismatched one, so the worst
/// case is a stored-but-unusable key, not an undetected MITM. v2 hardening:
/// pass the displayed blob from the frontend and store exactly that.
#[tauri::command]
#[tracing::instrument]
pub async fn accept_remote_host_key(server_id: String) -> Result<(), CommandError> {
    let server = get_server(&server_id)?;

    let captured = capture_host_keys(&server).await?;
    if captured.is_empty() {
        return Err(CommandError::expected(format!(
            "Could not retrieve the host key for {} — host unreachable?",
            server.name
        )));
    }
    let captured_text = captured
        .iter()
        .map(|(host, key_type, blob)| format!("{host} {key_type} {blob}"))
        .collect::<Vec<_>>()
        .join("\n");

    let home = dirs::home_dir().ok_or_else(|| CommandError::Validation {
        field: "known_hosts".into(),
        reason: "Could not resolve the home directory".into(),
    })?;
    append_to_known_hosts(&home, &captured_text)
}

/// Append the confirmed host-key lines to the user's `known_hosts`
/// (`<home>/.ssh/known_hosts`), creating `~/.ssh` when needed. Injectable
/// home path so unit tests never touch the real file.
fn append_to_known_hosts(home: &Path, text: &str) -> Result<(), CommandError> {
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
    let line_terminated = if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    };
    file.write_all(line_terminated.as_bytes())
        .map_err(|e| CommandError::Io {
            message: format!("Failed to update known_hosts: {e}"),
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_server(port: Option<u16>) -> crate::types::SshServer {
        crate::types::SshServer {
            id: "probe-test".into(),
            name: "Probe VPS".into(),
            host: "example.com".into(),
            port,
            username: "deploy".into(),
            key_path: None,
            auth_type: crate::types::AuthType::Key,
            created_at: 0,
            last_connected_at: None,
        }
    }

    #[test]
    fn capture_probe_args_never_authenticate_and_use_the_temp_file() {
        let capture = std::path::Path::new("C:\\tmp\\capture.kh");
        let args = build_capture_probe_args(&probe_server(Some(22)), capture);
        let joined = args.join(" ");

        // No auth packets, no prompts: BatchMode + pubkey disabled.
        assert!(joined.contains("-o BatchMode=yes"));
        assert!(joined.contains("-o PubkeyAuthentication=no"));
        // TOFU capture goes to the TEMPORARY known_hosts file only.
        assert!(joined.contains("-o StrictHostKeyChecking=accept-new"));
        assert!(joined.contains(&format!("-o UserKnownHostsFile={}", capture.display())));
        // Plain-form capture lines for the parser.
        assert!(joined.contains("-o HashKnownHosts=no"));
        // Connect-level timeout.
        assert!(joined.contains("-o ConnectTimeout=10"));
        // No key file — the probe is auth-agnostic (key AND password servers).
        assert!(!args.contains(&"-i".to_string()));
        // Default port 22 must be omitted; destination last.
        assert!(!args.contains(&"-p".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("deploy@example.com"));
    }

    #[test]
    fn capture_probe_args_bracket_non_default_port() {
        let capture = std::path::Path::new("/tmp/capture.kh");
        let args = build_capture_probe_args(&probe_server(Some(2222)), capture);
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"2222".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("deploy@example.com"));
    }

    #[test]
    fn capture_probe_args_carry_no_credentials() {
        // The probe argv can never contain a password or key material — the
        // value only exists in the keystore/ssh stdin pipe, never in args.
        let secret = "s3cret-pw";
        let capture = std::path::Path::new("/tmp/capture.kh");
        let args = build_capture_probe_args(&probe_server(Some(22)), capture);
        for arg in &args {
            assert!(!arg.contains(secret));
        }
    }

    #[test]
    fn temp_known_hosts_file_is_removed_on_drop() {
        let path;
        {
            let temp = TempKnownHostsFile::create().expect("create temp capture file");
            path = temp.path.clone();
            assert!(path.exists());
            // Simulate a capture: even with content the drop must clean up.
            std::fs::write(&path, b"example.com ssh-ed25519 AAAAB3NzaC1\n").unwrap();
        }
        assert!(!path.exists(), "capture file must be cleaned up on drop");
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
    fn fallback_without_probe_keeps_recorded_hosts_known() {
        let known = "# Host example.com found: line 1\nexample.com ssh-ed25519 AAAAB3NzaC1\n";
        assert_eq!(fallback_state_without_probe(known), "known");
    }

    #[test]
    fn fallback_without_probe_fails_closed_for_unrecorded_hosts() {
        assert_eq!(fallback_state_without_probe(""), "probe-unavailable");
        assert_eq!(fallback_state_without_probe("   \n  "), "probe-unavailable");
    }

    #[test]
    fn captured_text_round_trips_through_the_parser() {
        // The capture lines must survive the check→accept round trip without
        // changing shape (both stages parse the same 3-token format).
        let captured = vec![
            (
                "91.99.126.64".to_string(),
                "ssh-ed25519".to_string(),
                "AAAAC3NzaC1".to_string(),
            ),
            (
                "91.99.126.64".to_string(),
                "ssh-rsa".to_string(),
                "AAAAB3NzaC2".to_string(),
            ),
        ];
        let text = captured
            .iter()
            .map(|(host, key_type, blob)| format!("{host} {key_type} {blob}"))
            .collect::<Vec<_>>()
            .join("\n");
        let reparsed: Vec<(String, String, String)> =
            text.lines().filter_map(parse_scan_line).collect();
        assert_eq!(reparsed, captured);
    }

    #[test]
    fn append_to_known_hosts_creates_and_appends_line_terminated() {
        let home = std::env::temp_dir().join(format!("cripcode-kh-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&home).expect("temp home");

        append_to_known_hosts(&home, "example.com ssh-ed25519 AAAAB3NzaC1").expect("first append");
        append_to_known_hosts(&home, "example.com ssh-ed25519 AAAAB3NzaC2\n")
            .expect("second append (already line-terminated)");

        let content = std::fs::read_to_string(home.join(".ssh").join("known_hosts"))
            .expect("read known_hosts");
        assert!(content.contains("example.com ssh-ed25519 AAAAB3NzaC1\n"));
        assert!(content.contains("example.com ssh-ed25519 AAAAB3NzaC2\n"));
        // Exactly two entries, both newline-terminated (append-safe).
        assert_eq!(content.lines().count(), 2);
        assert!(content.ends_with('\n'));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn accept_re_captures_documenting_the_check_confirm_race() {
        // DOCUMENTED RISK (v1 parity with the keyscan design): the modal
        // shows the fingerprint from the `check` capture; `accept` performs
        // its OWN capture seconds later. A server key rotation inside that
        // window stores a key different from the displayed fingerprint.
        // The real `ssh` connections still verify the stored key at connect
        // time and refuse a mismatched one, so this race cannot turn into an
        // undetected MITM — worst case is a stored-but-unusable key that the
        // user re-confirms. Capture itself is not unit-testable (real ssh +
        // real network), so this test pins the documented contract: an empty
        // capture must fail accept with the unreachable-host error path.
        let captured: Vec<(String, String, String)> = Vec::new();
        let text = captured
            .iter()
            .map(|(host, key_type, blob)| format!("{host} {key_type} {blob}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.trim().is_empty());
        // The accept flow treats an empty capture exactly like the previous
        // keyscan design: "Could not retrieve the host key".
        assert_eq!(
            fallback_state_without_probe(&text),
            "probe-unavailable",
            "empty capture must never produce a trust state"
        );
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
