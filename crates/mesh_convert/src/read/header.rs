//! Заголовок NIF 20.0.0.4/20.0.0.5 и 10.2.0.0. Раскладка — в `docs/ru/formats/nif-oblivion.md`,
//! подтверждена чтением всех 7 382 файлов этих версий из `Oblivion - Meshes.bsa`.

use super::cursor::{Cursor, latin1};
use super::error::{Error, Result};

pub const VERSION_20_0_0_4: u32 = 0x1400_0004;
pub const VERSION_20_0_0_5: u32 = 0x1400_0005;
/// 10.2.0.0 (T2.3): 490 мешей Oblivion, `BSVER` 6…11. Отличия от 20.0.0.4 в читаемых блоках —
/// `docs/ru/formats/nif-oblivion.md`, «NIF 10.2.0.0».
pub const VERSION_10_2_0_0: u32 = 0x0A02_0000;
/// Старые 10.x (T2.3b): 97 мешей моделей `Oblivion.esm` — фонари шахт, трава, часть статики.
/// Отличия в заголовке — у [`parse`], в блоках — `docs/ru/formats/nif-oblivion.md`, «Старые 10.x».
pub const VERSION_10_0_1_0: u32 = 0x0A00_0100;
pub const VERSION_10_0_1_2: u32 = 0x0A00_0102;
pub const VERSION_10_1_0_101: u32 = 0x0A01_0065;
pub const VERSION_10_1_0_106: u32 = 0x0A01_006A;

/// Версии, которые читает ридер.
pub const SUPPORTED_VERSIONS: [u32; 7] = [
    VERSION_10_0_1_0,
    VERSION_10_0_1_2,
    VERSION_10_1_0_101,
    VERSION_10_1_0_106,
    VERSION_10_2_0_0,
    VERSION_20_0_0_4,
    VERSION_20_0_0_5,
];

/// Разобранный заголовок. Сами блоки лежат сразу за ним, с [`NifHeader::body_offset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NifHeader {
    /// Строка заголовка без `\n`, например `Gamebryo File Format, Version 20.0.0.4`.
    pub line: String,
    pub version: u32,
    pub user_version: u32,
    pub user_version_2: u32,
    /// Три строки экспортной информации: автор, скрипт обработки, скрипт экспорта.
    pub export_info: [String; 3],
    /// Имена типов блоков — таблица, на которую ссылаются индексы.
    pub block_types: Vec<String>,
    /// Индекс в `block_types` для каждого блока, в порядке файла.
    pub block_type_index: Vec<u16>,
    /// Группы блоков. У всех проверенных файлов пусты; читаются, чтобы курсор встал
    /// на начало блоков при любом значении.
    pub groups: Vec<u32>,
    /// Смещение первого блока от начала файла.
    pub body_offset: usize,
}

impl NifHeader {
    /// Имя типа блока по его номеру в файле.
    pub fn block_type_name(&self, block: usize) -> Option<&str> {
        let index = *self.block_type_index.get(block)?;
        self.block_types.get(usize::from(index)).map(String::as_str)
    }

    pub fn num_blocks(&self) -> usize {
        self.block_type_index.len()
    }
}

