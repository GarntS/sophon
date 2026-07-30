//! A safe, stateful wrapper around the qwentts.cpp C ABI.
//!
//! The selected Cargo backend feature builds the vendored qwentts.cpp source.
//! The crate exposes buffered and synchronous streaming synthesis while owning
//! one loaded Qwen engine.

mod raw;

use std::{
    ffi::{CStr, CString, NulError, c_char, c_void},
    path::Path,
    ptr::NonNull,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU8, Ordering},
    },
};

use thiserror::Error;

/// Errors returned by the safe qwentts.cpp API.
#[derive(Debug, Error)]
pub enum QwenTtsError {
    #[error("{field} contains an interior NUL byte")]
    InteriorNul { field: &'static str },
    #[error("text must not be empty")]
    EmptyText,
    #[error("duration must be finite, positive, and convertible to native tokens")]
    InvalidDuration,
    #[error("a model must be loaded before {operation}")]
    ModelNotLoaded { operation: &'static str },
    #[error("native initialization failed{diagnostic}")]
    Initialization { diagnostic: String },
    #[error("native {operation} failed with status {status}{diagnostic}")]
    Native {
        operation: &'static str,
        status: i32,
        diagnostic: String,
    },
    #[error("streaming synthesis was cancelled by the consumer")]
    StreamCancelled,
    #[error("streaming synthesis callback panicked")]
    StreamCallbackPanicked,
    #[error("native streaming callback supplied an invalid sample buffer")]
    InvalidStreamChunk,
    #[error("WAV output failed: {0}")]
    Wav(#[from] hound::Error),
}

impl QwenTtsError {
    fn native(operation: &'static str, status: i32) -> Self {
        Self::Native {
            operation,
            status,
            diagnostic: native_diagnostic(),
        }
    }
}

fn native_diagnostic() -> String {
    // SAFETY: qwen returns either null or a thread-local NUL-terminated string.
    unsafe {
        let message = raw::qt_last_error();
        if message.is_null() {
            String::new()
        } else {
            let message = CStr::from_ptr(message).to_string_lossy();
            if message.is_empty() {
                String::new()
            } else {
                format!(": {message}")
            }
        }
    }
}

fn c_string(value: &str, field: &'static str) -> Result<CString, QwenTtsError> {
    CString::new(value).map_err(|_: NulError| QwenTtsError::InteriorNul { field })
}

fn path_string(path: &Path, field: &'static str) -> Result<CString, QwenTtsError> {
    c_string(&path.to_string_lossy(), field)
}

/// Severity of a qwentts.cpp log message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// A process-wide qwentts.cpp log callback.
pub type LogCallback = Arc<dyn Fn(LogLevel, &str) + Send + Sync + 'static>;

static LOG_CALLBACK: RwLock<Option<LogCallback>> = RwLock::new(None);
static LOG_INSTALL_LOCK: Mutex<()> = Mutex::new(());

unsafe extern "C" fn log_trampoline(
    level: raw::qt_log_level,
    message: *const c_char,
    _: *mut c_void,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let level = match level {
            raw::qt_log_level_QT_LOG_DEBUG => LogLevel::Debug,
            raw::qt_log_level_QT_LOG_INFO => LogLevel::Info,
            raw::qt_log_level_QT_LOG_WARN => LogLevel::Warning,
            raw::qt_log_level_QT_LOG_ERROR => LogLevel::Error,
            _ => LogLevel::Error,
        };
        let message = if message.is_null() {
            String::new()
        } else {
            // SAFETY: qwentts.cpp supplies a NUL-terminated message valid for
            // this call. Copying it prevents borrowed native storage escaping.
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned()
        };
        let callback = LOG_CALLBACK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(callback) = callback {
            callback(level, &message);
        }
    }));
}

/// Replaces the process-wide Rust log callback.
///
/// The callback may run reentrantly on caller or native worker threads. Pass
/// `None` to restore qwentts.cpp's default native logging behavior.
pub fn set_log_callback(callback: Option<LogCallback>) {
    let _install_guard = LOG_INSTALL_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *LOG_CALLBACK
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = callback;
    // SAFETY: the trampoline has C ABI, uses no borrowed user data, and catches
    // all callback panics before returning across the native boundary.
    unsafe {
        raw::qt_log_set(
            callback_is_installed().then_some(log_trampoline),
            std::ptr::null_mut(),
        )
    }
}

