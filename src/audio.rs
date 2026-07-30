//! WAV parsing plus file and Unix-file-descriptor ingestion.

use std::{
    fs::{File, OpenOptions},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    os::fd::OwnedFd,
    path::Path,
};

use hound::{SampleFormat, WavReader};
use memfd::{FileSeal, MemfdOptions};
use rubato::{Fft, FixedSync, Resampler, audioadapter_buffers::direct::InterleavedSlice};

use crate::error::SophonError;

#[derive(Debug, Clone, PartialEq)]
pub struct OwnedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedWav {
    /// Samples in source-frame order, interleaved by channel.
    pub samples: Vec<f32>,
    pub source_rate: u32,
    pub channels: u16,
}

pub fn parse_wav<R: Read>(
    reader: R,
    max_bytes: u64,
    max_seconds: u64,
) -> Result<DecodedWav, SophonError> {
    let mut encoded = Vec::new();
    reader
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut encoded)
        .map_err(|error| SophonError::InvalidAudio(error.to_string()))?;
    if encoded.len() as u64 > max_bytes {
        return Err(SophonError::ResourceLimit(
            "encoded audio exceeds limit".into(),
        ));
    }

    let mut wav = WavReader::new(Cursor::new(encoded))
        .map_err(|error| SophonError::InvalidAudio(error.to_string()))?;
    let spec = wav.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(SophonError::InvalidAudio(
            "WAV channel count and sample rate must be nonzero".into(),
        ));
    }

    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, bits @ (8 | 16 | 24 | 32)) => {
            let scale = 2_f64.powi(i32::from(bits) - 1);
            wav.samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|sample| (f64::from(sample) / scale) as f32)
                        .map_err(|error| SophonError::InvalidAudio(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        (SampleFormat::Float, 32) => wav
            .samples::<f32>()
            .map(|sample| sample.map_err(|error| SophonError::InvalidAudio(error.to_string())))
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(SophonError::InvalidAudio(format!(
                "unsupported WAV sample representation: {:?} {}-bit",
                spec.sample_format, spec.bits_per_sample
            )));
        }
    };
    let channels = usize::from(spec.channels);
    if samples.len() % channels != 0 {
        return Err(SophonError::InvalidAudio(
            "WAV ends with an incomplete channel frame".into(),
        ));
    }
    let source_frames = samples.len() / channels;
    if source_frames as u128 > u128::from(max_seconds) * u128::from(spec.sample_rate) {
        return Err(SophonError::ResourceLimit(
            "decoded audio exceeds duration limit".into(),
        ));
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err(SophonError::InvalidAudio(
            "WAV contains a non-finite sample".into(),
        ));
    }
    Ok(DecodedWav {
        samples,
        source_rate: spec.sample_rate,
        channels: spec.channels,
    })
}

pub fn downmix_wav(audio: DecodedWav) -> Result<OwnedAudio, SophonError> {
    if audio.channels == 0 || audio.source_rate == 0 {
        return Err(SophonError::InvalidAudio(
            "WAV channel count and sample rate must be nonzero".into(),
        ));
    }
    let channels = usize::from(audio.channels);
    let mut frames = audio.samples.chunks_exact(channels);
    let mut mono = Vec::with_capacity(audio.samples.len() / channels);
    for frame in &mut frames {
        if frame.iter().any(|sample| !sample.is_finite()) {
            return Err(SophonError::InvalidAudio(
                "WAV contains a non-finite sample".into(),
            ));
        }
        mono.push(
            frame.iter().map(|sample| f64::from(*sample)).sum::<f64>() as f32 / channels as f32,
        );
    }
    if !frames.remainder().is_empty() {
        return Err(SophonError::InvalidAudio(
            "WAV ends with an incomplete channel frame".into(),
        ));
    }
    Ok(OwnedAudio {
        samples: mono,
        sample_rate: audio.source_rate,
    })
}

