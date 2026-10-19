//! Декодирование Windows-1252 — кодировки строк в плагинах Bethesda.
//!
//! Отдельной зависимости это не стоит: от Latin-1 cp1252 отличается только
//! диапазоном 0x80..=0x9F, то есть таблицей из 32 символов. Всё остальное
//! совпадает с кодовыми точками Unicode один в один.

const HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

/// Декодирует байты как cp1252. Завершающие нули отбрасываются: строки в
/// плагинах хранятся с NUL, но он не часть значения.
pub fn decode_zstring(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    decode(&bytes[..end])
}

pub fn decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => HIGH[usize::from(b - 0x80)],
            other => char::from(other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passes_through() {
        assert_eq!(
            decode(b"Meshes\\Clutter\\Bowl01.nif"),
            "Meshes\\Clutter\\Bowl01.nif"
        );
    }

    #[test]
    fn high_range_is_cp1252_not_latin1() {
        // 0x92 — правая одинарная кавычка в cp1252, а не управляющий символ Latin-1.
        assert_eq!(decode(&[0x92]), "\u{2019}");
        // 0x80 — знак евро.
        assert_eq!(decode(&[0x80]), "\u{20AC}");
    }

    #[test]
    fn upper_range_matches_latin1() {
        assert_eq!(decode(&[0xE9]), "é");
    }

    #[test]
    fn zstring_stops_at_nul() {
        assert_eq!(decode_zstring(b"Bowl01\0\0junk"), "Bowl01");
        assert_eq!(decode_zstring(b"NoNul"), "NoNul");
    }
}
