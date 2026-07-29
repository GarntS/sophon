//! D-Bus request decoding, response mapping, and service-object transport.

use std::{collections::BTreeMap, os::fd::OwnedFd};

use crate::{
    audio::read_clone_fd,
    config::TtsConfig,
    domain::{SophonError, TranscriptionOptions, TtsCapabilities, TtsRequest, VoiceIntent},
};

pub const BUS_NAME: &str = "com.garntresearch.sophon";
pub const OBJECT_PATH: &str = "/com/garntresearch/sophon";
pub const INTERFACE: &str = "com.garntresearch.sophon";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionValue {
    String(String),
    Bool(bool),
}

#[derive(Debug)]
pub enum TtsOptionValue {
    String(String),
    Double(f64),
    UnixFd(OwnedFd),
}

pub fn decode_options(
    values: BTreeMap<String, OptionValue>,
    defaults: &TranscriptionOptions,
) -> Result<TranscriptionOptions, SophonError> {
    let mut options = defaults.clone();
    for (key, value) in values {
        match (key.as_str(), value) {
            ("language", OptionValue::String(language)) => options.language = Some(language),
            ("translate", OptionValue::Bool(translate)) => options.translate = Some(translate),
            ("language" | "translate", _) => {
                return Err(SophonError::InvalidOptions(format!(
                    "option `{key}` has the wrong type"
                )));
            }
            _ => {
                return Err(SophonError::InvalidOptions(format!(
                    "unknown option `{key}`"
                )));
            }
        }
    }
    Ok(options)
}

fn validate_consumed_tts_text(value: &str, field: &str, max_bytes: u64) -> Result<(), SophonError> {
    if value.len() as u64 > max_bytes {
        return Err(SophonError::ResourceLimit(format!(
            "{field} exceeds configured byte limit"
        )));
    }
    if value.trim().is_empty() {
        return Err(SophonError::InvalidTtsOptions(format!(
            "{field} must not be empty when supplied"
        )));
    }
    if value.chars().any(|character| {
        character == '\0' || (character.is_control() && !character.is_whitespace())
    }) {
        return Err(SophonError::InvalidTtsOptions(format!(
            "{field} contains a disallowed control character"
        )));
    }
    Ok(())
}

pub fn decode_tts_options(
    text: &str,
    values: BTreeMap<String, TtsOptionValue>,
    config: &TtsConfig,
    capabilities: TtsCapabilities,
    available_voices: &[String],
) -> Result<TtsRequest, SophonError> {
    if text.trim().is_empty() {
        return Err(SophonError::InvalidTtsOptions(
            "text must not be empty or whitespace".into(),
        ));
    }
    if text.len() as u64 > config.operational.max_text_bytes {
        return Err(SophonError::ResourceLimit(
            "UTF-8 text exceeds configured byte limit".into(),
        ));
    }

    let mut voice = None;
    let mut language = None;
    let mut speed = config.operational.default_speed;
    let mut clone_audio = None;
    let mut clone_transcript = None;
    let mut voice_description = None;
    for (key, value) in values {
        match (key.as_str(), value) {
            ("voice", TtsOptionValue::String(value)) => voice = Some(value),
            ("language", TtsOptionValue::String(value)) => language = Some(value),
            ("speed", TtsOptionValue::Double(value)) => speed = value,
            ("clone_audio", TtsOptionValue::UnixFd(value)) => clone_audio = Some(value),
            ("clone_transcript", TtsOptionValue::String(value)) => clone_transcript = Some(value),
            ("voice_description", TtsOptionValue::String(value)) => voice_description = Some(value),
            (
                "voice" | "language" | "speed" | "clone_audio" | "clone_transcript"
                | "voice_description",
                _,
            ) => {
                return Err(SophonError::InvalidTtsOptions(format!(
                    "option `{key}` has the wrong type"
                )));
            }
            _ => {
                return Err(SophonError::InvalidTtsOptions(format!(
                    "unknown option `{key}`"
                )));
            }
        }
    }

    if !speed.is_finite() || !(0.5..=2.0).contains(&speed) {
        return Err(SophonError::InvalidTtsOptions(
            "speed must be finite and between 0.5 and 2.0".into(),
        ));
    }
    if !capabilities.speed_control && speed != 1.0 {
        return Err(SophonError::InvalidTtsOptions(
            "the active TTS provider does not support speed control".into(),
        ));
    }
    if language.as_ref().is_some_and(|language| {
        language.trim().is_empty()
            || !language
                .chars()
                .all(|character| character.is_ascii_alphabetic() || character == '-')
    }) {
        return Err(SophonError::InvalidTtsOptions(
            "language must be a non-empty language tag".into(),
        ));
    }
    if voice.as_ref().is_some_and(|voice| voice.trim().is_empty()) {
        return Err(SophonError::InvalidTtsOptions(
            "voice must not be empty".into(),
        ));
    }
    if let Some(transcript) = &clone_transcript {
        validate_consumed_tts_text(
            transcript,
            "clone_transcript",
            config.operational.max_text_bytes,
        )?;
    }
    if let Some(description) = &voice_description {
        validate_consumed_tts_text(
            description,
            "voice_description",
            config.operational.max_text_bytes,
        )?;
    }
    if clone_transcript.is_some() && clone_audio.is_none() {
        return Err(SophonError::InvalidTtsOptions(
            "clone_transcript requires clone_audio".into(),
        ));
    }
    let intent_count = usize::from(voice.is_some())
        + usize::from(clone_audio.is_some())
        + usize::from(voice_description.is_some());
    if intent_count > 1 {
        return Err(SophonError::InvalidTtsOptions(
            "voice, clone_audio, and voice_description are mutually exclusive".into(),
        ));
    }

    let voice = if let Some(voice) = voice {
        if !capabilities.named_voices {
            return Err(SophonError::UnsupportedCapability(
                "named voices are not supported".into(),
            ));
        }
        if !available_voices.iter().any(|available| available == &voice) {
            return Err(SophonError::InvalidTtsOptions(format!(
                "voice `{voice}` is not available"
            )));
        }
        VoiceIntent::Named(voice)
    } else if let Some(fd) = clone_audio {
        if !capabilities.voice_cloning {
            return Err(SophonError::UnsupportedCapability(
                "one-shot voice cloning is not supported".into(),
            ));
        }
        VoiceIntent::Clone {
            reference: read_clone_fd(
                fd,
                config.operational.max_reference_audio_bytes,
                config.operational.max_reference_audio_seconds,
            )?,
            transcript: clone_transcript,
        }
    } else if let Some(description) = voice_description {
        if !capabilities.voice_design {
            return Err(SophonError::UnsupportedCapability(
                "voice design is not supported".into(),
            ));
        }
        VoiceIntent::Design(description)
    } else {
        VoiceIntent::Default
    };

    Ok(TtsRequest {
        text: text.to_owned(),
        language,
        speed,
        voice,
    })
}

