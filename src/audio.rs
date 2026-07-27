//! WAV parsing plus file and Unix-file-descriptor ingestion.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::fd::OwnedFd,
    path::Path,
};

use hound::{SampleFormat, WavReader};

use crate::domain::SophonError;

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
}
