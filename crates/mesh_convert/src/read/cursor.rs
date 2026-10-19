use super::error::{Error, Result};

/// Курсор по байтам с позицией, по образцу `tes4_read::container`. Позиция нужна
/// в каждой ошибке: размеров блоков в NIF 20.0.0.x нет, и единственная проверка
/// раскладки — где именно остановился курсор.
///
/// Несёт версию файла и `BSVER` (T2.3): у 10.2.0.0 несколько полей стоят иначе, чем у
/// 20.0.0.4, и ридер блока спрашивает версию здесь, а не получает её параметром.
#[derive(Debug)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    version: u32,
    bsver: u32,
}

/// Версия по умолчанию — 20.0.0.4 с `BSVER` 11: так написаны синтетические тесты блоков.
const DEFAULT_VERSION: u32 = 0x1400_0004;
const DEFAULT_BSVER: u32 = 11;

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            version: DEFAULT_VERSION,
            bsver: DEFAULT_BSVER,
        }
    }

    /// Версия файла и `BSVER` из заголовка.
    pub fn set_version(&mut self, version: u32, bsver: u32) {
        self.version = version;
        self.bsver = bsver;
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn bsver(&self) -> u32 {
        self.bsver
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::Truncated {
                at: self.pos,
                need: n,
                have: self.remaining(),
            });
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let slice = self.bytes(N)?;
        Ok(slice
            .try_into()
            .expect("bytes(N) вернул ровно N байт, иначе была бы ошибка"))
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    /// `bool` в NIF 20.x — один байт. Значение, отличное от 0 и 1, — признак того,
    /// что раскладка съехала раньше, поэтому это ошибка, а не «истина».
    pub fn bool(&mut self) -> Result<bool> {
        let at = self.pos;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(Error::InvalidBool { at, value }),
        }
    }

    /// `N` подряд идущих `f32`: вектор, цвет, кватернион, матрица.
    pub fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0f32; N];
        for slot in &mut out {
            *slot = self.f32()?;
        }
        Ok(out)
    }

    /// `count` элементов по `elem_size` байт: проверяет, что они вообще помещаются,
    /// до того как под них выделять память. Заявленное в файле число не доверено.
    pub fn check_array(&self, count: usize, elem_size: usize) -> Result<()> {
        let need = count.saturating_mul(elem_size);
        if self.remaining() < need {
            return Err(Error::Truncated {
                at: self.pos,
                need,
                have: self.remaining(),
            });
        }
        Ok(())
    }

    /// `count` массивов из `N` `f32` каждый.
    pub fn float_arrays<const N: usize>(&mut self, count: usize) -> Result<Vec<[f32; N]>> {
        self.check_array(count, N * 4)?;
        (0..count).map(|_| self.floats::<N>()).collect()
    }

    /// Длина из файла как `usize`. Значение, не влезающее в `usize`, заведомо больше
    /// любого файла, поэтому ошибка та же, что и у «мало данных».
    pub fn len_u32(&mut self) -> Result<usize> {
        let at = self.pos;
        let raw = self.u32()?;
        usize::try_from(raw).map_err(|_| Error::Truncated {
            at,
            need: usize::MAX,
            have: self.remaining(),
        })
    }

    /// Строка до `\n` включительно; сам `\n` в результат не входит.
    pub fn line(&mut self) -> Result<&'a [u8]> {
        let rest = &self.data[self.pos..];
        let end = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or(Error::Truncated {
                at: self.pos,
                need: rest.len() + 1,
                have: rest.len(),
            })?;
        self.pos += end + 1;
        Ok(&rest[..end])
    }

    /// Строка с однобайтовой длиной (экспортная информация в заголовке).
    pub fn short_string(&mut self) -> Result<String> {
        let len = usize::from(self.u8()?);
        Ok(latin1(self.bytes(len)?))
    }

    /// Строка с четырёхбайтовой длиной (имена типов блоков и строки в блоках).
    pub fn sized_string(&mut self) -> Result<String> {
        let len = self.len_u32()?;
        Ok(latin1(self.bytes(len)?))
    }
}

/// Байты → строка без потерь: в заголовках и именах типов бывает не-UTF-8, и
/// заменять символы молча не нужно.
pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}
