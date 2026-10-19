//! Голоса: склейка mp3 Oblivion без перекодирования (T10.8, решение 4 фазы 10).
//!
//! Все 48 446 mp3 трёх архивов голосов Oblivion (`Voices1`, `Voices2`, `DLCShiveringIsles -
//! Voices`) — MPEG-1 Layer III, 64 кбит/с, моно, 44 100 Гц в каждом кадре (замер 2026-09-30, разовый
//! тест по архивам); у Morrowind (`Sound\Vo`) тот же формат. У 14 файлов есть ID3v2 и ID3v1, служебного
//! кадра Xing/Info нет ни у одного; 11 файлов тестового квеста `testtoddquest` кончаются мусором —
//! для них ошибка.
//!
//! Ответ реплики Oblivion — отдельный файл, реплика Morrowind — один файл на всю реплику, поэтому
//! кадры ответов идут подряд. Склейка кадров корректна: первый кадр каждого файла не берёт данных
//! из «резервуара» предыдущего (`main_data_begin` = 0 у первого кадра потока), а у CBR один формат
//! на весь файл, поэтому заголовок файла, кроме тегов, не нужен.

use std::fmt;

/// Формат, который склеивается: всё прочее — ошибка (карточка T10.8, «Стоп»).
pub const BITRATE_KBPS: u32 = 64;
pub const SAMPLE_RATE: u32 = 44_100;

/// Битрейт MPEG-1 Layer III по индексу заголовка, кбит/с (0 — «свободный», 15 — запрещён).
const BITRATES: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];
/// Частота MPEG-1 по индексу.
const SAMPLE_RATES: [u32; 4] = [44_100, 48_000, 32_000, 0];
/// Моно у MPEG-1: побочная информация 17 байт, за ней у служебного кадра — `Xing`/`Info`.
const SIDE_INFO_MONO: usize = 17;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mp3Error {
    /// Ни одного кадра.
    Empty,
    /// На месте кадра — не заголовок MPEG-1 Layer III (смещение от начала файла).
    NotFrame { offset: usize },
    /// Кадр другого формата: склейка без перекодирования дала бы неверный поток.
    Format {
        offset: usize,
        bitrate_kbps: u32,
        sample_rate: u32,
        mono: bool,
    },
    /// Последний кадр обрезан.
    Truncated { offset: usize },
}

impl fmt::Display for Mp3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => lang::trw!(f, "в mp3 нет кадров", "the mp3 has no frames"),
            Self::NotFrame { offset } => lang::trw!(
                f,
                "mp3: нет кадра MPEG-1 Layer III на {offset}",
                "mp3: no MPEG-1 Layer III frame at {offset}"
            ),
            Self::Format {
                offset,
                bitrate_kbps,
                sample_rate,
                mono,
            } => lang::trw!(
                f,
                "mp3: кадр на {offset} — {bitrate_kbps} кбит/с {sample_rate} Гц {}, склеивается только {BITRATE_KBPS} кбит/с {SAMPLE_RATE} Гц моно",
                "mp3: frame at {offset} is {bitrate_kbps} kbit/s {sample_rate} Hz {}, only {BITRATE_KBPS} kbit/s {SAMPLE_RATE} Hz mono can be joined",
                if *mono {
                    lang::tr("моно", "mono")
                } else {
                    lang::tr("стерео", "stereo")
                }
            ),
            Self::Truncated { offset } => lang::trw!(
                f,
                "mp3: кадр на {offset} обрезан",
                "mp3: frame at {offset} is truncated"
            ),
        }
    }
}

impl std::error::Error for Mp3Error {}

/// Длина ID3v2 в начале (с заголовком и подвалом), 0 — тега нет.
fn id3v2_len(b: &[u8]) -> usize {
    if b.len() < 10 || &b[..3] != b"ID3" {
        return 0;
    }
    // Размер — 4 байта по 7 значащих бит.
    let size = b[6..10]
        .iter()
        .fold(0usize, |a, &x| (a << 7) | usize::from(x & 0x7F));
    let footer = if b[5] & 0x10 != 0 { 10 } else { 0 };
    10 + size + footer
}

/// Кадры голоса без тегов ID3v2/ID3v1 и служебного кадра Xing/Info. Каждый кадр проверяется: MPEG-1
/// Layer III, [`BITRATE_KBPS`], [`SAMPLE_RATE`], моно.
pub fn voice_frames(bytes: &[u8]) -> Result<&[u8], Mp3Error> {
    let start = id3v2_len(bytes).min(bytes.len());
    let end =
        if bytes.len() >= start + 128 && &bytes[bytes.len() - 128..bytes.len() - 125] == b"TAG" {
            bytes.len() - 128
        } else {
            bytes.len()
        };
    let mut p = start;
    let mut first = start;
    while p < end {
        let Some(header) = bytes.get(p..p + 4) else {
            return Err(Mp3Error::Truncated { offset: p });
        };
        let h = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        // Синхрослово 11 бит, версия 11 (MPEG-1), слой 01 (Layer III).
        if h >> 21 != 0x7FF || (h >> 19) & 3 != 3 || (h >> 17) & 3 != 1 {
            return Err(Mp3Error::NotFrame { offset: p });
        }
        let bitrate_kbps = BITRATES[((h >> 12) & 0xF) as usize];
        let sample_rate = SAMPLE_RATES[((h >> 10) & 3) as usize];
        let mono = (h >> 6) & 3 == 3;
        if bitrate_kbps != BITRATE_KBPS || sample_rate != SAMPLE_RATE || !mono {
            return Err(Mp3Error::Format {
                offset: p,
                bitrate_kbps,
                sample_rate,
                mono,
            });
        }
        let padding = ((h >> 9) & 1) as usize;
        let len = (144 * BITRATE_KBPS * 1000 / SAMPLE_RATE) as usize + padding;
        if p + len > end {
            return Err(Mp3Error::Truncated { offset: p });
        }
        let tag = p + 4 + SIDE_INFO_MONO;
        if p == start && matches!(bytes.get(tag..tag + 4), Some(b"Xing" | b"Info")) {
            first = p + len;
        }
        p += len;
    }
    if first >= end {
        return Err(Mp3Error::Empty);
    }
    Ok(&bytes[first..end])
}

/// Файлы ответов одной реплики — один файл: кадры подряд, без тегов.
pub fn join_voice<'a>(files: impl IntoIterator<Item = &'a [u8]>) -> Result<Vec<u8>, Mp3Error> {
    let mut out = Vec::new();
    for f in files {
        out.extend_from_slice(voice_frames(f)?);
    }
    if out.is_empty() {
        return Err(Mp3Error::Empty);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
