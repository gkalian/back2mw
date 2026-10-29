//! NIF 20.0.0.4/20.0.0.5 and 10.2.0.0 header. The layout is in `docs/en/formats/nif-oblivion.md`,
//! confirmed by reading all 7,382 files of these versions from `Oblivion - Meshes.bsa`.

use super::cursor::{Cursor, latin1};
use super::error::{Error, Result};

pub const VERSION_20_0_0_4: u32 = 0x1400_0004;
pub const VERSION_20_0_0_5: u32 = 0x1400_0005;
/// 10.2.0.0 (T2.3): 490 Oblivion meshes, `BSVER` 6…11. Differences from 20.0.0.4 in the blocks we
/// read are in `docs/en/formats/nif-oblivion.md`, "NIF 10.2.0.0".
pub const VERSION_10_2_0_0: u32 = 0x0A02_0000;
/// Old 10.x (T2.3b): 97 meshes of `Oblivion.esm` models — mine lanterns, grass, some statics. The
/// header differences are in [`parse`], the block differences in `docs/en/formats/nif-oblivion.md`,
/// "Old 10.x".
pub const VERSION_10_0_1_0: u32 = 0x0A00_0100;
pub const VERSION_10_0_1_2: u32 = 0x0A00_0102;
pub const VERSION_10_1_0_101: u32 = 0x0A01_0065;
pub const VERSION_10_1_0_106: u32 = 0x0A01_006A;

/// Versions the reader reads.
pub const SUPPORTED_VERSIONS: [u32; 7] = [
    VERSION_10_0_1_0,
    VERSION_10_0_1_2,
    VERSION_10_1_0_101,
    VERSION_10_1_0_106,
    VERSION_10_2_0_0,
    VERSION_20_0_0_4,
    VERSION_20_0_0_5,
];

/// The parsed header. The blocks themselves follow right after it, from [`NifHeader::body_offset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NifHeader {
    /// The header line without `\n`, for example `Gamebryo File Format, Version 20.0.0.4`.
    pub line: String,
    pub version: u32,
    pub user_version: u32,
    pub user_version_2: u32,
    /// Three export info strings: author, process script, export script.
    pub export_info: [String; 3],
    /// Block type names — the table the indices refer to.
    pub block_types: Vec<String>,
    /// Index into `block_types` for each block, in file order.
    pub block_type_index: Vec<u16>,
    /// Block groups. Empty in all checked files; read so the cursor lands at the start of the
    /// blocks for any value.
    pub groups: Vec<u32>,
    /// Offset of the first block from the start of the file.
    pub body_offset: usize,
}

impl NifHeader {
    /// Block type name by its number in the file.
    pub fn block_type_name(&self, block: usize) -> Option<&str> {
        let index = *self.block_type_index.get(block)?;
        self.block_types.get(usize::from(index)).map(String::as_str)
    }

    pub fn num_blocks(&self) -> usize {
        self.block_type_index.len()
    }
}

/// Parses only the header. A version not in [`SUPPORTED_VERSIONS`] is an
/// [`Error::UnsupportedVersion`] error.
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

    // The endianness byte since 20.0.0.3; 10.2.0.0 does not have it.
    if version >= VERSION_20_0_0_4 {
        let endian = cur.u8()?;
        if endian != 1 {
            return Err(Error::UnsupportedEndian { flag: endian });
        }
    }

    // User version since 10.0.1.8: 10.0.1.0 and 10.0.1.2 do not have it.
    let user_version = if version >= 0x0A00_0108 {
        cur.u32()?
    } else {
        0
    };
    let num_blocks = cur.len_u32()?;
    // The Bethesda header (`BSVER` and three strings, `#BSSTREAMHEADER#`): in 10.0.1.2 — always
    // (all 23 files have `BSVER` 3), in 10.1.0.0…20.0.0.4 — with user version 3…11 (it is 10 in all
    // Oblivion files, 2026-09-26), in 10.0.1.0 — never.
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

    // We do not blindly reserve `num_blocks` from the file: a claimed number larger than what
    // physically fits means a truncated or broken file.
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

    /// Builds a header by hand, following the layout in `nif-oblivion.md`.
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

    /// Old 10.x (T2.3b): version, [user version since 10.0.1.8], block count, [Bethesda header],
    /// one type table for one block.
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

    /// 10.2.0.0: there is no endianness byte, the `BSVER` from the Bethesda header is its own (9
    /// here).
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

        // A user version outside 3…11 — there is no Bethesda header, such a file is not ours.
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
                "truncation to {cut} bytes passed"
            );
        }
        assert!(parse(&bytes).is_ok());
    }

    #[test]
    fn a_huge_declared_block_count_is_an_error() {
        let mut bytes = build(VERSION_20_0_0_4, &["NiNode"], &[0], &[]);
        // num_blocks comes after the string, the version, the endianness flag and user_version
        let at = LINE_4.len() + 4 + 1 + 4;
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(parse(&bytes), Err(Error::Truncated { .. })));
    }
}