fn callback_is_installed() -> bool {
    LOG_CALLBACK
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
}

/// Native model-loading settings.
#[derive(Debug, Clone, Copy)]
pub struct ModelOptions {
    pub use_flash_attention: bool,
    pub clamp_fp16: bool,
    pub max_batch: u32,
    pub codec_chunk_seconds: f32,
}

impl Default for ModelOptions {
    fn default() -> Self {
        Self {
            use_flash_attention: true,
            clamp_fp16: false,
            max_batch: 1,
            codec_chunk_seconds: 24.0,
        }
    }
}

/// A language hint accepted by qwentts.cpp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Auto,
    English,
    Chinese,
    Japanese,
    Korean,
    German,
    French,
    Russian,
    Portuguese,
    Spanish,
    Italian,
}

impl Language {
    fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::English => Some("english"),
            Self::Chinese => Some("chinese"),
            Self::Japanese => Some("japanese"),
            Self::Korean => Some("korean"),
            Self::German => Some("german"),
            Self::French => Some("french"),
            Self::Russian => Some("russian"),
            Self::Portuguese => Some("portuguese"),
            Self::Spanish => Some("spanish"),
            Self::Italian => Some("italian"),
        }
    }
}

/// Sampling controls for a synthesis request.
#[derive(Debug, Clone)]
pub struct SamplingOptions {
    pub seed: Option<i64>,
    pub max_new_tokens: u32,
    pub temperature: f32,
    pub top_k: u32,
    pub top_p: f32,
    pub repetition_penalty: f32,
}

impl Default for SamplingOptions {
    fn default() -> Self {
        Self {
            seed: None,
            max_new_tokens: 2048,
            temperature: 0.9,
            top_k: 50,
            top_p: 1.0,
            repetition_penalty: 1.05,
        }
    }
}

/// The Qwen-specific voice intent for a synthesis request.
#[derive(Debug, Default)]
pub enum Voice<'a> {
    #[default]
    Default,
    Named(&'a str),
    Clone(&'a VoiceReference),
    CloneWithTranscript {
        reference: &'a VoiceReference,
        transcript: &'a str,
    },
    Design(&'a str),
}

/// Options for one buffered synthesis operation.
#[derive(Debug)]
pub struct SynthesisOptions<'a> {
    pub language: Language,
    pub sampling: SamplingOptions,
    pub voice: Voice<'a>,
}

impl Default for SynthesisOptions<'_> {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            sampling: SamplingOptions::default(),
            voice: Voice::Default,
        }
    }
}

/// Rust-owned audio produced by one native streaming callback.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamChunk {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// Control returned by a streaming consumer after receiving a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamControl {
    Continue,
    Cancel,
}

const STREAM_RUNNING: u8 = 0;
const STREAM_CANCELLED: u8 = 1;
const STREAM_PANICKED: u8 = 2;
const STREAM_INVALID_CHUNK: u8 = 3;
const QWEN_SAMPLE_RATE: u32 = 24_000;

struct StreamCallbackState<F> {
    callback: Mutex<F>,
    termination: AtomicU8,
}

impl<F> StreamCallbackState<F> {
    fn new(callback: F) -> Self {
        Self {
            callback: Mutex::new(callback),
            termination: AtomicU8::new(STREAM_RUNNING),
        }
    }

    fn error(&self) -> Option<QwenTtsError> {
        match self.termination.load(Ordering::Acquire) {
            STREAM_RUNNING => None,
            STREAM_CANCELLED => Some(QwenTtsError::StreamCancelled),
            STREAM_PANICKED => Some(QwenTtsError::StreamCallbackPanicked),
            STREAM_INVALID_CHUNK => Some(QwenTtsError::InvalidStreamChunk),
            _ => Some(QwenTtsError::InvalidStreamChunk),
        }
    }
}

