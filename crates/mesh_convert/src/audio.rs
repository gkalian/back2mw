//! Sounds: Oblivion WAV → a WAV that the Morrowind engine plays in 3D (T10.1).
//!
//! All 1,533 files of `Oblivion - Sounds.bsa` are 16-bit PCM, like the 709 files of Morrowind's
//! `Sound\` (`ffprobe`, 2026-09-28), so there is nothing to re-encode. There is one difference:
//! Oblivion has 170 stereo files, while vanilla has one stereo file out of 709. Positional sound is
//! mono (a stereo buffer cannot be placed in space), so stereo is mixed down to mono by averaging
//! the channels; the sample rate does not change. Mono is copied as is, byte for byte.

use std::fmt;

pub mod mp3;

/// `WAVE_FORMAT_PCM`.
const FORMAT_PCM: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// No `RIFF`…`WAVE` header.
    NotWav,
    /// A chunk runs past the end of the file, or there is no `fmt `/`data`.
    Truncated { chunk: [u8; 4] },
    /// Not 16-bit PCM mono or stereo: the file is copied as is, there is nothing to mix it with.
    Unsupported {
        format: u16,
        channels: u16,
        bits: u16,
    },
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWav => lang::trw!(f, "не WAV", "not a WAV"),
            Self::Truncated { chunk } => {
                lang::trw!(
                    f,
                    "WAV обрезан в чанке {:?}",
                    "WAV truncated in chunk {:?}",
                    String::from_utf8_lossy(chunk)
                )
            }
            Self::Unsupported {
                format,
                channels,
                bits,
            } => lang::trw!(
                f,
                "WAV не поддержан: формат {format}, каналов {channels}, {bits} бит",
                "unsupported WAV: format {format}, {channels} channels, {bits} bit"
            ),
        }
    }
}

impl std::error::Error for AudioError {}

/// What `fmt ` says and where the samples are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub format: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Chunk parsing: `fmt ` and the `data` samples. Other chunks (`LIST`, `cue `) are skipped.
pub fn read_wav(bytes: &[u8]) -> Result<(WavInfo, &[u8]), AudioError> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AudioError::NotWav);
    }
    let (mut info, mut data) = (None, None);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let tag: [u8; 4] = bytes[at..at + 4]
            .try_into()
            .expect("the slice is exactly 4 bytes");
        let len = usize::try_from(u32_at(bytes, at + 4)).unwrap_or(usize::MAX);
        let body = at + 8;
        let end = body
            .checked_add(len)
            .filter(|&end| end <= bytes.len())
            .ok_or(AudioError::Truncated { chunk: tag })?;
        match &tag {
            b"fmt " if len >= 16 => {
                info = Some(WavInfo {
                    format: u16_at(bytes, body),
                    channels: u16_at(bytes, body + 2),
                    sample_rate: u32_at(bytes, body + 4),
                    bits: u16_at(bytes, body + 14),
                });
            }
            b"data" => data = Some(&bytes[body..end]),
            _ => {}
        }
        // Chunks are aligned to an even byte.
        at = end + (len & 1);
    }
    match (info, data) {
        (Some(info), Some(data)) => Ok((info, data)),
        (None, _) => Err(AudioError::Truncated { chunk: *b"fmt " }),
        (Some(_), None) => Err(AudioError::Truncated { chunk: *b"data" }),
    }
}

/// Stereo 16-bit PCM → mono (a new file); mono gives `None` (copy as is).
pub fn to_mono(bytes: &[u8]) -> Result<Option<Vec<u8>>, AudioError> {
    let (info, data) = read_wav(bytes)?;
    let unsupported = AudioError::Unsupported {
        format: info.format,
        channels: info.channels,
        bits: info.bits,
    };
    if info.format != FORMAT_PCM || info.bits != 16 {
        return Err(unsupported);
    }
    match info.channels {
        1 => Ok(None),
        2 => {
            let mut samples = Vec::with_capacity(data.len() / 2);
            for frame in data.chunks_exact(4) {
                let left = i32::from(i16::from_le_bytes([frame[0], frame[1]]));
                let right = i32::from(i16::from_le_bytes([frame[2], frame[3]]));
                let mixed = i16::try_from((left + right) / 2)
                    .expect("the average of two i16 fits into i16");
                samples.extend(mixed.to_le_bytes());
            }
            Ok(Some(write_wav(info.sample_rate, 1, &samples)))
        }
        _ => Err(unsupported),
    }
}

/// 16-bit PCM WAV from samples: `RIFF`, a 16-byte `fmt `, `data`.
pub fn write_wav(sample_rate: u32, channels: u16, samples: &[u8]) -> Vec<u8> {
    let block_align = channels * 2;
    let data_len = u32::try_from(samples.len()).expect("a sound is smaller than 4 GB");
    let mut out = Vec::with_capacity(44 + samples.len());
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes());
    out.extend(b"WAVE");
    out.extend(b"fmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(FORMAT_PCM.to_le_bytes());
    out.extend(channels.to_le_bytes());
    out.extend(sample_rate.to_le_bytes());
    out.extend((sample_rate * u32::from(block_align)).to_le_bytes());
    out.extend(block_align.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    out.extend(samples);
    if samples.len() % 2 == 1 {
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(values: &[i16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn stereo_becomes_the_mean_of_both_channels() {
        let stereo = write_wav(44_100, 2, &pcm(&[100, 300, -32768, -32768, 32767, -32768]));
        let mono = to_mono(&stereo).unwrap().expect("stereo is mixed down");
        let (info, data) = read_wav(&mono).unwrap();
        assert_eq!(
            (info.channels, info.sample_rate, info.bits),
            (1, 44_100, 16)
        );
        assert_eq!(data, pcm(&[200, -32768, 0]).as_slice());
    }

    #[test]
    fn mono_is_left_alone() {
        let mono = write_wav(22_050, 1, &pcm(&[1, 2, 3]));
        assert_eq!(to_mono(&mono).unwrap(), None);
    }

    #[test]
    fn extra_chunks_are_skipped_and_odd_chunks_are_padded() {
        let mut wav = write_wav(11_025, 2, &pcm(&[10, 20]));
        // An odd-length `LIST` between `fmt ` and `data` — with a padding byte inserted.
        let data_at = wav.windows(4).position(|w| w == b"data").unwrap();
        let list = [b"LIST".as_slice(), &3u32.to_le_bytes(), b"abc", &[0]].concat();
        wav.splice(data_at..data_at, list);
        let mono = to_mono(&wav).unwrap().unwrap();
        assert_eq!(read_wav(&mono).unwrap().1, pcm(&[15]).as_slice());
    }

    #[test]
    fn broken_and_unsupported_files_are_errors() {
        assert_eq!(to_mono(b"OggS...."), Err(AudioError::NotWav));
        let mut cut = write_wav(44_100, 1, &pcm(&[1, 2, 3, 4]));
        cut.truncate(cut.len() - 2);
        assert_eq!(
            to_mono(&cut),
            Err(AudioError::Truncated { chunk: *b"data" })
        );
        let mut adpcm = write_wav(44_100, 2, &pcm(&[1, 2]));
        adpcm[20] = 2; // format 2 is MS ADPCM
        assert!(matches!(
            to_mono(&adpcm),
            Err(AudioError::Unsupported { format: 2, .. })
        ));
    }
}