pub fn resample_mono(audio: OwnedAudio, target_rate: u32) -> Result<OwnedAudio, SophonError> {
    if audio.sample_rate == 0 || target_rate == 0 {
        return Err(SophonError::InvalidAudio(
            "source and model sample rates must be nonzero".into(),
        ));
    }
    if audio.samples.iter().any(|sample| !sample.is_finite()) {
        return Err(SophonError::InvalidAudio(
            "WAV contains a non-finite sample".into(),
        ));
    }
    if audio.sample_rate == target_rate {
        return Ok(audio);
    }
    if audio.samples.is_empty() {
        return Ok(OwnedAudio {
            samples: Vec::new(),
            sample_rate: target_rate,
        });
    }

    let input_frames = audio.samples.len();
    let input = InterleavedSlice::new(&audio.samples, 1, input_frames)
        .map_err(|error| SophonError::InvalidAudio(error.to_string()))?;
    let chunk_size = input_frames.clamp(1, 1024);
    let mut resampler = Fft::<f32>::new(
        audio.sample_rate as usize,
        target_rate as usize,
        chunk_size,
        1,
        FixedSync::Input,
    )
    .map_err(|error| SophonError::InvalidAudio(format!("cannot resample WAV: {error}")))?;
    let output = resampler
        .process_all(&input, input_frames, None)
        .map_err(|error| SophonError::InvalidAudio(format!("cannot resample WAV: {error}")))?;
    Ok(OwnedAudio {
        samples: output.take_data(),
        sample_rate: target_rate,
    })
}

pub fn read_file(path: &Path, max_bytes: u64, max_seconds: u64) -> Result<DecodedWav, SophonError> {
    if !path.is_absolute() {
        return Err(SophonError::InvalidAudio(
            "audio path must be absolute".into(),
        ));
    }
    let metadata = std::fs::metadata(path).map_err(|e| SophonError::InvalidAudio(e.to_string()))?;
    if !metadata.is_file() {
        return Err(SophonError::InvalidAudio(
            "audio path must identify a regular file".into(),
        ));
    }
    if metadata.len() > max_bytes {
        return Err(SophonError::ResourceLimit(
            "encoded audio exceeds limit".into(),
        ));
    }
    parse_wav(
        File::open(path).map_err(|e| SophonError::InvalidAudio(e.to_string()))?,
        max_bytes,
        max_seconds,
    )
}

/// Takes ownership of a transferred Unix descriptor, seeks it to byte zero,
/// and parses it without requiring the descriptor to be a literal memfd.
pub fn read_unix_fd(
    fd: OwnedFd,
    max_bytes: u64,
    max_seconds: u64,
) -> Result<DecodedWav, SophonError> {
    let mut file = File::from(fd);
    let size = file.metadata().ok().map(|metadata| metadata.len());
    if size.is_some_and(|size| size > max_bytes) {
        return Err(SophonError::ResourceLimit(
            "encoded audio exceeds limit".into(),
        ));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|e| SophonError::InvalidAudio(format!("audio descriptor is not seekable: {e}")))?;
    parse_wav(file, max_bytes, max_seconds)
}

pub fn read_seekable<R: Read + Seek>(
    mut reader: R,
    max_bytes: u64,
    max_seconds: u64,
) -> Result<DecodedWav, SophonError> {
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| SophonError::InvalidAudio(format!("audio descriptor is not seekable: {e}")))?;
    parse_wav(reader, max_bytes, max_seconds)
}

