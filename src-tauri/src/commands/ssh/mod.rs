//! # SSH Server Commands
//!
//! SSH server configuration CRUD and connection state management.
//! Server configs are persisted to `~/ShipStudio/.shipstudio/ssh-servers.json`;
//! connection state is in-memory only.
//!
//! See `docs/ssh-architecture.md` for the full architecture.

mod config;
mod connection;
mod files;
mod remote_git;
mod remote_projects;

pub use config::*;
pub use connection::*;
pub use files::*;
pub use remote_git::*;
pub use remote_projects::*;