unsafe extern "C" fn stream_chunk_trampoline<F>(
    samples: *const f32,
    n_samples: i32,
    user_data: *mut c_void,
) -> bool
where
    F: FnMut(StreamChunk) -> StreamControl + Send,
{
    if user_data.is_null() {
        return false;
    }
    // SAFETY: synthesize_streaming passes a live, pinned state for the
    // duration of the synchronous native call. Native callback invocations
    // for one request are serialized; the mutex also makes worker-thread
    // access safe and guards the FnMut state.
    let state = unsafe { &*(user_data.cast::<StreamCallbackState<F>>()) };
    if samples.is_null() || n_samples <= 0 {
        state
            .termination
            .store(STREAM_INVALID_CHUNK, Ordering::Release);
        return false;
    }
    if state.termination.load(Ordering::Acquire) != STREAM_RUNNING {
        return false;
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: qwentts.cpp guarantees this temporary buffer contains
        // n_samples initialized values for this callback invocation. Copy it
        // before calling safe consumer code so native storage cannot escape.
        let owned = unsafe { std::slice::from_raw_parts(samples, n_samples as usize) }.to_vec();
        let mut callback = state
            .callback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        callback(StreamChunk {
            samples: owned,
            sample_rate: QWEN_SAMPLE_RATE,
        })
    }));
    match result {
        Ok(StreamControl::Continue) => true,
        Ok(StreamControl::Cancel) => {
            state.termination.store(STREAM_CANCELLED, Ordering::Release);
            false
        }
        Err(_) => {
            state.termination.store(STREAM_PANICKED, Ordering::Release);
            false
        }
    }
}

/// Rust-owned audio produced by a synthesis call.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthesisResult {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl SynthesisResult {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.samples.len() as f64 / f64::from(self.sample_rate)
        }
    }

    pub fn write_wav(&self, path: impl AsRef<Path>) -> Result<(), QwenTtsError> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for sample in &self.samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        Ok(())
    }
}

fn with_synthesis_params<T>(
    text: &str,
    options: Option<SynthesisOptions<'_>>,
    invoke: impl FnOnce(&mut raw::qt_tts_params) -> Result<T, QwenTtsError>,
) -> Result<T, QwenTtsError> {
    if text.is_empty() {
        return Err(QwenTtsError::EmptyText);
    }
    let text = c_string(text, "text")?;
    let options = options.unwrap_or_default();
    let language = options
        .language
        .as_str()
        .map(|value| c_string(value, "language"))
        .transpose()?;
    let (speaker, instruction, transcript, reference) = match options.voice {
        Voice::Default => (None, None, None, None),
        Voice::Named(name) => (Some(c_string(name, "speaker")?), None, None, None),
        Voice::Clone(reference) => (None, None, None, Some(reference)),
        Voice::CloneWithTranscript {
            reference,
            transcript,
        } => (
            None,
            None,
            Some(c_string(transcript, "transcript")?),
            Some(reference),
        ),
        Voice::Design(instruction) => (
            None,
            Some(c_string(instruction, "instruction")?),
            None,
            None,
        ),
    };
    // SAFETY: native defaults initialize every field. All pointers installed
    // below refer to locals that remain alive through the synchronous invoke.
    unsafe {
        let mut params = std::mem::zeroed();
        raw::qt_tts_default_params(&mut params);
        params.text = text.as_ptr();
        params.lang = language
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr());
        params.speaker = speaker
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr());
        params.instruct = instruction
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr());
        params.ref_text = transcript
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr());
        params.seed = options.sampling.seed.unwrap_or(-1);
        params.max_new_tokens = options.sampling.max_new_tokens as i32;
        params.temperature = options.sampling.temperature;
        params.top_k = options.sampling.top_k as i32;
        params.top_p = options.sampling.top_p;
        params.repetition_penalty = options.sampling.repetition_penalty;
        if let Some(reference) = reference {
            params.ref_spk_emb = reference.raw.ref_spk_emb;
            params.ref_spk_dim = reference.raw.ref_spk_dim;
            params.ref_codes = reference.raw.ref_codes;
            params.ref_T = reference.raw.ref_T;
        }
        invoke(&mut params)
    }
}

/// A native pre-encoded clone reference. It frees its native buffers on drop.
pub struct VoiceReference {
    raw: raw::qt_voice_ref,
}

impl std::fmt::Debug for VoiceReference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VoiceReference(..)")
    }
}

impl Drop for VoiceReference {
    fn drop(&mut self) {
        // SAFETY: raw was zeroed or initialized by qt_extract_voice_ref and is owned here.
        unsafe { raw::qt_voice_ref_free(&mut self.raw) }
    }
}

