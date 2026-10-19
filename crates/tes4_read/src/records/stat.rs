use crate::container::Tag;
use crate::error::Result;
use crate::schema::{Fields, FromFields};

/// Статический объект. Самая простая запись с мешем, поэтому с неё начинается
/// сквозной путь «прочитал — написал»: в TES3 ей соответствует `STAT`, у
/// которого нет ничего, кроме `NAME` и `MODL`.
///
/// Поля различаются между играми, и спека покрывает оба варианта:
/// в TES4 габарит задаётся `MODB` (радиус), в TES5 — `OBND` (шесть `i16`).
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub editor_id: String,
    pub model: Option<String>,
    /// TES4: радиус габаритной сферы.
    pub bound_radius: Option<f32>,
    /// TES5: габаритный параллелепипед, min xyz и max xyz.
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
    for (slot, chunk) in out.iter_mut().zip(raw.chunks_exact(2)) {
        *slot = i16::from_le_bytes(
            chunk
                .try_into()
                .expect("chunks_exact(2) даёт ровно 2 байта"),
        );
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

    /// Запись с пустыми данными (data_size = 0) возвращает MissingField для EDID.
    ///
    /// Такие записи встречаются в Oblivion.esm (как минимум FormID=0008060C).
    /// `Fields::parse` на пустом срезе честно возвращает пустой набор полей,
    /// а попытка достать обязательный EDID оттуда → MissingField. Это ожидаемое
    /// поведение; вызывающий должен пропускать записи с `data.is_empty()`.
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

    /// Сквозная проверка фазы 0: спека читает запись, вынутую из контейнера,
    /// а не из подготовленных вручную байтов.
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
