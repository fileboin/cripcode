pub mod config;
pub mod migrate;
pub mod model;
#[cfg(feature = "template-postgres")]
pub mod postgres;
pub mod repository;
pub mod s3;
pub mod server;
pub mod storage;

pub use server::serve_from_env;
