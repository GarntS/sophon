use ort::{
    ep::{CPU, ExecutionProvider},
    session::Session,
};

fn main() -> ort::Result<()> {
    let provider = CPU::default();
    assert!(provider.is_available()?);

    // Constructing a session builder causes ORT to load the Nix-provided shared
    // library.  Explicitly registering CPU proves the provider is usable.
    Session::builder()?.with_execution_providers([provider.build()])?;
    Ok(())
}
