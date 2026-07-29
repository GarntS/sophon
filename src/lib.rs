//! Sophon's transport-independent speech transcription service.
//!
//! The module boundaries intentionally keep D-Bus, model acquisition, inference,
//! and transcript handling independently evolvable.

#[cfg(not(any(feature = "qwen-cpu", feature = "qwen-cuda", feature = "qwen-vulkan")))]
compile_error!(
    "select exactly one Qwen backend feature: `qwen-cpu`, `qwen-cuda`, or `qwen-vulkan`"
);
#[cfg(any(
    all(feature = "qwen-cpu", feature = "qwen-cuda"),
    all(feature = "qwen-cpu", feature = "qwen-vulkan"),
    all(feature = "qwen-cuda", feature = "qwen-vulkan"),
))]
compile_error!(
    "Qwen backend features are mutually exclusive; select exactly one of `qwen-cpu`, `qwen-cuda`, or `qwen-vulkan`"
);

pub mod acquisition;
pub mod audio;
pub mod backend;
pub mod config;
pub mod daemon;
pub mod dbus;
pub mod domain;
pub mod playback;
pub mod postprocess;
pub mod service;
pub mod transport;
pub mod tts;
pub mod worker;
