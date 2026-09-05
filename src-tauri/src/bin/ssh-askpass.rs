//! `ssh-askpass` — the OpenSSH client's password helper for password-mode
//! CripCode SSH servers.
//!
//! The OpenSSH client spawns this binary when it needs the SSH password and
//! no interactive TTY can answer the prompt (headless exec: connection test,
//! remote files, Ollama status, git, builds). The helper reads the server id
//! from the environment (`CRIPCODE_SSH_SERVER_ID`), fetches the password
//! from the OS keystore, and prints it as a single line on stdout — the
//! channel ssh treats as the prompt answer.
//!
//! Security shape: the password travels keystore → this process's memory →
//! stdout pipe → ssh's stdin. It is never logged, never written to disk,
//! and never placed in argv or environment variables. A missing entry fails
//! closed: non-zero exit, empty stdout, one generic secret-free note on
//! stderr (which may surface in a connection error message).

fn main() {
    let code = ship_studio_lib::commands::ssh::secrets::run_askpass(
        &ship_studio_lib::commands::ssh::secrets::KeyringStore,
    );
    std::process::exit(code);
}
