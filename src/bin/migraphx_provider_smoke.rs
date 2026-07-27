use ort::{
    ep::{ExecutionProvider, MIGraphX},
    session::Session,
};

fn main() -> ort::Result<()> {
    let provider = MIGraphX::default();
    assert!(provider.is_available()?);

    // Registering the provider proves that the Nix-provided runtime exposes
    // MIGraphX without requiring a model or physical GPU inference.
    Session::builder()?.with_execution_providers([provider.build()])?;
    Ok(())
}
