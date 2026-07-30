//! WAV parsing plus file and Unix-file-descriptor ingestion.

use std::{
    fs::{File, OpenOptions},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    os::fd::OwnedFd,
    path::Path,
};

use hound::{SampleFormat, WavReader};
use memfd::{FileSeal, MemfdOptions};

use crate::error::SophonError;

#[derive(Debug, Clone, PartialEq)]
pub struct OwnedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub fn parse_wav<R: Read>(
    reader: R,
    max_bytes: u64,
    max_seconds: u64,
) -> Result<Vec<f32>, SophonError> {
    let mut reader =
        WavReader::new(reader).map_err(|e| SophonError::InvalidAudio(e.to_string()))?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != 16_000
        || spec.bits_per_sample != 16
        || spec.sample_format != SampleFormat::Int
    {
        return Err(SophonError::InvalidAudio(
            "expected mono 16 kHz signed 16-bit PCM WAV".into(),
        ));
    }
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| SophonError::InvalidAudio(e.to_string()))?;
    let bytes = samples.len() as u64 * 2;
    if bytes > max_bytes {
        return Err(SophonError::ResourceLimit(
            "encoded audio exceeds limit".into(),
        ));
    }
    if samples.len() as u64 > max_seconds * 16_000 {
        return Err(SophonError::ResourceLimit(
            "decoded audio exceeds duration limit".into(),
        ));
    }
    Ok(samples
        .into_iter()
        .map(|sample| sample as f32 / i16::MAX as f32)
        .collect())
}

pub fn read_file(path: &Path, max_bytes: u64, max_seconds: u64) -> Result<Vec<f32>, SophonError> {
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
) -> Result<Vec<f32>, SophonError> {
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
) -> Result<Vec<f32>, SophonError> {
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
            writer.write_sample(0_i16).unwrap();
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
            parse_wav(Cursor::new(valid.clone()), 100, 1).unwrap().len(),
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
    fn rejects_every_noncanonical_wav_property_and_malformed_content() {
        let mut unsupported_bits = wav(1, 16_000, 16, 1);
        unsupported_bits[34] = 8;
        unsupported_bits[35] = 0;
        for input in [
            wav(2, 16_000, 16, 1),
            wav(1, 8_000, 16, 1),
            unsupported_bits,
            b"not a wav".to_vec(),
        ] {
            assert!(matches!(
                parse_wav(Cursor::new(input), 100_000, 1),
                Err(SophonError::InvalidAudio(_))
            ));
        }
    }

    #[test]
    fn file_ingestion_requires_an_absolute_regular_file_and_fd_starts_at_zero() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), wav(1, 16_000, 16, 1)).unwrap();
        assert!(matches!(
            read_file(Path::new("relative.wav"), 100_000, 1),
            Err(SophonError::InvalidAudio(_))
        ));
        assert_eq!(read_file(file.path(), 100_000, 1).unwrap().len(), 1);
        let fd: OwnedFd = File::open(file.path()).unwrap().into();
        assert_eq!(read_unix_fd(fd, 100_000, 1).unwrap().len(), 1);
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
