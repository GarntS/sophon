//! D-Bus request decoding, response mapping, and service-object transport.

use std::collections::BTreeMap;

use crate::domain::{SophonError, TranscriptionOptions};

pub const BUS_NAME: &str = "com.garntresearch.sophon";
pub const OBJECT_PATH: &str = "/com/garntresearch/sophon";
pub const INTERFACE: &str = "com.garntresearch.sophon";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionValue {
    String(String),
    Bool(bool),
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

pub fn dbus_error(error: &SophonError) -> (&'static str, String) {
    (error.public_kind().dbus_name(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PublicErrorKind;
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
    }
}