/// Разбирает только заголовок. Версия не из [`SUPPORTED_VERSIONS`] — ошибка
/// [`Error::UnsupportedVersion`].
pub fn parse(bytes: &[u8]) -> Result<NifHeader> {
    let mut cur = Cursor::new(bytes);

    let line_bytes = cur.line().map_err(|_| Error::NotANif {
        found: preview(bytes),
    })?;
    let line = latin1(line_bytes);
    if !(line.starts_with("Gamebryo File Format") || line.starts_with("NetImmerse File Format")) {
        return Err(Error::NotANif {
            found: preview(bytes),
        });
    }

    let version = cur.u32()?;
    if !SUPPORTED_VERSIONS.contains(&version) {
        return Err(Error::UnsupportedVersion { version });
    }

    // Байт порядка байтов — с 20.0.0.3; у 10.2.0.0 его нет.
    if version >= VERSION_20_0_0_4 {
        let endian = cur.u8()?;
        if endian != 1 {
            return Err(Error::UnsupportedEndian { flag: endian });
        }
    }

    // Пользовательская версия — с 10.0.1.8: у 10.0.1.0 и 10.0.1.2 её нет.
    let user_version = if version >= 0x0A00_0108 {
        cur.u32()?
    } else {
        0
    };
    let num_blocks = cur.len_u32()?;
    // Заголовок Bethesda (`BSVER` и три строки, `#BSSTREAMHEADER#`): у 10.0.1.2 — всегда (у
    // всех 23 файлов `BSVER` 3), у 10.1.0.0…20.0.0.4 — при пользовательской версии 3…11 (у
    // всех файлов Oblivion она 10, 2026-09-26), у 10.0.1.0 — никогда.
    let bs_header = match version {
        VERSION_10_0_1_0 => false,
        VERSION_10_0_1_2 | VERSION_20_0_0_5 => true,
        _ => (3..=11).contains(&user_version),
    };
    if !bs_header && version != VERSION_10_0_1_0 {
        return Err(Error::UnsupportedVersion { version });
    }
    let (user_version_2, export_info) = if bs_header {
        let bsver = cur.u32()?;
        let info = [
            cur.short_string()?,
            cur.short_string()?,
            cur.short_string()?,
        ];
        (bsver, info)
    } else {
        (0, Default::default())
    };

    let num_types = usize::from(cur.u16()?);
    let mut block_types = Vec::with_capacity(num_types);
    for _ in 0..num_types {
        block_types.push(cur.sized_string()?);
    }

    // Не резервируем `num_blocks` из файла вслепую: заявленное число больше того,
    // что физически помещается, — это усечённый или битый файл.
    if cur.remaining() / 2 < num_blocks {
        return Err(Error::Truncated {
            at: cur.pos(),
            need: num_blocks.saturating_mul(2),
            have: cur.remaining(),
        });
    }
    let mut block_type_index = Vec::with_capacity(num_blocks);
    for block in 0..num_blocks {
        let index = cur.u16()?;
        if usize::from(index) >= block_types.len() {
            return Err(Error::BadBlockTypeIndex {
                block,
                index,
                types: block_types.len(),
            });
        }
        block_type_index.push(index);
    }

    let num_groups = cur.len_u32()?;
    if cur.remaining() / 4 < num_groups {
        return Err(Error::Truncated {
            at: cur.pos(),
            need: num_groups.saturating_mul(4),
            have: cur.remaining(),
        });
    }
    let mut groups = Vec::with_capacity(num_groups);
    for _ in 0..num_groups {
        groups.push(cur.u32()?);
    }

    Ok(NifHeader {
        line,
        version,
        user_version,
        user_version_2,
        export_info,
        block_types,
        block_type_index,
        groups,
        body_offset: cur.pos(),
    })
}

