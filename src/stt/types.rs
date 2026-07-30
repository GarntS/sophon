//! Transport-independent speech-to-text values.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranscriptionOptions {
    pub language: Option<String>,
}