/// A mutable qwentts.cpp engine. It starts unloaded.
///
/// The engine is movable between threads, but all safe operations require
/// exclusive access and it is deliberately not [`Sync`].
pub struct QwenTtsEngine {
    context: Option<NonNull<raw::qt_context>>,
}

// SAFETY: The pinned qwentts.cpp ABI documents its context as thread-safe and
// serializes GPU access within a context. Moving exclusive ownership does not
// introduce concurrent access; every safe operation continues to require
// `&mut self`. This assertion must be reviewed when updating the native source.
unsafe impl Send for QwenTtsEngine {}

impl Default for QwenTtsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl QwenTtsEngine {
    pub fn new() -> Self {
        Self { context: None }
    }

    pub fn is_loaded(&self) -> bool {
        self.context.is_some()
    }

    pub fn load_model(
        &mut self,
        talker_path: impl AsRef<Path>,
        codec_path: impl AsRef<Path>,
    ) -> Result<(), QwenTtsError> {
        self.load_model_with_options(talker_path, codec_path, ModelOptions::default())
    }

    pub fn load_model_with_options(
        &mut self,
        talker_path: impl AsRef<Path>,
        codec_path: impl AsRef<Path>,
        options: ModelOptions,
    ) -> Result<(), QwenTtsError> {
        let talker = path_string(talker_path.as_ref(), "talker model path")?;
        let codec = path_string(codec_path.as_ref(), "codec model path")?;
        // SAFETY: native defaults initialize every ABI field; C strings outlive qt_init.
        let context = unsafe {
            let mut params = std::mem::zeroed();
            raw::qt_init_default_params(&mut params);
            params.talker_path = talker.as_ptr();
            params.codec_path = codec.as_ptr();
            params.use_fa = options.use_flash_attention;
            params.clamp_fp16 = options.clamp_fp16;
            params.max_batch = options.max_batch as i32;
            params.codec_chunk_sec = options.codec_chunk_seconds;
            NonNull::new(raw::qt_init(&params))
        }
        .ok_or_else(|| QwenTtsError::Initialization {
            diagnostic: native_diagnostic(),
        })?;
        self.unload_model();
        self.context = Some(context);
        Ok(())
    }

    pub fn unload_model(&mut self) {
        if let Some(context) = self.context.take() {
            // SAFETY: context is exclusively owned by this engine.
            unsafe { raw::qt_free(context.as_ptr()) }
        }
    }

    fn context(&self, operation: &'static str) -> Result<NonNull<raw::qt_context>, QwenTtsError> {
        self.context
            .ok_or(QwenTtsError::ModelNotLoaded { operation })
    }

    pub fn synthesize(
        &mut self,
        text: &str,
        options: Option<SynthesisOptions<'_>>,
    ) -> Result<SynthesisResult, QwenTtsError> {
        let context = self.context("synthesis")?;
        with_synthesis_params(text, options, |params| {
            // SAFETY: context is exclusively borrowed and params owns valid
            // pointers for this synchronous call.
            unsafe {
                let mut audio: raw::qt_audio = std::mem::zeroed();
                let status = raw::qt_synthesize(context.as_ptr(), params, &mut audio) as i32;
                if status != 0 {
                    raw::qt_audio_free(&mut audio);
                    return Err(QwenTtsError::native("synthesis", status));
                }
                let samples = if audio.samples.is_null() || audio.n_samples <= 0 {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(audio.samples, audio.n_samples as usize).to_vec()
                };
                let sample_rate = audio.sample_rate.max(0) as u32;
                raw::qt_audio_free(&mut audio);
                Ok(SynthesisResult {
                    samples,
                    sample_rate,
                })
            }
        })
    }

