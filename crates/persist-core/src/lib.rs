//! # Persist core
//!
//! Persist backs up selected local directories to a cloud repository using
//! incremental, content-addressable storage. This crate contains everything
//! that is shared between the command-line interface and the desktop
//! application:
//!
//! * the [backup engine](engine),
//! * the [repository format](repo) and its [data model](model),
//! * the [storage abstraction](storage) (Google Drive, local, in-memory),
//! * the [authentication manager](auth),
//! * the local [SQLite state](db) and [configuration](config).
//!
//! The public entry point most callers want is [`session::Session`], which wires
//! all of the above together.

pub mod auth;
pub mod config;
pub mod db;
pub mod drive;
pub mod engine;
pub mod error;
pub mod exclude;
pub mod hashing;
pub mod model;
pub mod repo;
pub mod scanner;
pub mod session;
pub mod storage;
pub mod util;

pub use error::{Error, Result};

/// The version of Persist, from the crate metadata.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