fn preview(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(24)
        .map(|&b| {
            if b.is_ascii_graphic() {
                char::from(b)
            } else {
                '.'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE_4: &[u8] = b"Gamebryo File Format, Version 20.0.0.4\n";
    const LINE_5: &[u8] = b"Gamebryo File Format, Version 20.0.0.5\n";

    fn le32(n: usize) -> [u8; 4] {
        u32::try_from(n).unwrap().to_le_bytes()
    }

    /// Собирает заголовок руками, по раскладке из `nif-oblivion.md`.
    fn build(version: u32, types: &[&str], index: &[u16], groups: &[u32]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(if version == VERSION_20_0_0_4 {
            LINE_4
        } else {
            LINE_5
        });
        b.extend_from_slice(&version.to_le_bytes());
        b.push(1);
        b.extend_from_slice(&11u32.to_le_bytes());
        b.extend_from_slice(&le32(index.len()));
        b.extend_from_slice(&11u32.to_le_bytes());
        for s in ["author", "proc", "export"] {
            b.push(u8::try_from(s.len()).unwrap());
            b.extend_from_slice(s.as_bytes());
        }
        b.extend_from_slice(&u16::try_from(types.len()).unwrap().to_le_bytes());
        for t in types {
            b.extend_from_slice(&le32(t.len()));
            b.extend_from_slice(t.as_bytes());
        }
        for i in index {
            b.extend_from_slice(&i.to_le_bytes());
        }
        b.extend_from_slice(&le32(groups.len()));
        for g in groups {
            b.extend_from_slice(&g.to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_both_supported_versions() {
        for version in [VERSION_20_0_0_4, VERSION_20_0_0_5] {
            let bytes = build(version, &["NiNode", "NiTriStrips"], &[0, 1, 1], &[]);
            let header = parse(&bytes).unwrap();
            assert_eq!(header.version, version);
            assert_eq!(header.user_version, 11);
            assert_eq!(header.user_version_2, 11);
            assert_eq!(header.export_info, ["author", "proc", "export"]);
            assert_eq!(header.num_blocks(), 3);
            assert_eq!(header.block_type_name(0), Some("NiNode"));
            assert_eq!(header.block_type_name(2), Some("NiTriStrips"));
            assert_eq!(header.block_type_name(3), None);
            assert_eq!(header.body_offset, bytes.len());
        }
    }

    #[test]
    fn body_offset_points_past_groups_and_at_the_first_block() {
        let mut bytes = build(VERSION_20_0_0_4, &["NiNode"], &[0], &[7, 9]);
        let header_len = bytes.len();
        bytes.extend_from_slice(b"BLOCK");
        let header = parse(&bytes).unwrap();
        assert_eq!(header.groups, [7, 9]);
        assert_eq!(header.body_offset, header_len);
    }

    #[test]
    fn refuses_other_10x_with_its_version() {
        let mut bytes = b"Gamebryo File Format, Version 10.1.0.0\n".to_vec();
        bytes.extend_from_slice(&0x0A01_0000u32.to_le_bytes());
        assert_eq!(
            parse(&bytes),
            Err(Error::UnsupportedVersion {
                version: 0x0A01_0000
            })
        );
    }

    /// Старые 10.x (T2.3b): версия, [пользовательская версия с 10.0.1.8], число блоков,
    /// [заголовок Bethesda], одна таблица типов на один блок.
    fn build_old(version: u32, user: Option<u32>, bsver: Option<u32>) -> Vec<u8> {
        let mut b = b"NetImmerse File Format, Version 10.0.1.x\n".to_vec();
        b.extend_from_slice(&version.to_le_bytes());
        if let Some(user) = user {
            b.extend_from_slice(&user.to_le_bytes());
        }
        b.extend_from_slice(&le32(1));
        if let Some(bsver) = bsver {
            b.extend_from_slice(&bsver.to_le_bytes());
            for s in ["a", "b", "c"] {
                b.push(1);
                b.extend_from_slice(s.as_bytes());
            }
        }
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&le32(6));
        b.extend_from_slice(b"NiNode");
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&le32(0));
        b
    }

    #[test]
    fn old_10x_headers_differ_in_user_version_and_bethesda_header() {
        let cases = [
            (VERSION_10_0_1_0, None, None, 0, 0),
            (VERSION_10_0_1_2, None, Some(3), 0, 3),
            (VERSION_10_1_0_101, Some(10), Some(4), 10, 4),
            (VERSION_10_1_0_106, Some(10), Some(5), 10, 5),
        ];
        for (version, user, bsver, want_user, want_bsver) in cases {
            let bytes = build_old(version, user, bsver);
            let header = parse(&bytes).unwrap();
            assert_eq!(
                (header.version, header.user_version, header.user_version_2),
                (version, want_user, want_bsver)
            );
            assert_eq!(header.block_type_name(0), Some("NiNode"));
            assert_eq!(header.body_offset, bytes.len());
        }
    }

    /// 10.2.0.0: байта порядка байтов нет, `BSVER` из заголовка Bethesda — свой (здесь 9).
    #[test]
    fn parses_10_2_without_the_endian_byte() {
        let mut b = b"Gamebryo File Format, Version 10.2.0.0\n".to_vec();
        b.extend_from_slice(&VERSION_10_2_0_0.to_le_bytes());
        b.extend_from_slice(&10u32.to_le_bytes());
        b.extend_from_slice(&le32(1));
        b.extend_from_slice(&9u32.to_le_bytes());
        for s in ["a", "b", "c"] {
            b.push(1);
            b.extend_from_slice(s.as_bytes());
        }
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&le32(6));
        b.extend_from_slice(b"NiNode");
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&le32(0));
        let header = parse(&b).unwrap();
        assert_eq!(
            (header.version, header.user_version, header.user_version_2),
            (VERSION_10_2_0_0, 10, 9)
        );
        assert_eq!(header.body_offset, b.len());

        // Пользовательская версия вне 3…11 — заголовка Bethesda нет, такой файл не наш.
        let mut other = b"Gamebryo File Format, Version 10.2.0.0\n".to_vec();
        other.extend_from_slice(&VERSION_10_2_0_0.to_le_bytes());
        other.extend_from_slice(&0u32.to_le_bytes());
        other.extend_from_slice(&le32(1));
        assert!(matches!(
            parse(&other),
            Err(Error::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn refuses_a_file_that_is_not_a_nif() {
        assert!(matches!(
            parse(b"BSA\0 not a nif at all\n....."),
            Err(Error::NotANif { .. })
        ));
        assert!(matches!(parse(b"no newline"), Err(Error::NotANif { .. })));
    }

    #[test]
    fn refuses_big_endian() {
        let mut bytes = build(VERSION_20_0_0_4, &["NiNode"], &[0], &[]);
        bytes[LINE_4.len() + 4] = 0;
        assert_eq!(parse(&bytes), Err(Error::UnsupportedEndian { flag: 0 }));
    }

    #[test]
    fn rejects_a_block_type_index_outside_the_table() {
        let bytes = build(VERSION_20_0_0_4, &["NiNode"], &[0, 5], &[]);
        assert_eq!(
            parse(&bytes),
            Err(Error::BadBlockTypeIndex {
                block: 1,
                index: 5,
                types: 1
            })
        );
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let bytes = build(VERSION_20_0_0_5, &["NiNode", "NiTriStrips"], &[0, 1], &[3]);
        for cut in 0..bytes.len() {
            assert!(
                parse(&bytes[..cut]).is_err(),
                "усечение до {cut} байт прошло"
            );
        }
        assert!(parse(&bytes).is_ok());
    }

    #[test]
    fn a_huge_declared_block_count_is_an_error() {
        let mut bytes = build(VERSION_20_0_0_4, &["NiNode"], &[0], &[]);
        // num_blocks лежит после строки, версии, флага порядка байтов и user_version
        let at = LINE_4.len() + 4 + 1 + 4;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(parse(&bytes), Err(Error::Truncated { .. })));
    }
}
