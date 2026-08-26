//! # SSH Server Commands
//!
//! SSH server configuration CRUD and connection state management.
//! Server configs are persisted to `~/ShipStudio/.shipstudio/ssh-servers.json`;
//! connection state is in-memory only.
//!
//! See `docs/ssh-architecture.md` for the full architecture.

mod ai_provider;
mod config;
mod connection;
mod files;
mod ollama;
mod remote_agent;
mod remote_dev_server;
mod remote_git;
mod remote_preview;
mod remote_projects;

pub use ai_provider::*;
pub use config::*;
pub use connection::*;
pub use files::*;
pub use ollama::*;
pub use remote_agent::*;
pub use remote_dev_server::*;
pub use remote_git::*;
pub use remote_preview::*;
pub use remote_projects::*;
