//! Byte/bit cursor over an in-memory log.

/// Cursor over a byte slice with optional bit-level reads (needed only for
/// Elias-delta encoded fields). Byte reads always re-align to a byte boundary.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bits already consumed from `data[pos]` (0 = aligned).
    bit: u8,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader {
            data,
            pos: 0,
            bit: 0,
        }
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Byte position; if a byte is partially consumed it is still counted as current.
    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos.min(self.data.len());
        self.bit = 0;
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Skip the rest of a partially consumed byte.
    pub fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    pub fn peek_u8(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    pub fn read_u8(&mut self) -> Option<u8> {
        self.align();
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    pub fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        self.align();
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    /// Read one bit, most-significant bit of each byte first.
    pub fn read_bit(&mut self) -> Option<bool> {
        let b = *self.data.get(self.pos)?;
        let bit = (b >> (7 - self.bit)) & 1 == 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Some(bit)
    }

    /// Read up to 32 bits, MSB first.
    pub fn read_bits(&mut self, n: u32) -> Option<u32> {
        debug_assert!(n <= 32);
        let mut v = 0u64;
        for _ in 0..n {
            v = (v << 1) | u64::from(self.read_bit()?);
        }
        Some(v as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_and_bytes() {
        let data = [0b1010_0000u8, 0xFF, 0x12];
        let mut r = Reader::new(&data);
        assert_eq!(r.read_bit(), Some(true));
        assert_eq!(r.read_bit(), Some(false));
        assert_eq!(r.read_bits(2), Some(0b10));
        // byte read re-aligns and skips the rest of byte 0
        assert_eq!(r.read_u8(), Some(0xFF));
        assert_eq!(r.pos(), 2);
        assert_eq!(r.read_bits(8), Some(0x12));
        assert_eq!(r.read_bit(), None);
        assert!(r.is_eof());
    }
}
