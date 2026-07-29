use std::{env, fmt, process::Command};

pub const FEATURES: [&str; 4] = ["cpu", "cuda", "sycl", "vulkan"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    Cpu,
    Cuda,
    Sycl,
    Vulkan,
}

impl Backend {
    pub const fn feature(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::Sycl => "sycl",
            Self::Vulkan => "vulkan",
        }
    }

    pub const fn cmake_option(self) -> Option<&'static str> {
        match self {
            Self::Cpu => None,
            Self::Cuda => Some("GGML_CUDA"),
            Self::Sycl => Some("GGML_SYCL"),
            Self::Vulkan => Some("GGML_VULKAN"),
        }
    }

    pub const fn prerequisite(self) -> &'static str {
        match self {
            Self::Cpu => "CMake, a C/C++ compiler, pkg-config, and OpenBLAS",
            Self::Cuda => {
                "the CUDA toolkit (including nvcc), CMake, a C/C++ compiler, pkg-config, and OpenBLAS"
            }
            Self::Sycl => {
                "an Intel-compatible SYCL compiler with -fsycl support, CMake, pkg-config, and OpenBLAS"
            }
            Self::Vulkan => {
                "the Vulkan SDK (headers, loader, and glslc or glslangValidator), CMake, a C/C++ compiler, pkg-config, and OpenBLAS"
            }
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.feature())
    }
}

pub fn select_backend<I, S>(enabled: I) -> Result<Backend, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let enabled = enabled
        .into_iter()
        .filter_map(|feature| match feature.as_ref() {
            "cpu" => Some(Backend::Cpu),
            "cuda" => Some(Backend::Cuda),
            "sycl" => Some(Backend::Sycl),
            "vulkan" => Some(Backend::Vulkan),
            _ => None,
        })
        .collect::<Vec<_>>();

    match enabled.as_slice() {
        [backend] => Ok(*backend),
        [] => Err("qwentts-cpp requires exactly one acceleration feature; enable one of `cpu`, `cuda`, `sycl`, or `vulkan` (the default is `cpu`)".into()),
        _ => Err(format!(
            "qwentts-cpp requires exactly one acceleration feature, but these were enabled together: {}; disable all but one of `cpu`, `cuda`, `sycl`, or `vulkan`",
            enabled
                .iter()
                .map(|backend| format!("`{backend}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn selected_from_env() -> Result<Backend, String> {
    select_backend(FEATURES.into_iter().filter(|feature| {
        env::var_os(format!("CARGO_FEATURE_{}", feature.to_ascii_uppercase())).is_some()
    }))
}

fn command_available(program: &str, arguments: &[&str]) -> bool {
    Command::new(program)
        .args(arguments)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn require_command(backend: Backend, program: &str, arguments: &[&str], description: &str) {
    if !command_available(program, arguments) {
        panic!(
            "qwentts-cpp `{backend}` backend prerequisite is unavailable: {description}. Install {} and ensure the required tools are on PATH/PKG_CONFIG_PATH.",
            backend.prerequisite()
        );
    }
}

pub fn validate_prerequisites(backend: Backend) {
    require_command(backend, "cmake", &["--version"], "CMake was not found");
    require_command(
        backend,
        "pkg-config",
        &["--exists", "openblas"],
        "pkg-config could not find OpenBLAS (`openblas.pc`)",
    );

    match backend {
        Backend::Cpu => {}
        Backend::Cuda => require_command(
            backend,
            "nvcc",
            &["--version"],
            "the CUDA compiler `nvcc` was not found",
        ),
        Backend::Sycl => {
            let compiler = env::var("CXX").unwrap_or_else(|_| "icpx".into());
            require_command(
                backend,
                &compiler,
                &["--version"],
                "an Intel-compatible SYCL C++ compiler was not found (set CXX to it)",
            );
        }
        Backend::Vulkan => {
            require_command(
                backend,
                "pkg-config",
                &["--exists", "vulkan"],
                "pkg-config could not find the Vulkan loader SDK (`vulkan.pc`)",
            );
            if !command_available("glslc", &["--version"])
                && !command_available("glslangValidator", &["--version"])
            {
                panic!(
                    "qwentts-cpp `vulkan` backend prerequisite is unavailable: neither `glslc` nor `glslangValidator` was found. Install {} and ensure the shader compiler is on PATH.",
                    backend.prerequisite()
                );
            }
        }
    }
}
