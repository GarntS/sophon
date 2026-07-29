mod build_support;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use build_support::{Backend, FEATURES};

fn emit_rebuild_triggers(source: &Path) {
    println!("cargo:rerun-if-changed={}", source.display());
    for feature in FEATURES {
        println!(
            "cargo:rerun-if-env-changed=CARGO_FEATURE_{}",
            feature.to_ascii_uppercase()
        );
    }
    for variable in [
        "CC",
        "CXX",
        "CFLAGS",
        "CXXFLAGS",
        "CMAKE_GENERATOR",
        "CMAKE_TOOLCHAIN_FILE",
        "PKG_CONFIG_PATH",
        "CUDA_HOME",
        "CUDA_PATH",
        "ONEAPI_ROOT",
        "VULKAN_SDK",
        "LIBCLANG_PATH",
        "TARGET",
        "HOST",
        "PROFILE",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
}

fn configure_native(source: &Path, out_dir: &Path, backend: Backend) -> PathBuf {
    let native_out = out_dir.join("qwentts-native");
    let mut config = cmake::Config::new(source);
    config
        .out_dir(&native_out)
        .build_target("qwen")
        .define("QWEN_SHARED", "ON")
        .define("BUILD_SHARED_LIBS", "ON")
        .define("GGML_BLAS", "ON")
        .define("GGML_BLAS_VENDOR", "OpenBLAS")
        .define("GGML_CUDA", "OFF")
        .define("GGML_SYCL", "OFF")
        .define("GGML_VULKAN", "OFF");

    if let Some(option) = backend.cmake_option() {
        config.define(option, "ON");
    }
    if backend == Backend::Sycl {
        config.define("GGML_SYCL_DNN", "OFF");
    }

    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| config.build())).unwrap_or_else(
        |_| {
            panic!(
                "qwentts-cpp failed to configure or compile the selected `{backend}` backend. Verify that {} are installed and visible to CMake.",
                backend.prerequisite()
            )
        },
    );

    native_out.join("build")
}

fn is_shared_library(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with("libqwen")
        || name.starts_with("libggml")
        || name.eq_ignore_ascii_case("qwen.dll")
        || name.to_ascii_lowercase().starts_with("ggml") && name.ends_with(".dll")
}

fn collect_shared_libraries(directory: &Path, libraries: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_shared_libraries(&path, libraries);
        } else if is_shared_library(&path) {
            libraries.push(path);
        }
    }
}

fn link_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if let Some(name) = name.strip_prefix("lib") {
        return name
            .strip_suffix(".so")
            .or_else(|| name.strip_suffix(".dylib"))
            .map(str::to_owned);
    }
    name.strip_suffix(".dll").map(str::to_owned)
}

fn stage_and_link_libraries(build_dir: &Path, out_dir: &Path) -> PathBuf {
    let lib_dir = out_dir.join("lib");
    fs::create_dir_all(&lib_dir).expect("failed to create qwentts native library directory");

    let mut libraries = Vec::new();
    collect_shared_libraries(build_dir, &mut libraries);
    libraries.sort();
    libraries.dedup_by(|left, right| left.file_name() == right.file_name());
    if !libraries.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name == "libqwen.so" || name == "libqwen.dylib" || name == "qwen.dll"
            })
    }) {
        panic!(
            "qwentts.cpp built the `qwen` target but no shared qwen library was found under {}",
            build_dir.display()
        );
    }

    for source in &libraries {
        let destination = lib_dir.join(source.file_name().expect("library has a file name"));
        fs::copy(source, destination).unwrap_or_else(|error| {
            panic!(
                "failed to stage native library {}: {error}",
                source.display()
            )
        });
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    let mut link_names = libraries
        .iter()
        .filter_map(|path| link_name(path))
        .collect::<Vec<_>>();
    link_names.sort_by_key(|name| (name != "qwen", name.clone()));
    link_names.dedup();
    for library in link_names {
        println!("cargo:rustc-link-lib=dylib={library}");
    }

    if cfg!(any(target_os = "linux", target_os = "macos")) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
    }

    lib_dir
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"));
    let source = manifest_dir.join("../../third_party/qwentts.cpp");
    let header = source.join("src/qwen.h");
    if !header.is_file() || !source.join("CMakeLists.txt").is_file() {
        panic!(
            "vendored qwentts.cpp source is incomplete at {}; initialize repository submodules before building qwentts-cpp",
            source.display()
        );
    }
    emit_rebuild_triggers(&source);

    let backend = build_support::selected_from_env().unwrap_or_else(|error| panic!("{error}"));
    build_support::validate_prerequisites(backend);

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let build_dir = configure_native(&source, &out_dir, backend);
    let lib_dir = stage_and_link_libraries(&build_dir, &out_dir);

    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .clang_arg(format!("-I{}", source.join("src").display()))
        .allowlist_function("qt_.*")
        .allowlist_type("qt_.*")
        .allowlist_var("QT_.*")
        .generate_comments(false)
        .generate()
        .unwrap_or_else(|error| {
            panic!(
                "failed to generate private bindings from vendored qwen.h for `{backend}`: {error}; install libclang and set LIBCLANG_PATH if needed"
            )
        });
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write qwen.h bindings");

    println!("cargo:include={}", source.join("src").display());
    println!("cargo:libdir={}", lib_dir.display());
    println!("cargo:backend={backend}");
}