pub fn decode_clone_wav<R: Read + Seek>(
    mut reader: R,
    max_bytes: u64,
    max_seconds: u64,
) -> Result<OwnedAudio, SophonError> {
    let encoded_bytes = reader.seek(SeekFrom::End(0)).map_err(|error| {
        SophonError::InvalidReferenceAudio(format!("reference descriptor is not seekable: {error}"))
    })?;
    if encoded_bytes > max_bytes {
        return Err(SophonError::ResourceLimit(
            "encoded reference audio exceeds limit".into(),
        ));
    }
    reader.seek(SeekFrom::Start(0)).map_err(|error| {
        SophonError::InvalidReferenceAudio(format!(
            "reference descriptor cannot rewind to byte zero: {error}"
        ))
    })?;
    let mut wav = WavReader::new(reader)
        .map_err(|error| SophonError::InvalidReferenceAudio(error.to_string()))?;
    let spec = wav.spec();
    if spec.channels != 1
        || spec.sample_rate != 24_000
        || spec.bits_per_sample != 32
        || spec.sample_format != SampleFormat::Float
    {
        return Err(SophonError::InvalidReferenceAudio(
            "expected mono 24 kHz 32-bit IEEE-float WAV".into(),
        ));
    }
    let samples = wav
        .samples::<f32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SophonError::InvalidReferenceAudio(error.to_string()))?;
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err(SophonError::InvalidReferenceAudio(
            "reference audio contains a non-finite sample".into(),
        ));
    }
    if samples.len() as u128 > u128::from(max_seconds) * 24_000 {
        return Err(SophonError::ResourceLimit(
            "decoded reference audio exceeds duration limit".into(),
        ));
    }
    Ok(OwnedAudio {
        samples,
        sample_rate: 24_000,
    })
}

pub fn read_clone_fd(
    fd: OwnedFd,
    max_bytes: u64,
    max_seconds: u64,
) -> Result<OwnedAudio, SophonError> {
    decode_clone_wav(File::from(fd), max_bytes, max_seconds)
}

pub fn encode_float_wav(audio: &OwnedAudio, max_seconds: u64) -> Result<Vec<u8>, SophonError> {
    if audio.sample_rate == 0 || audio.sample_rate > 384_000 {
        return Err(SophonError::SynthesisFailed(format!(
            "provider returned invalid sample rate {}",
            audio.sample_rate
        )));
    }
    if audio.samples.is_empty() {
        return Err(SophonError::SynthesisFailed(
            "provider returned no audio frames".into(),
        ));
    }
    if audio.samples.iter().any(|sample| !sample.is_finite()) {
        return Err(SophonError::SynthesisFailed(
            "provider returned a non-finite audio sample".into(),
        ));
    }
    if audio.samples.len() as u128 > u128::from(audio.sample_rate) * u128::from(max_seconds) {
        return Err(SophonError::ResourceLimit(
            "generated audio exceeds duration limit".into(),
        ));
    }
    let frame_bytes = (audio.samples.len() as u64)
        .checked_mul(std::mem::size_of::<f32>() as u64)
        .ok_or_else(|| SophonError::ResourceLimit("generated audio is too large".into()))?;
    if frame_bytes > u64::from(u32::MAX) - 36 {
        return Err(SophonError::ResourceLimit(
            "generated WAV exceeds RIFF size limits".into(),
        ));
    }

    let mut cursor = Cursor::new(Vec::with_capacity(frame_bytes as usize + 44));
    {
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels: 1,
                sample_rate: audio.sample_rate,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
        )
        .map_err(|error| SophonError::OutputFailed(error.to_string()))?;
        for sample in &audio.samples {
            writer
                .write_sample(*sample)
                .map_err(|error| SophonError::OutputFailed(error.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|error| SophonError::OutputFailed(error.to_string()))?;
    }
    Ok(cursor.into_inner())
}

fn publish_exclusive_with<F>(path: &Path, write: F) -> Result<u64, SophonError>
where
    F: FnOnce(&mut File) -> std::io::Result<u64>,
{
    if !path.is_absolute() {
        return Err(SophonError::OutputFailed(
            "output path must be absolute".into(),
        ));
    }
    if path.exists() {
        return Err(SophonError::OutputExists(path.display().to_string()));
    }
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(SophonError::OutputExists(path.display().to_string()));
        }
        Err(error) => return Err(SophonError::OutputFailed(error.to_string())),
    };
    let result = write(&mut file).and_then(|length| {
        file.sync_all()?;
        Ok(length)
    });
    drop(file);
    match result {
        Ok(length) => Ok(length),
        Err(error) => {
            let _ = std::fs::remove_file(path);
            Err(SophonError::OutputFailed(error.to_string()))
        }
    }
}

