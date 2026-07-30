//! Speech-to-text types and functionality.

pub mod backend;
pub mod service;
pub mod types;
pub mod worker;

pub use service::STTService;
pub use types::TranscriptionOptions;
pub use worker::STTWorker;
