#[allow(dead_code)]
#[path = "../build_support.rs"]
mod build_support;

use build_support::{Backend, select_backend};

#[test]
fn default_cpu_feature_is_a_valid_single_backend() {
    assert_eq!(select_backend(["cpu"]), Ok(Backend::Cpu));
}

#[test]
fn every_single_backend_feature_is_valid() {
    for (feature, expected) in [
        ("cpu", Backend::Cpu),
        ("cuda", Backend::Cuda),
        ("sycl", Backend::Sycl),
        ("vulkan", Backend::Vulkan),
    ] {
        assert_eq!(select_backend([feature]), Ok(expected));
    }
}

#[test]
fn zero_backends_has_actionable_diagnostic() {
    let error = select_backend(Vec::<&str>::new()).unwrap_err();
    assert!(error.contains("exactly one acceleration feature"));
    assert!(error.contains("default is `cpu`"));
}

#[test]
fn conflicting_backends_are_named_in_diagnostic() {
    let error = select_backend(["cpu", "cuda"]).unwrap_err();
    assert!(error.contains("`cpu`"));
    assert!(error.contains("`cuda`"));
    assert!(error.contains("disable all but one"));
}

#[test]
fn backend_diagnostics_name_the_selected_prerequisites() {
    assert!(Backend::Cuda.prerequisite().contains("nvcc"));
    assert!(Backend::Sycl.prerequisite().contains("SYCL"));
    assert!(Backend::Vulkan.prerequisite().contains("Vulkan SDK"));
}
