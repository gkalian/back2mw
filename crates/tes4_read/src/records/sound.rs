use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// `SNDX` flag: the sound loops.
pub const SOUND_LOOP: u32 = 0x10;

/// A sound (`SOUN`, T10.1).
///
/// The layout was measured on `Oblivion.esm` (2026-09-28, `docs/en/formats/tes4.md`, "Sounds"): all
/// 1,140 have `FNAM`, in 618 it is a `.wav` file, in 522 a **directory** ending with `\` (the
/// engine takes a random file from it each time). `SNDX` (12 bytes) in 1,138, the old `SNDD` (8
/// bytes, without attenuation and time) in 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sound {
    pub editor_id: String,
    /// `FNAM` — relative to `Sound\`: a file or a directory (ends with `\`).
    pub file: String,
    /// The distance up to which the sound does not attenuate, in fives of units.
    pub min_distance: u8,
    /// The distance beyond which the sound is not heard, in hundreds of units.
    pub max_distance: u8,
    /// Frequency shift, %.
    pub frequency: i8,
    /// 0x1 random frequency shift, 0x2 play at random, 0x4 no reverb, 0x8 random location, 0x10
    /// loop ([`SOUND_LOOP`]), 0x20 menu sound, 0x40 2D, 0x80 LFE.
    pub flags: u32,
    /// Static attenuation, hundredths of a dB (0 for `SNDD`, which has no such field).
    pub attenuation: u16,
}

impl Sound {
    /// `FNAM` is a directory: the engine picks a file at random.
    pub fn is_directory(&self) -> bool {
        self.file.ends_with(['\\', '/'])
    }
}

impl FromFields for Sound {
    const SIG: Tag = *b"SOUN";
    const NAME: &'static str = "SOUN";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let raw = match (fields.first(*b"SNDX"), fields.first(*b"SNDD")) {
            (Some(raw), _) if raw.len() == 12 => raw,
            (None, Some(raw)) if raw.len() == 8 => raw,
            (Some(raw), _) | (None, Some(raw)) => {
                return Err(Error::FieldSize {
                    record: "SOUN",
                    field: *b"SNDX",
                    expected: 12,
                    got: raw.len(),
                });
            }
            (None, None) => fields.required(*b"SNDX")?,
        };
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            file: fields.zstring(*b"FNAM")?,
            min_distance: raw[0],
            max_distance: raw[1],
            frequency: i8::from_le_bytes([raw[2]]),
            flags: u32::from_le_bytes(raw[4..8].try_into().expect("the slice is exactly 4 bytes")),
            attenuation: raw
                .get(8..10)
                .map_or(0, |b| u16::from_le_bytes([b[0], b[1]])),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Record, RecordHeader};
    use crate::synth;

    fn record(fields: Vec<Vec<u8>>) -> Record {
        Record {
            header: RecordHeader {
                tag: *b"SOUN",
                flags: 0,
                form_id: 1,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: fields.concat(),
        }
    }

    fn data(long: bool) -> Vec<u8> {
        let mut d = vec![60, 20, 0xFE, 0];
        d.extend(0x11u32.to_le_bytes());
        if long {
            d.extend(617u16.to_le_bytes());
            d.extend([0, 0]);
        }
        d
    }

    #[test]
    fn both_layouts_are_read() {
        for long in [true, false] {
            let tag = if long { b"SNDX" } else { b"SNDD" };
            let s = Sound::from_record(&record(vec![
                synth::field(b"EDID", b"AMBWaterfall\0"),
                synth::field(b"FNAM", b"fx\\ambient\\waterfall.wav\0"),
                synth::field(tag, &data(long)),
            ]))
            .unwrap();
            assert_eq!(s.min_distance, 60);
            assert_eq!(s.max_distance, 20);
            assert_eq!(s.frequency, -2);
            assert_eq!(s.flags, 0x11);
            assert_eq!(s.attenuation, if long { 617 } else { 0 }, "long = {long}");
            assert!(!s.is_directory());
        }
    }

    #[test]
    fn a_folder_is_recognised_and_a_wrong_size_is_an_error() {
        let s = Sound::from_record(&record(vec![
            synth::field(b"EDID", b"NPCFoot\0"),
            synth::field(b"FNAM", b"fx\\npc\\foot\\\0"),
            synth::field(b"SNDX", &data(true)),
        ]))
        .unwrap();
        assert!(s.is_directory());
        let err = Sound::from_record(&record(vec![
            synth::field(b"EDID", b"Bad\0"),
            synth::field(b"FNAM", b"a.wav\0"),
            synth::field(b"SNDX", &[0; 10]),
        ]))
        .unwrap_err();
        assert!(matches!(err, Error::FieldSize { got: 10, .. }), "{err:?}");
    }
}