    /// Runs synchronous native streaming synthesis. The callback may execute
    /// on a qwentts.cpp worker thread, receives only owned chunks, and must not
    /// call back into this exclusively borrowed engine.
    pub fn synthesize_streaming<F>(
        &mut self,
        text: &str,
        options: Option<SynthesisOptions<'_>>,
        callback: F,
    ) -> Result<(), QwenTtsError>
    where
        F: FnMut(StreamChunk) -> StreamControl + Send,
    {
        let context = self.context("streaming synthesis")?;
        let state = StreamCallbackState::new(callback);
        with_synthesis_params(text, options, |params| {
            params.on_chunk = Some(stream_chunk_trampoline::<F>);
            params.on_chunk_user_data = (&state as *const StreamCallbackState<F>).cast_mut().cast();
            // SAFETY: context and params remain valid through this synchronous
            // call; state remains pinned on this stack until native callbacks
            // have stopped and qt_synthesize returns.
            unsafe {
                let mut audio: raw::qt_audio = std::mem::zeroed();
                let status = raw::qt_synthesize(context.as_ptr(), params, &mut audio) as i32;
                // Streaming mode promises no buffered duplicate. Freeing the
                // zeroed output is safe and also contains a native violation.
                raw::qt_audio_free(&mut audio);
                if let Some(error) = state.error() {
                    return Err(error);
                }
                if status != 0 {
                    return Err(QwenTtsError::native("streaming synthesis", status));
                }
                Ok(())
            }
        })
    }

    pub fn extract_voice_reference(
        &mut self,
        samples_24khz_mono: &[f32],
    ) -> Result<VoiceReference, QwenTtsError> {
        let context = self.context("voice-reference extraction")?;
        // SAFETY: slice stays valid for the synchronous native call and raw is zeroed for native ownership.
        unsafe {
            let mut reference: raw::qt_voice_ref = std::mem::zeroed();
            let status = raw::qt_extract_voice_ref(
                context.as_ptr(),
                samples_24khz_mono.as_ptr(),
                samples_24khz_mono.len() as i32,
                &mut reference,
            ) as i32;
            if status != 0 {
                return Err(QwenTtsError::native("voice-reference extraction", status));
            }
            Ok(VoiceReference { raw: reference })
        }
    }

    /// Converts an output duration to the loaded model's generation-token count.
    pub fn duration_sec_to_tokens(&self, duration_secs: f32) -> Result<u32, QwenTtsError> {
        const TOKENS_PER_SECOND: f32 = 12.5;
        if !duration_secs.is_finite()
            || duration_secs <= 0.0
            || duration_secs > i32::MAX as f32 / TOKENS_PER_SECOND
        {
            return Err(QwenTtsError::InvalidDuration);
        }
        let context = self.context("duration-to-token conversion")?;
        // SAFETY: context belongs to this loaded engine and the validated value
        // is within the native conversion's positive i32 output range.
        let tokens = unsafe { raw::qt_duration_sec_to_tokens(context.as_ptr(), duration_secs) };
        u32::try_from(tokens)
            .map_err(|_| QwenTtsError::native("duration-to-token conversion", tokens))
    }

    pub fn speakers(&self) -> Result<Vec<String>, QwenTtsError> {
        let context = self.context("speaker enumeration")?;
        // SAFETY: speaker pointers remain valid while context is loaded; strings are copied immediately.
        unsafe {
            let count = raw::qt_n_speakers(context.as_ptr()).max(0);
            Ok((0..count)
                .filter_map(|index| {
                    let name = raw::qt_speaker_name(context.as_ptr(), index);
                    (!name.is_null()).then(|| CStr::from_ptr(name).to_string_lossy().into_owned())
                })
                .collect())
        }
    }
}

impl Drop for QwenTtsEngine {
    fn drop(&mut self) {
        self.unload_model();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use static_assertions::{assert_impl_all, assert_not_impl_any};
    use std::sync::Mutex;

    assert_impl_all!(QwenTtsEngine: Send);
    assert_not_impl_any!(QwenTtsEngine: Sync);
    assert_impl_all!(LogCallback: Send, Sync);
    assert_impl_all!(StreamChunk: Send);
    assert_impl_all!(StreamControl: Send);

    static LOG_TEST_LOCK: Mutex<()> = Mutex::new(());

    unsafe fn emit_stream_chunk<F>(
        state: &StreamCallbackState<F>,
        samples: *const f32,
        n_samples: i32,
    ) -> bool
    where
        F: FnMut(StreamChunk) -> StreamControl + Send,
    {
        // SAFETY: the fixture keeps state and any supplied samples live.
        unsafe {
            stream_chunk_trampoline::<F>(
                samples,
                n_samples,
                (state as *const StreamCallbackState<F>).cast_mut().cast(),
            )
        }
    }

    #[test]
    fn stream_trampoline_copies_chunks_and_contains_consumer_termination() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&chunks);
        let state = StreamCallbackState::new(move |chunk: StreamChunk| {
            captured.lock().unwrap().push(chunk);
            StreamControl::Continue
        });
        let mut native = vec![0.25, -0.5];
        // SAFETY: the state and native samples remain live for this direct
        // trampoline fixture invocation.
        assert!(unsafe { emit_stream_chunk(&state, native.as_ptr(), native.len() as i32) });
        native.fill(1.0);
        assert!(unsafe { emit_stream_chunk(&state, native.as_ptr(), native.len() as i32) });
        let chunks = chunks.lock().unwrap();
        assert_eq!(chunks[0].samples, [0.25, -0.5]);
        assert_eq!(chunks[1].samples, [1.0, 1.0]);
        assert_eq!(chunks[0].sample_rate, QWEN_SAMPLE_RATE);
        drop(chunks);
        assert!(state.error().is_none());

