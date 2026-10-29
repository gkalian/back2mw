use super::error::{Error, Result};

/// A byte cursor with a position, modeled on `tes4_read::container`. The position is needed in
/// every error: NIF 20.0.0.x has no block sizes, and the only layout check is where exactly the
/// cursor stopped.
///
/// It carries the file version and `BSVER` (T2.3): in 10.2.0.0 several fields are placed
/// differently than in 20.0.0.4, and a block reader asks for the version here rather than getting
/// it as a parameter.
#[derive(Debug)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    version: u32,
    bsver: u32,
}

/// The default version is 20.0.0.4 with `BSVER` 11: the synthetic block tests are written that way.
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

    /// File version and `BSVER` from the header.
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
            .expect("bytes(N) returned exactly N bytes, otherwise there would have been an error"))
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

    /// A `bool` in NIF 20.x is one byte. A value other than 0 and 1 is a sign that the layout went
    /// off earlier, so it is an error, not "true".
    pub fn bool(&mut self) -> Result<bool> {
        let at = self.pos;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(Error::InvalidBool { at, value }),
        }
    }

    /// `N` consecutive `f32`: a vector, a color, a quaternion, a matrix.
    pub fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0f32; N];
        for slot in &mut out {
            *slot = self.f32()?;
        }
        Ok(out)
    }

    /// `count` elements of `elem_size` bytes: checks that they fit at all before allocating memory
    /// for them. The number claimed in the file is not trusted.
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

    /// `count` arrays of `N` `f32` each.
    pub fn float_arrays<const N: usize>(&mut self, count: usize) -> Result<Vec<[f32; N]>> {
        self.check_array(count, N * 4)?;
        (0..count).map(|_| self.floats::<N>()).collect()
    }

    /// A length from the file as `usize`. A value that does not fit into `usize` is certainly
    /// larger than any file, so the error is the same as for "not enough data".
    pub fn len_u32(&mut self) -> Result<usize> {
        let at = self.pos;
        let raw = self.u32()?;
        usize::try_from(raw).map_err(|_| Error::Truncated {
            at,
            need: usize::MAX,
            have: self.remaining(),
        })
    }

    /// A line up to and including `\n`; the `\n` itself is not part of the result.
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

    /// A string with a one-byte length (export info in the header).
    pub fn short_string(&mut self) -> Result<String> {
        let len = usize::from(self.u8()?);
        Ok(latin1(self.bytes(len)?))
    }

    /// A string with a four-byte length (block type names and strings in blocks).
    pub fn sized_string(&mut self) -> Result<String> {
        let len = self.len_u32()?;
        Ok(latin1(self.bytes(len)?))
    }
}

/// Bytes → string without loss: headers and type names can contain non-UTF-8, and characters should
/// not be silently replaced.
pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}