pub fn publish_exclusive(path: &Path, wav: &[u8]) -> Result<u64, SophonError> {
    publish_exclusive_with(path, |file| {
        file.write_all(wav)?;
        Ok(wav.len() as u64)
    })
}

pub fn sealed_memfd(wav: &[u8]) -> Result<(OwnedFd, u64), SophonError> {
    let memfd = MemfdOptions::default()
        .allow_sealing(true)
        .create("sophon-tts.wav")
        .map_err(|error| SophonError::OutputFailed(error.to_string()))?;
    let mut file = memfd.as_file();
    file.write_all(wav)
        .and_then(|()| file.sync_all())
        .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
        .map_err(|error| SophonError::OutputFailed(error.to_string()))?;
    memfd
        .add_seals(&[
            FileSeal::SealWrite,
            FileSeal::SealGrow,
            FileSeal::SealShrink,
            FileSeal::SealSeal,
        ])
        .map_err(|error| SophonError::OutputFailed(error.to_string()))?;
    let file = memfd.into_file();
    Ok((file.into(), wav.len() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn wav(channels: u16, rate: u32, bits: u16, samples: usize) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: bits,
            sample_format: SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for _ in 0..samples * channels as usize {
            writer.write_sample(0_i32).unwrap();
        }
        writer.finalize().unwrap();
        cursor.into_inner()
    }

    fn int_wav(channels: u16, rate: u32, bits: u16, samples: &[i32]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: bits,
                sample_format: SampleFormat::Int,
            },
        )
        .unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
        cursor.into_inner()
    }

    fn float_wav(channels: u16, rate: u32, samples: &[f32]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut cursor,
            hound::WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
        )
        .unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
        cursor.into_inner()
    }

    #[test]
    fn accepts_canonical_wav_and_enforces_size_and_duration() {
        let valid = wav(1, 16_000, 16, 2);
        assert_eq!(
            parse_wav(Cursor::new(valid.clone()), 100, 1)
                .unwrap()
                .samples
                .len(),
            2
        );
        assert!(matches!(
            parse_wav(Cursor::new(valid.clone()), 1, 1),
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(matches!(
            parse_wav(Cursor::new(wav(1, 16_000, 16, 16_001)), 100_000, 1),
            Err(SophonError::ResourceLimit(_))
        ));
    }

    #[test]
    fn decodes_supported_pcm_to_interleaved_float_with_source_metadata() {
        for bits in [8, 16, 24, 32] {
            let decoded = parse_wav(Cursor::new(wav(2, 8_000, bits, 2)), 100_000, 1).unwrap();
            assert_eq!(decoded.source_rate, 8_000);
            assert_eq!(decoded.channels, 2);
            assert_eq!(decoded.samples, vec![0.0; 4]);
        }
        let decoded = parse_wav(
            Cursor::new(float_wav(2, 22_050, &[0.25, -0.5, 0.75, -1.0])),
            100_000,
            1,
        )
        .unwrap();
        assert_eq!(decoded.source_rate, 22_050);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.samples, [0.25, -0.5, 0.75, -1.0]);

        assert!(matches!(
            parse_wav(Cursor::new(b"not a wav"), 100_000, 1),
            Err(SophonError::InvalidAudio(_))
        ));
    }

    #[test]
    fn normalizes_integer_depths_and_rejects_malformed_nonfinite_or_incomplete_wavs() {
        for (bits, minimum, half) in [
            (8, -128, 64),
            (16, -32_768, 16_384),
            (24, -8_388_608, 4_194_304),
            (32, i32::MIN, 1_073_741_824),
        ] {
            let decoded = parse_wav(
                Cursor::new(int_wav(1, 16_000, bits, &[minimum, half])),
                1_000,
                1,
            )
            .unwrap();
            assert!((decoded.samples[0] + 1.0).abs() < f32::EPSILON);
            assert!((decoded.samples[1] - 0.5).abs() < f32::EPSILON);
        }

        let mut zero_channels = wav(1, 16_000, 16, 1);
        zero_channels[22..24].copy_from_slice(&0_u16.to_le_bytes());
        let mut zero_rate = wav(1, 16_000, 16, 1);
        zero_rate[24..28].copy_from_slice(&0_u32.to_le_bytes());
        let mut incomplete = int_wav(2, 16_000, 16, &[0, 0]);
        incomplete.truncate(incomplete.len() - 2);
        let riff_len = (incomplete.len() as u32 - 8).to_le_bytes();
        incomplete[4..8].copy_from_slice(&riff_len);
        incomplete[40..44].copy_from_slice(&2_u32.to_le_bytes());
        for input in [
            zero_channels,
            zero_rate,
            incomplete,
            float_wav(1, 16_000, &[f32::NAN]),
            b"not a wav".to_vec(),
        ] {
            assert!(matches!(
                parse_wav(Cursor::new(input), 100_000, 1),
                Err(SophonError::InvalidAudio(_))
            ));
        }
    }

    #[test]
    fn downmixes_frames_by_arithmetic_mean_and_rejects_invalid_pcm() {
        let mono = downmix_wav(DecodedWav {
            samples: vec![1.0, -1.0, 0.75, 0.25],
            source_rate: 48_000,
            channels: 2,
        })
        .unwrap();
        assert_eq!(mono.sample_rate, 48_000);
        assert_eq!(mono.samples, [0.0, 0.5]);

        for invalid in [
            DecodedWav {
                samples: vec![0.0],
                source_rate: 48_000,
                channels: 0,
            },
            DecodedWav {
                samples: vec![0.0],
                source_rate: 0,
                channels: 1,
            },
            DecodedWav {
                samples: vec![0.0],
                source_rate: 48_000,
                channels: 2,
            },
            DecodedWav {
                samples: vec![f32::NAN],
                source_rate: 48_000,
                channels: 1,
            },
        ] {
            assert!(matches!(
                downmix_wav(invalid),
                Err(SophonError::InvalidAudio(_))
            ));
        }
    }

    #[test]
    fn resamples_whole_clips_and_bypasses_equal_rates_exactly() {
        let original = OwnedAudio {
            samples: (0..800).map(|index| (index as f32 * 0.01).sin()).collect(),
            sample_rate: 8_000,
        };
        assert_eq!(resample_mono(original.clone(), 8_000).unwrap(), original);

        let upsampled = resample_mono(original.clone(), 16_000).unwrap();
        assert_eq!(upsampled.sample_rate, 16_000);
        assert_eq!(upsampled.samples.len(), 1_600);
        assert!(upsampled.samples.iter().all(|sample| sample.is_finite()));

        let downsampled = resample_mono(original, 4_000).unwrap();
        assert_eq!(downsampled.sample_rate, 4_000);
        assert_eq!(downsampled.samples.len(), 400);
        assert!(downsampled.samples.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn file_ingestion_requires_an_absolute_regular_file_and_fd_starts_at_zero() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), wav(1, 16_000, 16, 1)).unwrap();
        assert!(matches!(
            read_file(Path::new("relative.wav"), 100_000, 1),
            Err(SophonError::InvalidAudio(_))
        ));
        assert_eq!(read_file(file.path(), 100_000, 1).unwrap().samples.len(), 1);
        let fd: OwnedFd = File::open(file.path()).unwrap().into();
        assert_eq!(read_unix_fd(fd, 100_000, 1).unwrap().samples.len(), 1);
    }

    #[test]
    fn clone_wav_requires_canonical_float_format_and_enforces_both_limits() {
        let valid = float_wav(1, 24_000, &[0.25, -0.5]);
        let decoded = decode_clone_wav(Cursor::new(valid.clone()), 1024, 1).unwrap();
        assert_eq!(decoded.sample_rate, 24_000);
        assert_eq!(decoded.samples, [0.25, -0.5]);
        assert!(matches!(
            decode_clone_wav(Cursor::new(valid.clone()), 1, 1),
            Err(SophonError::ResourceLimit(_))
        ));
        assert!(matches!(
            decode_clone_wav(
                Cursor::new(float_wav(1, 24_000, &vec![0.0; 24_001])),
                1_000_000,
                1
            ),
            Err(SophonError::ResourceLimit(_))
        ));

        let mut wrong_encoding = valid.clone();
        wrong_encoding[20] = 1;
        wrong_encoding[21] = 0;
        let inputs = [
            float_wav(2, 24_000, &[0.0, 0.0]),
            float_wav(1, 16_000, &[0.0]),
            wrong_encoding,
            b"not a wav".to_vec(),
            valid[..valid.len() - 1].to_vec(),
        ];
        for input in inputs {
            assert!(matches!(
                decode_clone_wav(Cursor::new(input), 1_000_000, 60),
                Err(SophonError::InvalidReferenceAudio(_))
            ));
        }
    }

    #[test]
    fn float_wav_encoding_is_complete_and_rejects_invalid_provider_audio() {
        let audio = OwnedAudio {
            samples: vec![0.25, -0.5],
            sample_rate: 24_000,
        };
        let encoded = encode_float_wav(&audio, 1).unwrap();
        let mut reader = WavReader::new(Cursor::new(encoded)).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 24_000);
        assert_eq!(reader.spec().bits_per_sample, 32);
        assert_eq!(reader.spec().sample_format, SampleFormat::Float);
        assert_eq!(
            reader
                .samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            audio.samples
        );
        for invalid in [
            OwnedAudio {
                samples: vec![],
                sample_rate: 24_000,
            },
            OwnedAudio {
                samples: vec![f32::NAN],
                sample_rate: 24_000,
            },
            OwnedAudio {
                samples: vec![0.0],
                sample_rate: 0,
            },
        ] {
            assert!(matches!(
                encode_float_wav(&invalid, 1),
                Err(SophonError::SynthesisFailed(_))
            ));
        }
        assert!(matches!(
            encode_float_wav(
                &OwnedAudio {
                    samples: vec![0.0; 11],
                    sample_rate: 10,
                },
                1
            ),
            Err(SophonError::ResourceLimit(_))
        ));
    }

    #[test]
    fn exclusive_publication_never_replaces_and_cleans_failed_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("speech.wav");
        assert_eq!(publish_exclusive(&path, b"complete").unwrap(), 8);
        assert!(matches!(
            publish_exclusive(&path, b"replacement"),
            Err(SophonError::OutputExists(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"complete");

        let partial = directory.path().join("partial.wav");
        let result = publish_exclusive_with(&partial, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("fixture write failure"))
        });
        assert!(matches!(result, Err(SophonError::OutputFailed(_))));
        assert!(!partial.exists());
    }

    #[test]
    fn memfd_is_complete_rewound_sealed_and_lives_with_client_descriptor() {
        let wav = float_wav(1, 24_000, &[0.25, -0.5]);
        let (fd, length) = sealed_memfd(&wav).unwrap();
        assert_eq!(length, wav.len() as u64);
        let mut file = File::from(fd);
        assert_eq!(file.stream_position().unwrap(), 0);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, wav);
        assert!(file.write_all(b"x").is_err());
        assert!(file.set_len(0).is_err());
        let memfd = memfd::Memfd::try_from_file(file).unwrap();
        let seals = memfd.seals().unwrap();
        assert!(seals.contains(&FileSeal::SealWrite));
        assert!(seals.contains(&FileSeal::SealGrow));
        assert!(seals.contains(&FileSeal::SealShrink));
        assert!(seals.contains(&FileSeal::SealSeal));

        let (server_fd, _) = sealed_memfd(&wav).unwrap();
        let server_file = File::from(server_fd);
        let mut client_file = server_file.try_clone().unwrap();
        drop(server_file);
        client_file.seek(SeekFrom::Start(0)).unwrap();
        let mut client_bytes = Vec::new();
        client_file.read_to_end(&mut client_bytes).unwrap();
        assert_eq!(client_bytes, wav);
    }
}
