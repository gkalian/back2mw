//! Декларативное чтение полей записи.
//!
//! Смысл в том, чтобы спека на тип записи была таблицей полей, а не рукописным
//! разбором по смещениям. Referenc-проект делает второе — `struct.unpack` на
//! каждое поле в каждой стадии, — и платит за это объёмом и целым классом
//! ошибок. Здесь разбор контейнера полей общий, а спека записи только называет
//! поля и их типы.

use crate::container::{Record, Tag};
use crate::cp1252;
use crate::error::{Error, Result};

/// Поле, размер которого не влезает в `u16`: следующее за ним поле берёт размер
/// отсюда, а своё поле размера держит нулём.
const XXXX: Tag = *b"XXXX";

/// Разобранные поля записи в порядке файла. Повторы сохраняются — у многих
/// записей одно и то же поле встречается по нескольку раз.
#[derive(Debug, Clone)]
pub struct Fields<'a> {
    subs: Vec<(Tag, &'a [u8])>,
    record: &'static str,
}

impl<'a> Fields<'a> {
    pub fn parse(data: &'a [u8], record: &'static str) -> Result<Self> {
        let mut subs = Vec::new();
        let mut pos = 0usize;
        let mut size_override: Option<usize> = None;

        while pos < data.len() {
            let available = data.len() - pos;
            if available < 6 {
                return Err(Error::Truncated {
                    at: pos,
                    need: 6,
                    have: available,
                });
            }
            let tag: Tag = data[pos..pos + 4].try_into().expect("срез ровно 4 байта");
            let small = u16::from_le_bytes(
                data[pos + 4..pos + 6]
                    .try_into()
                    .expect("срез ровно 2 байта"),
            );
            pos += 6;

            if tag == XXXX {
                if small != 4 {
                    return Err(Error::FieldSize {
                        record,
                        field: XXXX,
                        expected: 4,
                        got: usize::from(small),
                    });
                }
                let bytes = data.get(pos..pos + 4).ok_or(Error::Truncated {
                    at: pos,
                    need: 4,
                    have: data.len() - pos,
                })?;
                size_override = Some(u32::from_le_bytes(
                    bytes.try_into().expect("срез ровно 4 байта"),
                ) as usize);
                pos += 4;
                continue;
            }

            let size = size_override.take().unwrap_or(usize::from(small));
            let value = data.get(pos..pos + size).ok_or(Error::Truncated {
                at: pos,
                need: size,
                have: data.len() - pos,
            })?;
            pos += size;
            subs.push((tag, value));
        }

        Ok(Self { subs, record })
    }

    pub fn record_name(&self) -> &'static str {
        self.record
    }

    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.subs.len()
    }

    /// Теги в порядке файла — для диагностики нераспознанных полей.
    pub fn tags(&self) -> impl Iterator<Item = Tag> + '_ {
        self.subs.iter().map(|(tag, _)| *tag)
    }

    /// Поля в порядке файла. Нужно там, где смысл поля задаёт предыдущее: `VTXT` относится к
    /// ближайшему `ATXT` перед ним, `RDWT` — к ближайшему `RDAT`.
    pub fn iter(&self) -> impl Iterator<Item = (Tag, &'a [u8])> + '_ {
        self.subs.iter().copied()
    }

    pub fn first(&self, tag: Tag) -> Option<&'a [u8]> {
        self.subs.iter().find(|(t, _)| *t == tag).map(|(_, v)| *v)
    }

    pub fn all(&self, tag: Tag) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.subs
            .iter()
            .filter(move |(t, _)| *t == tag)
            .map(|(_, v)| *v)
    }

    pub fn required(&self, tag: Tag) -> Result<&'a [u8]> {
        self.first(tag).ok_or(Error::MissingField {
            record: self.record,
            field: tag,
        })
    }

    pub fn zstring(&self, tag: Tag) -> Result<String> {
        Ok(cp1252::decode_zstring(self.required(tag)?))
    }

    pub fn opt_zstring(&self, tag: Tag) -> Option<String> {
        self.first(tag).map(cp1252::decode_zstring)
    }

    pub fn fixed<const N: usize>(&self, tag: Tag) -> Result<[u8; N]> {
        let value = self.required(tag)?;
        value.try_into().map_err(|_| Error::FieldSize {
            record: self.record,
            field: tag,
            expected: N,
            got: value.len(),
        })
    }

    pub fn opt_fixed<const N: usize>(&self, tag: Tag) -> Result<Option<[u8; N]>> {
        match self.first(tag) {
            None => Ok(None),
            Some(value) => value.try_into().map(Some).map_err(|_| Error::FieldSize {
                record: self.record,
                field: tag,
                expected: N,
                got: value.len(),
            }),
        }
    }

    pub fn f32(&self, tag: Tag) -> Result<f32> {
        Ok(f32::from_le_bytes(self.fixed::<4>(tag)?))
    }

    pub fn opt_f32(&self, tag: Tag) -> Result<Option<f32>> {
        Ok(self.opt_fixed::<4>(tag)?.map(f32::from_le_bytes))
    }

    pub fn u32(&self, tag: Tag) -> Result<u32> {
        Ok(u32::from_le_bytes(self.fixed::<4>(tag)?))
    }

    pub fn opt_u32(&self, tag: Tag) -> Result<Option<u32>> {
        Ok(self.opt_fixed::<4>(tag)?.map(u32::from_le_bytes))
    }
}