        let cancelled = StreamCallbackState::new(|_: StreamChunk| StreamControl::Cancel);
        assert!(!unsafe { emit_stream_chunk(&cancelled, native.as_ptr(), native.len() as i32) });
        assert!(matches!(
            cancelled.error(),
            Some(QwenTtsError::StreamCancelled)
        ));

        let panicked =
            StreamCallbackState::new(|_: StreamChunk| -> StreamControl { panic!("fixture panic") });
        assert!(!unsafe { emit_stream_chunk(&panicked, native.as_ptr(), native.len() as i32) });
        assert!(matches!(
            panicked.error(),
            Some(QwenTtsError::StreamCallbackPanicked)
        ));

        let invalid = StreamCallbackState::new(|_: StreamChunk| StreamControl::Continue);
        assert!(!unsafe { emit_stream_chunk(&invalid, std::ptr::null(), 1) });
        assert!(matches!(
            invalid.error(),
            Some(QwenTtsError::InvalidStreamChunk)
        ));
    }

    fn emit_test_log(level: raw::qt_log_level, message: &[u8]) {
        let message = CString::new(message).unwrap();
        // SAFETY: the message is NUL-terminated and remains alive for the call.
        unsafe { log_trampoline(level, message.as_ptr(), std::ptr::null_mut()) }
    }

    #[test]
    fn log_callback_maps_levels_copies_messages_and_can_be_replaced() {
        let _guard = LOG_TEST_LOCK.lock().unwrap();
        let first_messages = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&first_messages);
        set_log_callback(Some(Arc::new(move |level, message| {
            captured.lock().unwrap().push((level, message.to_owned()));
        })));
        emit_test_log(raw::qt_log_level_QT_LOG_DEBUG, b"debug");
        emit_test_log(raw::qt_log_level_QT_LOG_INFO, b"info");
        emit_test_log(raw::qt_log_level_QT_LOG_WARN, b"warn");
        emit_test_log(raw::qt_log_level_QT_LOG_ERROR, b"bad \xff utf-8");
        assert_eq!(
            *first_messages.lock().unwrap(),
            vec![
                (LogLevel::Debug, "debug".to_owned()),
                (LogLevel::Info, "info".to_owned()),
                (LogLevel::Warning, "warn".to_owned()),
                (LogLevel::Error, "bad \u{fffd} utf-8".to_owned()),
            ]
        );

        let replacement_called = Arc::new(Mutex::new(false));
        let captured = Arc::clone(&replacement_called);
        set_log_callback(Some(Arc::new(move |_, _| {
            *captured.lock().unwrap() = true;
        })));
        emit_test_log(raw::qt_log_level_QT_LOG_INFO, b"replacement");
        assert!(*replacement_called.lock().unwrap());

        set_log_callback(None);
        assert!(!callback_is_installed());
    }

    #[test]
    fn log_callback_panics_are_contained() {
        let _guard = LOG_TEST_LOCK.lock().unwrap();
        set_log_callback(Some(Arc::new(|_, _| panic!("logger panic"))));
        emit_test_log(raw::qt_log_level_QT_LOG_ERROR, b"panic safely");
        set_log_callback(None);
    }

    #[test]
    fn unloaded_engine_can_move_between_threads() {
        let engine = QwenTtsEngine::new();
        let engine = std::thread::spawn(move || engine).join().unwrap();
        assert!(!engine.is_loaded());
    }

    #[test]
    fn unloaded_engine_does_not_call_native_synthesis() {
        assert!(matches!(
            QwenTtsEngine::new().synthesize("hello", None),
            Err(QwenTtsError::ModelNotLoaded { .. })
        ));
        assert!(matches!(
            QwenTtsEngine::new()
                .synthesize_streaming("hello", None, |_: StreamChunk| StreamControl::Continue,),
            Err(QwenTtsError::ModelNotLoaded { .. })
        ));
    }

    #[test]
    fn shared_synthesis_options_forward_every_sampling_field() {
        let sampling = SamplingOptions {
            seed: Some(42),
            max_new_tokens: 123,
            temperature: 0.4,
            top_k: 17,
            top_p: 0.8,
            repetition_penalty: 1.2,
        };
        with_synthesis_params(
            "hello",
            Some(SynthesisOptions {
                language: Language::German,
                sampling,
                voice: Voice::Named("speaker-a"),
            }),
            |params| {
                // SAFETY: with_synthesis_params keeps all C strings alive for
                // this closure invocation.
                unsafe {
                    assert_eq!(CStr::from_ptr(params.text).to_str().unwrap(), "hello");
                    assert_eq!(CStr::from_ptr(params.lang).to_str().unwrap(), "german");
                    assert_eq!(
                        CStr::from_ptr(params.speaker).to_str().unwrap(),
                        "speaker-a"
                    );
                }
                assert_eq!(params.seed, 42);
                assert_eq!(params.max_new_tokens, 123);
                assert_eq!(params.temperature, 0.4);
                assert_eq!(params.top_k, 17);
                assert_eq!(params.top_p, 0.8);
                assert_eq!(params.repetition_penalty, 1.2);
                assert!(params.on_chunk.is_none());
                Ok(())
            },
        )
        .unwrap();
    }

    #[test]
    fn duration_conversion_requires_a_loaded_engine() {
        assert!(matches!(
            QwenTtsEngine::new().duration_sec_to_tokens(1.0),
            Err(QwenTtsError::ModelNotLoaded { .. })
        ));
    }

    #[test]
    fn duration_conversion_rejects_invalid_inputs() {
        let engine = QwenTtsEngine::new();
        for duration in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -1.0,
            f32::MAX,
        ] {
            assert!(matches!(
                engine.duration_sec_to_tokens(duration),
                Err(QwenTtsError::InvalidDuration)
            ));
        }
    }

    #[test]
    fn interior_nuls_are_rejected_before_native_calls() {
        assert!(matches!(
            c_string("bad\0text", "text"),
            Err(QwenTtsError::InteriorNul { field: "text" })
        ));
    }

    #[test]
    fn native_failure_preserves_a_copied_diagnostic() {
        let _guard = LOG_TEST_LOCK.lock().unwrap();
        // SAFETY: the ABI explicitly accepts null context/audio arguments and reports an error.
        unsafe {
            let mut reference: raw::qt_voice_ref = std::mem::zeroed();
            let status = raw::qt_extract_voice_ref(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut reference,
            ) as i32;
            assert_ne!(status, 0);
            assert!(
                matches!(QwenTtsError::native("voice-reference extraction", status), QwenTtsError::Native { diagnostic, .. } if diagnostic.contains("q, ref_audio_24k or out is NULL"))
            );
            raw::qt_voice_ref_free(&mut reference);
        }
    }

    #[test]
    fn zeroed_voice_reference_can_be_dropped_safely() {
        // SAFETY: qt_voice_ref_free explicitly accepts a zero-initialized reference.
        let reference = VoiceReference {
            raw: unsafe { std::mem::zeroed() },
        };
        drop(reference);
    }

    #[test]
    fn owned_results_calculate_duration_and_write_float_wav() {
        let result = SynthesisResult {
            samples: vec![0.0, 0.25, -0.25],
            sample_rate: 24_000,
        };
        assert_eq!(result.duration_secs(), 3.0 / 24_000.0);
        let file = tempfile::NamedTempFile::new().unwrap();
        result.write_wav(file.path()).unwrap();
        let reader = hound::WavReader::open(file.path()).unwrap();
        assert_eq!(reader.spec().sample_format, hound::SampleFormat::Float);
        assert_eq!(reader.duration(), 3);
    }
}
