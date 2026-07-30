//! Model-specific inference backend construction and accelerator selection.

use std::path::Path;

use transcribe_rs::{
    SpeechModel,
    accel::{OrtAccelerator, set_ort_accelerator},
    onnx::{Quantization as OrtQuantization, canary::CanaryModel, parakeet::ParakeetModel},
};

use crate::{
    config::{Accelerator, Engine, Quantization},
    error::SophonError,
    stt::TranscriptionOptions,
};

fn quantization(value: Quantization) -> OrtQuantization {
    match value {
        Quantization::Int8 => OrtQuantization::Int8,
        Quantization::Fp16 => OrtQuantization::FP16,
        Quantization::Fp32 => OrtQuantization::FP32,
    }
}

/// Sets the process-global ORT provider before any model session is created.
/// Explicit unavailable providers are errors; `auto` deliberately permits CPU.
pub fn configure_accelerator(value: Accelerator) -> Result<(), SophonError> {
    let requested = match value {
        Accelerator::Auto => {
            set_ort_accelerator(OrtAccelerator::Auto);
            return Ok(());
        }
        Accelerator::Cpu => OrtAccelerator::CpuOnly,
        Accelerator::Cuda => OrtAccelerator::Cuda,
        Accelerator::Migraphx => OrtAccelerator::Migraphx,
    };
    if !OrtAccelerator::available().contains(&requested) {
        return Err(SophonError::ModelUnavailable(format!(
            "requested accelerator {value:?} is not compiled into this package"
        )));
    }
    set_ort_accelerator(requested);
    Ok(())
}

pub fn to_transcribe_options(
    options: &TranscriptionOptions,
    defaults: &TranscriptionOptions,
    supported_languages: &[String],
) -> Result<transcribe_rs::TranscribeOptions, SophonError> {
    let language = options
        .language
        .clone()
        .or_else(|| defaults.language.clone());
    if let Some(language) = &language
        && !supported_languages.is_empty()
        && !supported_languages
            .iter()
            .any(|supported| supported == language)
    {
        return Err(SophonError::InvalidOptions(format!(
            "language `{language}` is unsupported by the active model"
        )));
    }
    Ok(transcribe_rs::TranscribeOptions {
        language,
        translate: false,
        ..Default::default()
    })
}

trait BackendFactory {
    fn load_parakeet(
        &self,
        model_dir: &Path,
        quantization: OrtQuantization,
    ) -> Result<Box<dyn SpeechModel>, SophonError>;
    fn load_canary(
        &self,
        model_dir: &Path,
        quantization: OrtQuantization,
    ) -> Result<Box<dyn SpeechModel>, SophonError>;
}

struct NativeBackendFactory;

impl BackendFactory for NativeBackendFactory {
    fn load_parakeet(
        &self,
        model_dir: &Path,
        quantization: OrtQuantization,
    ) -> Result<Box<dyn SpeechModel>, SophonError> {
        ParakeetModel::load(model_dir, &quantization)
            .map(|model| Box::new(model) as Box<dyn SpeechModel>)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))
    }

    fn load_canary(
        &self,
        model_dir: &Path,
        quantization: OrtQuantization,
    ) -> Result<Box<dyn SpeechModel>, SophonError> {
        CanaryModel::load(model_dir, &quantization)
            .map(|model| Box::new(model) as Box<dyn SpeechModel>)
            .map_err(|error| SophonError::ModelUnavailable(error.to_string()))
    }
}

fn create_model_with(
    factory: &impl BackendFactory,
    engine: Engine,
    model_dir: &Path,
    value: Quantization,
) -> Result<Box<dyn SpeechModel>, SophonError> {
    match engine {
        Engine::Parakeet => factory.load_parakeet(model_dir, quantization(value)),
        Engine::Canary => factory.load_canary(model_dir, quantization(value)),
    }
}

pub fn create_model(
    engine: Engine,
    model_dir: &Path,
    value: Quantization,
) -> Result<Box<dyn SpeechModel>, SophonError> {
    create_model_with(&NativeBackendFactory, engine, model_dir, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingFactory(Mutex<Vec<Engine>>);

    impl BackendFactory for RecordingFactory {
        fn load_parakeet(
            &self,
            _: &Path,
            _: OrtQuantization,
        ) -> Result<Box<dyn SpeechModel>, SophonError> {
            self.0.lock().unwrap().push(Engine::Parakeet);
            Err(SophonError::ModelUnavailable("fixture failure".into()))
        }

        fn load_canary(
            &self,
            _: &Path,
            _: OrtQuantization,
        ) -> Result<Box<dyn SpeechModel>, SophonError> {
            self.0.lock().unwrap().push(Engine::Canary);
            Err(SophonError::ModelUnavailable("fixture failure".into()))
        }
    }

    #[test]
    fn options_apply_defaults_and_reject_unsupported_requests() {
        let defaults = TranscriptionOptions {
            language: Some("en".into()),
        };
        let languages = vec!["en".into(), "de".into()];
        assert_eq!(
            to_transcribe_options(&TranscriptionOptions::default(), &defaults, &languages)
                .unwrap()
                .language
                .as_deref(),
            Some("en")
        );
        assert!(matches!(
            to_transcribe_options(
                &TranscriptionOptions {
                    language: Some("ja".into())
                },
                &defaults,
                &languages
            ),
            Err(SophonError::InvalidOptions(_))
        ));
    }

    #[test]
    fn factory_selects_the_configured_backend_without_a_real_model() {
        let factory = RecordingFactory(Mutex::new(Vec::new()));
        for engine in [Engine::Parakeet, Engine::Canary] {
            assert!(matches!(
                create_model_with(&factory, engine, Path::new("/fixture"), Quantization::Int8),
                Err(SophonError::ModelUnavailable(_))
            ));
        }
        assert_eq!(
            *factory.0.lock().unwrap(),
            vec![Engine::Parakeet, Engine::Canary]
        );
    }

    #[test]
    fn explicit_migraphx_requires_a_migraphx_build() {
        let result = configure_accelerator(Accelerator::Migraphx);
        if cfg!(feature = "migraphx") {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result,
                Err(SophonError::ModelUnavailable(message))
                    if message.contains("Migraphx")
            ));
        }
    }

    #[test]
    #[ignore = "requires SOPHON_PARAKEET_MODEL_DIR with a real model"]
    fn parakeet_model_smoke() {
        let model_dir =
            std::env::var("SOPHON_PARAKEET_MODEL_DIR").expect("set SOPHON_PARAKEET_MODEL_DIR");
        create_model(Engine::Parakeet, Path::new(&model_dir), Quantization::Int8).unwrap();
    }

    #[test]
    #[ignore = "requires SOPHON_CANARY_MODEL_DIR with a real model"]
    fn canary_model_smoke() {
        let model_dir =
            std::env::var("SOPHON_CANARY_MODEL_DIR").expect("set SOPHON_CANARY_MODEL_DIR");
        create_model(Engine::Canary, Path::new(&model_dir), Quantization::Int8).unwrap();
    }
}