pub fn dbus_error(error: &SophonError) -> (&'static str, String) {
    (error.public_kind().dbus_name(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Config, ConfigPaths},
        domain::PublicErrorKind,
    };
    use std::os::fd::OwnedFd;

    fn tts_config() -> TtsConfig {
        let root = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::from_homes(root.path().join("config"), root.path().join("cache"));
        Config::load(&paths).unwrap().tts.unwrap()
    }

    fn capabilities() -> TtsCapabilities {
        TtsCapabilities {
            named_voices: true,
            voice_cloning: false,
            voice_design: false,
            speed_control: true,
        }
    }
    #[test]
    fn options_are_strict() {
        assert!(
            decode_options(
                BTreeMap::from([("bad".into(), OptionValue::Bool(true))]),
                &TranscriptionOptions::default()
            )
            .is_err()
        );
        assert_eq!(
            PublicErrorKind::NotReady.dbus_name(),
            "com.garntresearch.sophon.NotReady"
        );
        for (kind, suffix) in [
            (PublicErrorKind::InvalidTtsOptions, "InvalidTtsOptions"),
            (
                PublicErrorKind::InvalidReferenceAudio,
                "InvalidReferenceAudio",
            ),
            (
                PublicErrorKind::UnsupportedCapability,
                "UnsupportedCapability",
            ),
            (PublicErrorKind::OutputExists, "OutputExists"),
            (PublicErrorKind::OutputFailed, "OutputFailed"),
            (PublicErrorKind::SynthesisFailed, "SynthesisFailed"),
            (PublicErrorKind::PlaybackFailed, "PlaybackFailed"),
        ] {
            assert_eq!(
                kind.dbus_name(),
                format!("com.garntresearch.sophon.{suffix}")
            );
        }
    }

    #[test]
    fn tts_options_apply_defaults_and_decode_named_voice_language_and_speed() {
        let config = tts_config();
        let voices = vec!["af_heart".into(), "am_adam".into()];
        let request = decode_tts_options(
            "hello",
            BTreeMap::from([
                ("voice".into(), TtsOptionValue::String("am_adam".into())),
                ("language".into(), TtsOptionValue::String("en".into())),
                ("speed".into(), TtsOptionValue::Double(1.25)),
            ]),
            &config,
            capabilities(),
            &voices,
        )
        .unwrap();
        assert_eq!(request.speed, 1.25);
        assert_eq!(request.language.as_deref(), Some("en"));
        assert!(matches!(request.voice, VoiceIntent::Named(ref voice) if voice == "am_adam"));

        let defaults =
            decode_tts_options("hello", BTreeMap::new(), &config, capabilities(), &voices).unwrap();
        assert_eq!(defaults.speed, config.operational.default_speed);
        assert!(matches!(defaults.voice, VoiceIntent::Default));
    }

    #[test]
    fn tts_options_reject_types_intent_conflicts_orphans_ranges_and_text_limits() {
        let mut config = tts_config();
        config.operational.max_text_bytes = 4;
        let voices = vec!["af_heart".into()];
        let invalid = [
            BTreeMap::from([("unknown".into(), TtsOptionValue::String("x".into()))]),
            BTreeMap::from([("speed".into(), TtsOptionValue::String("fast".into()))]),
            BTreeMap::from([("speed".into(), TtsOptionValue::Double(f64::NAN))]),
            BTreeMap::from([(
                "clone_transcript".into(),
                TtsOptionValue::String("words".into()),
            )]),
            BTreeMap::from([
                ("voice".into(), TtsOptionValue::String("af_heart".into())),
                (
                    "voice_description".into(),
                    TtsOptionValue::String("warm".into()),
                ),
            ]),
        ];
        for values in invalid {
            assert!(matches!(
                decode_tts_options("test", values, &config, capabilities(), &voices),
                Err(SophonError::InvalidTtsOptions(_) | SophonError::ResourceLimit(_))
            ));
        }
        assert!(matches!(
            decode_tts_options(
                "oversized",
                BTreeMap::new(),
                &config,
                capabilities(),
                &voices
            ),
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(matches!(
            decode_tts_options("  ", BTreeMap::new(), &config, capabilities(), &voices),
            Err(SophonError::InvalidTtsOptions(_))
        ));
    }

    #[test]
    fn text_like_tts_inputs_are_limited_independently_before_native_work() {
        let mut config = tts_config();
        config.operational.max_text_bytes = 4;
        let mut design_capable = capabilities();
        design_capable.voice_design = true;
        let request = decode_tts_options(
            "test",
            BTreeMap::from([(
                "voice_description".into(),
                TtsOptionValue::String("warm".into()),
            )]),
            &config,
            design_capable,
            &[],
        )
        .unwrap();
        assert!(matches!(request.voice, VoiceIntent::Design(description) if description == "warm"));

        for description in ["large", "bad\0"] {
            assert!(matches!(
                decode_tts_options(
                    "test",
                    BTreeMap::from([(
                        "voice_description".into(),
                        TtsOptionValue::String(description.into()),
                    )]),
                    &config,
                    design_capable,
                    &[],
                ),
                Err(SophonError::ResourceLimit(_) | SophonError::InvalidTtsOptions(_))
            ));
        }

        let file = tempfile::tempfile().unwrap();
        let fd: OwnedFd = file.into();
        let mut clone_capable = capabilities();
        clone_capable.voice_cloning = true;
        assert!(matches!(
            decode_tts_options(
                "test",
                BTreeMap::from([
                    ("clone_audio".into(), TtsOptionValue::UnixFd(fd)),
                    (
                        "clone_transcript".into(),
                        TtsOptionValue::String("large".into()),
                    ),
                ]),
                &config,
                clone_capable,
                &[],
            ),
            Err(SophonError::ResourceLimit(_))
        ));
    }

    #[test]
    fn unsupported_speed_is_rejected_during_decode_before_queueing() {
        let config = tts_config();
        let mut unsupported = capabilities();
        unsupported.speed_control = false;
        let mut values = BTreeMap::new();
        values.insert("speed".into(), TtsOptionValue::Double(1.25));
        assert!(matches!(
            decode_tts_options("hello", values, &config, unsupported, &[]),
            Err(SophonError::InvalidTtsOptions(_))
        ));
        assert!(decode_tts_options("hello", BTreeMap::new(), &config, unsupported, &[]).is_ok());
    }

    #[test]
    fn tts_clone_fd_honors_capability_before_strict_reference_decoding() {
        let config = tts_config();
        let file = tempfile::tempfile().unwrap();
        let fd: OwnedFd = file.into();
        assert!(matches!(
            decode_tts_options(
                "clone",
                BTreeMap::from([("clone_audio".into(), TtsOptionValue::UnixFd(fd))]),
                &config,
                capabilities(),
                &[]
            ),
            Err(SophonError::UnsupportedCapability(_))
        ));

        let file = tempfile::tempfile().unwrap();
        let fd: OwnedFd = file.into();
        let capable = TtsCapabilities {
            voice_cloning: true,
            ..capabilities()
        };
        assert!(matches!(
            decode_tts_options(
                "clone",
                BTreeMap::from([("clone_audio".into(), TtsOptionValue::UnixFd(fd))]),
                &config,
                capable,
                &[]
            ),
            Err(SophonError::InvalidReferenceAudio(_))
        ));
    }
}
