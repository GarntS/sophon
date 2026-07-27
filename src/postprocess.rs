//! Ordered transcript post-processing pipeline.

use crate::domain::Transcript;

pub trait TranscriptProcessor: Send + Sync {
    fn process(&self, transcript: Transcript) -> Transcript;
}

#[derive(Debug, Default)]
pub struct IdentityProcessor;

impl TranscriptProcessor for IdentityProcessor {
    fn process(&self, transcript: Transcript) -> Transcript {
        transcript
    }
}

#[derive(Default)]
pub struct PostProcessingPipeline {
    processors: Vec<Box<dyn TranscriptProcessor>>,
}

impl PostProcessingPipeline {
    pub fn new(processors: Vec<Box<dyn TranscriptProcessor>>) -> Self {
        Self { processors }
    }

    pub fn process(&self, transcript: Transcript) -> Transcript {
        self.processors
            .iter()
            .fold(transcript, |transcript, processor| {
                processor.process(transcript)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{IdentityProcessor, PostProcessingPipeline, TranscriptProcessor};
    use crate::domain::Transcript;

    #[test]
    fn identity_preserves_distinct_raw_and_final_text() {
        let transcript = Transcript {
            raw_text: "raw recognition".into(),
            final_text: "final transcript".into(),
            segments: vec![],
            engine: "parakeet".into(),
            model: "test".into(),
        };

        let result = PostProcessingPipeline::new(vec![Box::new(IdentityProcessor)])
            .process(transcript.clone());

        assert_eq!(result, transcript);
        assert_ne!(result.raw_text, result.final_text);
    }

    #[test]
    fn processors_run_in_order() {
        struct Append(&'static str);
        impl TranscriptProcessor for Append {
            fn process(&self, mut transcript: Transcript) -> Transcript {
                transcript.final_text.push_str(self.0);
                transcript
            }
        }

        let transcript = Transcript {
            raw_text: "raw".into(),
            final_text: "final".into(),
            segments: vec![],
            engine: "parakeet".into(),
            model: "test".into(),
        };
        let result =
            PostProcessingPipeline::new(vec![Box::new(Append(" one")), Box::new(Append(" two"))])
                .process(transcript);

        assert_eq!(result.final_text, "final one two");
    }
}
