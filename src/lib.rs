//! Sophon's transport-independent speech transcription service.
//!
//! The module boundaries keep D-Bus, the model registry, providers, inference,
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

pub mod audio;
pub mod config;
pub mod dbus;
pub mod error;
pub mod model_registry;
pub mod provider_runtime;
pub mod stt;
pub mod tts;
