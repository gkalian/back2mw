use crate::container::Tag;
use crate::error::Result;
use crate::schema::{Fields, FromFields};

/// A static object. The simplest record with a mesh, so the end-to-end "read — write" path starts
/// with it: in TES3 it corresponds to `STAT`, which has nothing but `NAME` and `MODL`.
///
/// The fields differ between the games, and the spec covers both variants: in TES4 the bounds are
/// given by `MODB` (a radius), in TES5 by `OBND` (six `i16`).
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub editor_id: String,
    pub model: Option<String>,
    /// TES4: bounding sphere radius.
    pub bound_radius: Option<f32>,
    /// TES5: bounding box, min xyz and max xyz.
    pub bounds: Option<[i16; 6]>,
}

impl FromFields for Stat {
    const SIG: Tag = *b"STAT";
    const NAME: &'static str = "STAT";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            model: fields.opt_zstring(*b"MODL"),
            bound_radius: fields.opt_f32(*b"MODB")?,
            bounds: fields.opt_fixed::<12>(*b"OBND")?.map(decode_bounds),
        })
    }
}

fn decode_bounds(raw: [u8; 12]) -> [i16; 6] {
    let mut out = [0i16; 6];
    for (slot, &chunk) in out.iter_mut().zip(raw.as_chunks::<2>().0) {
        *slot = i16::from_le_bytes(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{GameFormat, Plugin, Record, RecordHeader};
    use crate::error::Error;
    use crate::synth;

    fn stat_record(fields: Vec<u8>) -> Record {
        Record {
            header: RecordHeader {
                tag: *b"STAT",
                flags: 0,
                form_id: 0x0001_0203,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: fields,
        }
    }

    #[test]
    fn reads_tes4_shape() {
        let mut data = Vec::new();
        data.extend_from_slice(&synth::field(b"EDID", b"Bowl01\0"));
        data.extend_from_slice(&synth::field(b"MODL", b"Clutter\\Bowl01.nif\0"));
        data.extend_from_slice(&synth::field(b"MODB", &12.5f32.to_le_bytes()));

        let stat = Stat::from_record(&stat_record(data)).unwrap();
        assert_eq!(stat.editor_id, "Bowl01");
        assert_eq!(stat.model.as_deref(), Some("Clutter\\Bowl01.nif"));
        assert_eq!(stat.bound_radius, Some(12.5));
        assert_eq!(stat.bounds, None);
    }

    #[test]
    fn reads_tes5_shape_with_obnd() {
        let mut obnd = Vec::new();
        for v in [-1i16, -2, -3, 4, 5, 6] {
            obnd.extend_from_slice(&v.to_le_bytes());
        }
        let mut data = Vec::new();
        data.extend_from_slice(&synth::field(b"EDID", b"Rock\0"));
        data.extend_from_slice(&synth::field(b"OBND", &obnd));

        let stat = Stat::from_record(&stat_record(data)).unwrap();
        assert_eq!(stat.bounds, Some([-1, -2, -3, 4, 5, 6]));
        assert_eq!(stat.bound_radius, None);
        assert_eq!(stat.model, None);
    }

    #[test]
    fn rejects_a_record_of_another_type() {
        let mut wrong = stat_record(synth::field(b"EDID", b"x\0"));
        wrong.header.tag = *b"ACTI";
        assert_eq!(
            Stat::from_record(&wrong).unwrap_err(),
            Error::WrongRecord {
                expected: *b"STAT",
                got: *b"ACTI"
            }
        );
    }

    /// A record with empty data (data_size = 0) returns MissingField for EDID.
    ///
    /// Such records occur in Oblivion.esm (at least FormID=0008060C). `Fields::parse` on an empty
    /// slice honestly returns an empty field set, and trying to get the required EDID from it gives
    /// MissingField. This is expected behavior; the caller must skip records with
    /// `data.is_empty()`.
    #[test]
    fn empty_data_returns_missing_field() {
        let record = stat_record(vec![]);
        assert!(record.data.is_empty());
        assert_eq!(
            Stat::from_record(&record).unwrap_err(),
            Error::MissingField {
                record: "STAT",
                field: *b"EDID"
            }
        );
    }

    /// The end-to-end phase 0 check: the spec reads a record taken from the container, not from
    /// hand-prepared bytes.
    #[test]
    fn reads_through_the_container() {
        let stat = synth::record(
            GameFormat::Tes4,
            b"STAT",
            0x0000_1234,
            0,
            &[
                synth::field(b"EDID", b"Bowl01\0"),
                synth::field(b"MODL", b"a.nif\0"),
            ]
            .concat(),
        );
        let bytes = synth::plugin(
            GameFormat::Tes4,
            &synth::header_fields(1),
            &[synth::group(GameFormat::Tes4, b"STAT", 0, &stat)],
        );

        let plugin = Plugin::parse(&bytes).unwrap();
        let records: Vec<_> = plugin.records_of(*b"STAT").collect();
        assert_eq!(records.len(), 1);

        let parsed = Stat::from_record(records[0]).unwrap();
        assert_eq!(parsed.editor_id, "Bowl01");
        assert_eq!(records[0].header.form_id, 0x0000_1234);
    }
}