/// Спека записи: сигнатура плюс отображение полей в типизированную структуру.
pub trait FromFields: Sized {
    const SIG: Tag;
    const NAME: &'static str;

    fn from_fields(fields: &Fields<'_>) -> Result<Self>;

    fn from_record(record: &Record) -> Result<Self> {
        if record.header.tag != Self::SIG {
            return Err(Error::WrongRecord {
                expected: Self::SIG,
                got: record.header.tag,
            });
        }
        Self::from_fields(&Fields::parse(&record.data, Self::NAME)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    #[test]
    fn reads_fields_in_file_order_keeping_repeats() {
        let mut data = Vec::new();
        data.extend_from_slice(&synth::field(b"EDID", b"Bowl01\0"));
        data.extend_from_slice(&synth::field(b"MODL", b"a.nif\0"));
        data.extend_from_slice(&synth::field(b"MODL", b"b.nif\0"));

        let fields = Fields::parse(&data, "STAT").unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(
            fields.tags().collect::<Vec<_>>(),
            [*b"EDID", *b"MODL", *b"MODL"]
        );
        assert_eq!(fields.zstring(*b"EDID").unwrap(), "Bowl01");
        // `first` берёт первое, `all` — все.
        assert_eq!(fields.opt_zstring(*b"MODL").unwrap(), "a.nif");
        assert_eq!(fields.all(*b"MODL").count(), 2);
    }

    #[test]
    fn xxxx_overrides_size_of_next_field_only() {
        let big = vec![0xABu8; usize::from(u16::MAX) + 10];
        let mut data = Vec::new();
        data.extend_from_slice(&synth::field(b"HUGE", &big));
        data.extend_from_slice(&synth::field(b"EDID", b"x\0"));

        let fields = Fields::parse(&data, "TEST").unwrap();
        assert_eq!(fields.first(*b"HUGE").unwrap().len(), big.len());
        assert_eq!(fields.zstring(*b"EDID").unwrap(), "x");
        // Сам XXXX в списке полей не остаётся — это служебная обёртка.
        assert!(!fields.tags().any(|t| t == XXXX));
    }

    #[test]
    fn missing_required_field_names_itself() {
        let data = synth::field(b"MODL", b"a\0");
        let fields = Fields::parse(&data, "STAT").unwrap();
        assert_eq!(
            fields.zstring(*b"EDID").unwrap_err(),
            Error::MissingField {
                record: "STAT",
                field: *b"EDID"
            }
        );
    }

    #[test]
    fn wrong_size_is_reported_not_silently_accepted() {
        let data = synth::field(b"MODB", &[1, 2]);
        let fields = Fields::parse(&data, "STAT").unwrap();
        assert_eq!(
            fields.f32(*b"MODB").unwrap_err(),
            Error::FieldSize {
                record: "STAT",
                field: *b"MODB",
                expected: 4,
                got: 2
            }
        );
    }

    #[test]
    fn truncated_field_is_an_error() {
        // Заявлено 10 байт, приложено 2.
        let mut data = Vec::new();
        data.extend_from_slice(b"EDID");
        data.extend_from_slice(&10u16.to_le_bytes());
        data.extend_from_slice(&[1, 2]);
        assert!(Fields::parse(&data, "STAT").is_err());
    }
}
