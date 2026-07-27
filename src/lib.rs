//! Sophon's transport-independent speech transcription service.
//!
//! The module boundaries intentionally keep D-Bus, model acquisition, inference,
//! and transcript handling independently evolvable.

pub mod acquisition;
pub mod audio;
pub mod backend;
pub mod config;
pub mod daemon;
pub mod dbus;
pub mod domain;
pub mod postprocess;
pub mod service;
pub mod transport;
pub mod worker;
