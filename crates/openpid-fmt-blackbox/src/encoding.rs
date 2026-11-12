//! Field encodings of the Blackbox format (decoders, plus encoders for tests
//! and tooling). Encoding ids follow Betaflight's `blackbox_fielddefs.h`.

use crate::reader::Reader;

/// `ZigZag` signed variable-byte.
pub const SIGNED_VB: u8 = 0;
/// Unsigned variable-byte (7 bits per byte).
pub const UNSIGNED_VB: u8 = 1;
/// Negated 14-bit value as unsigned variable-byte (battery voltage).
pub const NEG_14BIT: u8 = 3;
/// Elias-delta unsigned 32-bit (bit stream).
pub const ELIAS_DELTA_U32: u8 = 4;
/// Elias-delta `ZigZag` signed 32-bit (bit stream).
pub const ELIAS_DELTA_S32: u8 = 5;
/// Up to 8 signed variable-byte fields behind a presence-bit header.
pub const TAG8_8SVB: u8 = 6;
/// Three signed fields with a 2-bit width selector.
pub const TAG2_3S32: u8 = 7;
/// Four signed fields of 0/4/8/16 bits with a byte selector.
pub const TAG8_4S16: u8 = 8;
/// Nothing is written; the value is zero.
pub const NULL: u8 = 9;
/// Three signed fields packed as 2/2/2, 5/5/4, 8/7/7 bits or bytes.
pub const TAG2_3SVARIABLE: u8 = 10;

/// Whether this crate can decode the encoding id.
pub fn is_known(encoding: u8) -> bool {
    matches!(
        encoding,
        SIGNED_VB
            | UNSIGNED_VB
            | NEG_14BIT
            | ELIAS_DELTA_U32
            | ELIAS_DELTA_S32
            | TAG8_8SVB
            | TAG2_3S32
            | TAG8_4S16
            | NULL
            | TAG2_3SVARIABLE
    )
}

/// Number of fields consumed by one encoded unit (grouped encodings), or 1.
pub fn group_size(encoding: u8) -> usize {
    match encoding {
        TAG2_3S32 | TAG2_3SVARIABLE => 3,
        TAG8_4S16 => 4,
        _ => 1,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
/// Why a field could not be decoded.
pub enum DecodeError {
    #[error("unexpected end of data")]
    /// The data ended inside a value.
    Eof,
    #[error("variable-byte value longer than 5 bytes")]
    /// A variable-byte value ran past 32 bits.
    Overflow,
    #[error("corrupt group header")]
    /// A group header had impossible bits set.
    BadGroup,
    #[error("unsupported encoding {0}")]
    /// The encoding id is not implemented.
    Unsupported(u8),
}

/// Result of a decoding step.
pub type Result<T> = core::result::Result<T, DecodeError>;

#[inline]
/// `ZigZag` fold: `0, 1, 2, 3 → 0, -1, 1, -2`.
pub fn zigzag_decode(v: u32) -> i32 {
    ((v >> 1) as i32) ^ -((v & 1) as i32)
}

#[inline]
/// Inverse of [`zigzag_decode`].
pub fn zigzag_encode(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

/// Sign-extend the low `bits` bits of `v`.
#[inline]
pub fn sign_extend(v: u32, bits: u32) -> i32 {
    let s = 32 - bits;
    ((v << s) as i32) >> s
}

/// Unsigned variable-byte (7 bits per byte, high bit = continuation).
pub fn read_uvb(r: &mut Reader<'_>) -> Result<u32> {
    let mut value = 0u32;
    let mut shift = 0u32;
    loop {
        let b = r.read_u8().ok_or(DecodeError::Eof)?;
        value |= u32::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift > 28 {
            return Err(DecodeError::Overflow);
        }
    }
}

/// ZigZag-folded signed variable-byte.
pub fn read_svb(r: &mut Reader<'_>) -> Result<i32> {
    read_uvb(r).map(zigzag_decode)
}

/// Negated 14-bit value stored as unsigned variable-byte.
pub fn read_neg14(r: &mut Reader<'_>) -> Result<i32> {
    let v = read_uvb(r)?;
    Ok(-sign_extend(v, 14))
}

/// Header byte with one presence bit per field, then signed VB for present fields.
/// `count` is the number of fields in the group (1..=8).
pub fn read_tag8_8svb(r: &mut Reader<'_>, count: usize) -> Result<[i32; 8]> {
    debug_assert!((1..=8).contains(&count));
    let mut out = [0i32; 8];
    if count == 1 {
        out[0] = read_svb(r)?;
        return Ok(out);
    }
    let mut header = r.read_u8().ok_or(DecodeError::Eof)?;
    for v in out.iter_mut().take(count) {
        if header & 1 == 1 {
            *v = read_svb(r)?;
        }
        header >>= 1;
    }
    if header != 0 {
        return Err(DecodeError::BadGroup);
    }
    Ok(out)
}

/// Three signed values with a 2-bit width selector.
pub fn read_tag2_3s32(r: &mut Reader<'_>) -> Result<[i32; 3]> {
    let lead = r.read_u8().ok_or(DecodeError::Eof)?;
    let mut out = [0i32; 3];
    match lead >> 6 {
        0 => {
            out[0] = sign_extend(u32::from((lead >> 4) & 0x03), 2);
            out[1] = sign_extend(u32::from((lead >> 2) & 0x03), 2);
            out[2] = sign_extend(u32::from(lead & 0x03), 2);
        }
        1 => {
            out[0] = sign_extend(u32::from(lead & 0x0F), 4);
            let b = r.read_u8().ok_or(DecodeError::Eof)?;
            out[1] = sign_extend(u32::from(b >> 4), 4);
            out[2] = sign_extend(u32::from(b & 0x0F), 4);
        }
        2 => {
            out[0] = sign_extend(u32::from(lead & 0x3F), 6);
            let b = r.read_u8().ok_or(DecodeError::Eof)?;
            out[1] = sign_extend(u32::from(b & 0x3F), 6);
            let b = r.read_u8().ok_or(DecodeError::Eof)?;
            out[2] = sign_extend(u32::from(b & 0x3F), 6);
        }
        _ => {
            let mut sel = lead;
            for v in &mut out {
                let width = usize::from(sel & 0x03) + 1;
                *v = read_le_signed(r, width)?;
                sel >>= 2;
            }
        }
    }
    Ok(out)
}

/// Little-endian signed integer of `width` bytes (1..=4).
fn read_le_signed(r: &mut Reader<'_>, width: usize) -> Result<i32> {
    let bytes = r.read_bytes(width).ok_or(DecodeError::Eof)?;
    let mut v = 0u32;
    for (i, b) in bytes.iter().enumerate() {
        v |= u32::from(*b) << (8 * i);
    }
    Ok(sign_extend(v, 8 * width as u32))
}

/// Four signed values of 0/4/8/16 bits with an 8-bit selector.
pub fn read_tag8_4s16(r: &mut Reader<'_>, data_version: u32) -> Result<[i32; 4]> {
    if data_version < 2 {
        read_tag8_4s16_v1(r)
    } else {
        read_tag8_4s16_v2(r)
    }
}

fn read_tag8_4s16_v1(r: &mut Reader<'_>) -> Result<[i32; 4]> {
    let mut sel = r.read_u8().ok_or(DecodeError::Eof)?;
    let mut out = [0i32; 4];
    let mut i = 0;
    while i < 4 {
        match sel & 0x03 {
            0 => {}
            1 => {
                // Two 4-bit fields share one byte.
                let b = r.read_u8().ok_or(DecodeError::Eof)?;
                out[i] = sign_extend(u32::from(b & 0x0F), 4);
                i += 1;
                sel >>= 2;
                if i < 4 {
                    out[i] = sign_extend(u32::from(b >> 4), 4);
                }
            }
            2 => out[i] = i32::from(r.read_u8().ok_or(DecodeError::Eof)? as i8),
            _ => out[i] = read_le_signed(r, 2)?,
        }
        i += 1;
        sel >>= 2;
    }
    Ok(out)
}

fn read_tag8_4s16_v2(r: &mut Reader<'_>) -> Result<[i32; 4]> {
    let mut sel = r.read_u8().ok_or(DecodeError::Eof)?;
    let mut out = [0i32; 4];
    let mut nibble_pending = false;
    let mut buffer = 0u8;
    for v in &mut out {
        match sel & 0x03 {
            0 => {}
            1 => {
                if nibble_pending {
                    *v = sign_extend(u32::from(buffer & 0x0F), 4);
                    nibble_pending = false;
                } else {
                    buffer = r.read_u8().ok_or(DecodeError::Eof)?;
                    *v = sign_extend(u32::from(buffer >> 4), 4);
                    nibble_pending = true;
                }
            }
            2 => {
                if nibble_pending {
                    let hi = buffer << 4;
                    buffer = r.read_u8().ok_or(DecodeError::Eof)?;
                    *v = i32::from((hi | (buffer >> 4)) as i8);
                } else {
                    *v = i32::from(r.read_u8().ok_or(DecodeError::Eof)? as i8);
                }
            }
            _ => {
                let c1 = r.read_u8().ok_or(DecodeError::Eof)?;
                let c2 = r.read_u8().ok_or(DecodeError::Eof)?;
                if nibble_pending {
                    let raw = (u16::from(buffer & 0x0F) << 12)
                        | (u16::from(c1) << 4)
                        | u16::from(c2 >> 4);
                    *v = i32::from(raw as i16);
                    buffer = c2;
                } else {
                    *v = i32::from(((u16::from(c1) << 8) | u16::from(c2)) as i16);
                }
            }
        }
        sel >>= 2;
    }
    Ok(out)
}

/// Three signed values packed as 2/2/2, 5/5/4, 8/7/7 bits or byte-sized.
pub fn read_tag2_3svariable(r: &mut Reader<'_>) -> Result<[i32; 3]> {
    let lead = r.read_u8().ok_or(DecodeError::Eof)?;
    let mut out = [0i32; 3];
    match lead >> 6 {
        0 => {
            out[0] = sign_extend(u32::from((lead >> 4) & 0x03), 2);
            out[1] = sign_extend(u32::from((lead >> 2) & 0x03), 2);
            out[2] = sign_extend(u32::from(lead & 0x03), 2);
        }
        1 => {
            let b1 = r.read_u8().ok_or(DecodeError::Eof)?;
            out[0] = sign_extend(u32::from((lead >> 1) & 0x1F), 5);
            out[1] = sign_extend((u32::from(lead & 0x01) << 4) | u32::from(b1 >> 4), 5);
            out[2] = sign_extend(u32::from(b1 & 0x0F), 4);
        }
        2 => {
            let b1 = r.read_u8().ok_or(DecodeError::Eof)?;
            let b2 = r.read_u8().ok_or(DecodeError::Eof)?;
            out[0] = sign_extend((u32::from(lead & 0x3F) << 2) | u32::from(b1 >> 6), 8);
            out[1] = sign_extend((u32::from(b1 & 0x3F) << 1) | u32::from(b2 >> 7), 7);
            out[2] = sign_extend(u32::from(b2 & 0x7F), 7);
        }
        _ => {
            let mut sel = lead & 0x3F;
            for v in &mut out {
                let width = usize::from(sel & 0x03) + 1;
                *v = read_le_signed(r, width)?;
                sel >>= 2;
            }
        }
    }
    Ok(out)
}

/// Elias-delta coded unsigned 32-bit value (bit stream, MSB first).
pub fn read_elias_delta_u32(r: &mut Reader<'_>) -> Result<u32> {
    let mut zeros = 0u32;
    loop {
        if r.read_bit().ok_or(DecodeError::Eof)? {
            break;
        }
        zeros += 1;
        if zeros > 5 {
            // valueLen fits in 6 bits (max 32), so more than 5 leading zeros is garbage
            return Err(DecodeError::BadGroup);
        }
    }
    let mut value_len = 1u32;
    for _ in 0..zeros {
        value_len = (value_len << 1) | u32::from(r.read_bit().ok_or(DecodeError::Eof)?);
    }
    if value_len > 32 {
        return Err(DecodeError::BadGroup);
    }
    let mut value = 1u64;
    for _ in 0..value_len - 1 {
        value = (value << 1) | u64::from(r.read_bit().ok_or(DecodeError::Eof)?);
    }
    if value == 0xFFFF_FFFF {
        let extra = u32::from(r.read_bit().ok_or(DecodeError::Eof)?);
        Ok(0xFFFF_FFFE + extra)
    } else {
        Ok((value - 1) as u32)
    }
}

/// Elias-delta coded `ZigZag` signed 32-bit value.
pub fn read_elias_delta_s32(r: &mut Reader<'_>) -> Result<i32> {
    read_elias_delta_u32(r).map(zigzag_decode)
}

/// Encoders mirroring the firmware writers. Used by tests, fuzzing seeds and
/// the test-signal tooling.
pub mod encode {
    use super::zigzag_encode;

    /// Unsigned variable-byte.
    pub fn write_uvb(buf: &mut Vec<u8>, mut v: u32) {
        while v > 127 {
            buf.push((v as u8) | 0x80);
            v >>= 7;
        }
        buf.push(v as u8);
    }

    /// `ZigZag` signed variable-byte.
    pub fn write_svb(buf: &mut Vec<u8>, v: i32) {
        write_uvb(buf, zigzag_encode(v));
    }

    /// `v` must fit in 14 bits after negation.
    pub fn write_neg14(buf: &mut Vec<u8>, v: i32) {
        write_uvb(buf, ((-v) as u32) & 0x3FFF);
    }

    /// Presence-bit header followed by the non-zero fields as signed variable-byte.
    ///
    /// # Panics
    /// Panics unless `1 <= values.len() <= 8`.
    pub fn write_tag8_8svb(buf: &mut Vec<u8>, values: &[i32]) {
        assert!((1..=8).contains(&values.len()));
        if values.len() == 1 {
            write_svb(buf, values[0]);
            return;
        }
        let mut header = 0u8;
        for (i, v) in values.iter().enumerate() {
            if *v != 0 {
                header |= 1 << i;
            }
        }
        buf.push(header);
        for v in values.iter().filter(|v| **v != 0) {
            write_svb(buf, *v);
        }
    }

    fn fits(v: i32, bits: u32) -> bool {
        let max = 1i32 << (bits - 1);
        (-max..max).contains(&v)
    }

    fn write_le(buf: &mut Vec<u8>, v: i32, width: usize) {
        for i in 0..width {
            buf.push((v >> (8 * i)) as u8);
        }
    }

    fn byte_width(v: i32) -> u8 {
        if fits(v, 8) {
            0
        } else if fits(v, 16) {
            1
        } else if fits(v, 24) {
            2
        } else {
            3
        }
    }

    /// Three signed values with a 2-bit width selector (firmware `blackboxWriteTag2_3S32`).
    pub fn write_tag2_3s32(buf: &mut Vec<u8>, v: [i32; 3]) {
        if v.iter().all(|x| fits(*x, 2)) {
            buf.push(((v[0] as u8 & 3) << 4) | ((v[1] as u8 & 3) << 2) | (v[2] as u8 & 3));
        } else if v.iter().all(|x| fits(*x, 4)) {
            buf.push((1 << 6) | (v[0] as u8 & 0x0F));
            buf.push(((v[1] as u8 & 0x0F) << 4) | (v[2] as u8 & 0x0F));
        } else if v.iter().all(|x| fits(*x, 6)) {
            buf.push((2 << 6) | (v[0] as u8 & 0x3F));
            // the firmware writes the whole low byte of the second and third
            // value: the two bits a decoder masks off carry the sign
            buf.push(v[1] as u8);
            buf.push(v[2] as u8);
        } else {
            let widths = [byte_width(v[0]), byte_width(v[1]), byte_width(v[2])];
            buf.push((3 << 6) | widths[0] | (widths[1] << 2) | (widths[2] << 4));
            for (x, w) in v.iter().zip(widths) {
                write_le(buf, *x, usize::from(w) + 1);
            }
        }
    }

    /// Data-version-2 layout.
    pub fn write_tag8_4s16(buf: &mut Vec<u8>, v: [i32; 4]) {
        let mut selector = 0u8;
        for (i, x) in v.iter().enumerate() {
            let s = if *x == 0 {
                0
            } else if fits(*x, 4) {
                1
            } else if fits(*x, 8) {
                2
            } else {
                3
            };
            selector |= s << (2 * i);
        }
        buf.push(selector);
        let mut nibble_pending = false;
        let mut buffer = 0u8;
        for (i, x) in v.iter().enumerate() {
            let x = *x;
            match (selector >> (2 * i)) & 3 {
                0 => {}
                1 => {
                    if nibble_pending {
                        buf.push(buffer | (x as u8 & 0x0F));
                        nibble_pending = false;
                    } else {
                        buffer = (x as u8 & 0x0F) << 4;
                        nibble_pending = true;
                    }
                }
                2 => {
                    if nibble_pending {
                        buf.push(buffer | ((x >> 4) as u8 & 0x0F));
                        buffer = (x as u8 & 0x0F) << 4;
                    } else {
                        buf.push(x as u8);
                    }
                }
                _ => {
                    if nibble_pending {
                        buf.push(buffer | ((x >> 12) as u8 & 0x0F));
                        buf.push((x >> 4) as u8);
                        buffer = (x as u8 & 0x0F) << 4;
                    } else {
                        buf.push((x >> 8) as u8);
                        buf.push(x as u8);
                    }
                }
            }
        }
        if nibble_pending {
            buf.push(buffer);
        }
    }

    /// Three signed values packed as the firmware's `blackboxWriteTag2_3SVariable`.
    pub fn write_tag2_3svariable(buf: &mut Vec<u8>, v: [i32; 3]) {
        if v.iter().all(|x| fits(*x, 2)) {
            buf.push(((v[0] as u8 & 3) << 4) | ((v[1] as u8 & 3) << 2) | (v[2] as u8 & 3));
        } else if fits(v[0], 5) && fits(v[1], 5) && fits(v[2], 4) {
            buf.push((1 << 6) | ((v[0] as u8 & 0x1F) << 1) | ((v[1] as u8 & 0x1F) >> 4));
            buf.push(((v[1] as u8 & 0x0F) << 4) | (v[2] as u8 & 0x0F));
        } else if fits(v[0], 8) && fits(v[1], 7) && fits(v[2], 7) {
            buf.push((2 << 6) | ((v[0] as u8) >> 2));
            buf.push(((v[0] as u8 & 0x03) << 6) | ((v[1] as u8 & 0x7F) >> 1));
            buf.push(((v[1] as u8 & 0x01) << 7) | (v[2] as u8 & 0x7F));
        } else {
            let widths = [byte_width(v[0]), byte_width(v[1]), byte_width(v[2])];
            buf.push((3 << 6) | widths[0] | (widths[1] << 2) | (widths[2] << 4));
            for (x, w) in v.iter().zip(widths) {
                write_le(buf, *x, usize::from(w) + 1);
            }
        }
    }

    /// MSB-first bit writer for Elias-delta fields.
    #[derive(Debug, Default)]
    pub struct BitWriter {
        /// Bytes written so far; the last byte may be partially filled.
        pub bytes: Vec<u8>,
        nbits: u32,
    }

    impl BitWriter {
        /// Append the low `count` bits of `bits`, most significant first.
        pub fn write_bits(&mut self, bits: u32, count: u32) {
            for i in (0..count).rev() {
                let bit = (bits >> i) & 1;
                if self.nbits.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                let last = self.bytes.len() - 1;
                self.bytes[last] |= (bit as u8) << (7 - self.nbits % 8);
                self.nbits += 1;
            }
        }

        fn write_elias_delta_internal(&mut self, value: u32) {
            let value_len = 32 - value.leading_zeros();
            let len_len = 32 - value_len.leading_zeros();
            self.write_bits(0, len_len - 1);
            self.write_bits(value_len, len_len);
            self.write_bits(value, value_len - 1);
        }

        /// Elias-delta unsigned 32-bit, with the firmware's escape for `u32::MAX - 1` and `u32::MAX`.
        pub fn write_elias_delta_u32(&mut self, value: u32) {
            if value >= 0xFFFF_FFFE {
                self.write_elias_delta_internal(0xFFFF_FFFF);
                self.write_bits(value - 0xFFFF_FFFE, 1);
            } else {
                self.write_elias_delta_internal(value + 1);
            }
        }

        /// Elias-delta `ZigZag` signed 32-bit.
        pub fn write_elias_delta_s32(&mut self, value: i32) {
            self.write_elias_delta_u32(zigzag_encode(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::encode::*;
    use super::*;

    /// Small deterministic PRNG so property-style tests need no dependencies.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// Random i32 whose magnitude spans all bit widths roughly evenly.
        fn i32(&mut self) -> i32 {
            let bits = (self.next() % 33) as u32;
            if bits == 0 {
                return 0;
            }
            let v = (self.next() as u32) & (u32::MAX >> (32 - bits));
            if self.next() & 1 == 1 {
                -i64::from(v) as i32
            } else {
                v as i32
            }
        }
    }

    #[test]
    fn uvb_examples_from_spec() {
        for (value, bytes) in [
            (1u32, vec![0x01u8]),
            (42, vec![0x2A]),
            (127, vec![0x7F]),
            (128, vec![0x80, 0x01]),
            (129, vec![0x81, 0x01]),
            (23456, vec![0xA0, 0xB7, 0x01]),
        ] {
            let mut buf = Vec::new();
            write_uvb(&mut buf, value);
            assert_eq!(buf, bytes, "{value}");
            assert_eq!(read_uvb(&mut Reader::new(&buf)), Ok(value));
        }
        assert_eq!(
            read_uvb(&mut Reader::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01])),
            Err(DecodeError::Overflow)
        );
        assert_eq!(read_uvb(&mut Reader::new(&[0x80])), Err(DecodeError::Eof));
    }

    #[test]
    fn zigzag_examples_from_spec() {
        for (s, z) in [
            (0i32, 0u32),
            (-1, 1),
            (1, 2),
            (-2, 3),
            (i32::MAX, 4_294_967_294),
            (i32::MIN, 4_294_967_295),
        ] {
            assert_eq!(zigzag_encode(s), z);
            assert_eq!(zigzag_decode(z), s);
        }
    }

    /// Bytes worked out by hand from the layouts of the firmware's encoders
    /// (Betaflight `master` 4b1520e, `src/main/blackbox/blackbox_encoding.c`):
    /// what a flight controller puts on the card for these values.
    #[test]
    fn bytes_of_the_firmware_encoders() {
        let tag2 = |v: [i32; 3]| {
            let mut buf = Vec::new();
            write_tag2_3s32(&mut buf, v);
            buf
        };
        // two bits each: one byte
        assert_eq!(tag2([1, -1, 0]), [0x1c]);
        // four bits each
        assert_eq!(tag2([7, -8, 3]), [0x47, 0x83]);
        // six bits: the second and third value keep their whole low byte
        assert_eq!(tag2([10, -5, -20]), [0x8a, 0xfb, 0xec]);
        assert_eq!(tag2([-32, 31, -9]), [0xa0, 0x1f, 0xf7]);
        // one, two and three bytes, the widths in the low bits of the tag
        assert_eq!(
            tag2([100, -200, 70_000]),
            [0xe4, 0x64, 0x38, 0xff, 0x70, 0x11, 0x01]
        );
        assert_eq!(
            tag2([-9_000_000, 0, 0]),
            [0xc3, 0xc0, 0xab, 0x76, 0xff, 0x00, 0x00]
        );
        for v in [[10, -5, -20], [-32, 31, -9], [31, -32, 0]] {
            assert_eq!(read_tag2_3s32(&mut Reader::new(&tag2(v))), Ok(v));
        }

        let mut buf = Vec::new();
        write_tag8_4s16(&mut buf, [0, 5, -100, 300]);
        assert_eq!(buf, [0xe4, 0x59, 0xc0, 0x12, 0xc0]);
        let mut buf = Vec::new();
        write_tag8_8svb(&mut buf, &[0, 3, 0, -1]);
        assert_eq!(buf, [0x0a, 0x06, 0x01]);
        // a single field has no presence byte
        let mut buf = Vec::new();
        write_tag8_8svb(&mut buf, &[-3]);
        assert_eq!(buf, [0x05]);
        // the battery against its reference: 14 bits of the difference
        let mut buf = Vec::new();
        write_neg14(&mut buf, 1650 - 1680);
        write_neg14(&mut buf, 1700 - 1680);
        assert_eq!(buf, [0x1e, 0xec, 0x7f]);
    }

    #[test]
    fn svb_roundtrip() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..10_000 {
            let v = rng.i32();
            let mut buf = Vec::new();
            write_svb(&mut buf, v);
            assert_eq!(read_svb(&mut Reader::new(&buf)), Ok(v));
        }
    }

    #[test]
    fn neg14_roundtrip() {
        for v in [-8191i32, -1000, -1, 0, 1, 1000, 8192] {
            let mut buf = Vec::new();
            write_neg14(&mut buf, v);
            assert_eq!(read_neg14(&mut Reader::new(&buf)), Ok(v), "{v}");
        }
    }

    #[test]
    fn tag8_8svb_roundtrip() {
        let mut rng = Rng(42);
        for _ in 0..5_000 {
            let count = (rng.next() % 8 + 1) as usize;
            let values: Vec<i32> = (0..count)
                .map(|_| {
                    if rng.next().is_multiple_of(3) {
                        0
                    } else {
                        rng.i32()
                    }
                })
                .collect();
            let mut buf = Vec::new();
            write_tag8_8svb(&mut buf, &values);
            let got = read_tag8_8svb(&mut Reader::new(&buf), count).unwrap();
            assert_eq!(&got[..count], &values[..]);
        }
        // spec example: 0, 0, 4, 0, 8 -> 0b00010100, 0x04, 0x08
        let mut buf = Vec::new();
        write_tag8_8svb(&mut buf, &[0, 0, 4, 0, 8]);
        assert_eq!(buf, vec![0b0001_0100, 0x08, 0x10]);
        assert_eq!(
            read_tag8_8svb(&mut Reader::new(&[0b1000_0000, 0x02]), 5),
            Err(DecodeError::BadGroup)
        );
    }

    fn mixed3(rng: &mut Rng) -> [i32; 3] {
        let mode = rng.next() % 5;
        let gen_v = |rng: &mut Rng, bits: u32| {
            let max = 1i64 << (bits - 1);
            ((rng.next() % (2 * max as u64)) as i64 - max) as i32
        };
        match mode {
            0 => [gen_v(rng, 2), gen_v(rng, 2), gen_v(rng, 2)],
            1 => [gen_v(rng, 4), gen_v(rng, 4), gen_v(rng, 4)],
            2 => [gen_v(rng, 6), gen_v(rng, 6), gen_v(rng, 6)],
            3 => [gen_v(rng, 8), gen_v(rng, 7), gen_v(rng, 7)],
            _ => [rng.i32(), rng.i32(), rng.i32()],
        }
    }

    #[test]
    fn tag2_3s32_roundtrip() {
        let mut rng = Rng(7);
        for _ in 0..20_000 {
            let v = mixed3(&mut rng);
            let mut buf = Vec::new();
            write_tag2_3s32(&mut buf, v);
            assert_eq!(
                read_tag2_3s32(&mut Reader::new(&buf)),
                Ok(v),
                "{v:?} {buf:?}"
            );
        }
    }

    #[test]
    fn tag2_3svariable_roundtrip() {
        let mut rng = Rng(11);
        for _ in 0..20_000 {
            let v = mixed3(&mut rng);
            let mut buf = Vec::new();
            write_tag2_3svariable(&mut buf, v);
            assert_eq!(
                read_tag2_3svariable(&mut Reader::new(&buf)),
                Ok(v),
                "{v:?} {buf:?}"
            );
        }
    }

    #[test]
    fn tag8_4s16_v2_roundtrip() {
        let mut rng = Rng(99);
        for _ in 0..20_000 {
            let gen_v = |rng: &mut Rng| match rng.next() % 4 {
                0 => 0,
                1 => (rng.next() % 16) as i32 - 8,
                2 => (rng.next() % 256) as i32 - 128,
                _ => (rng.next() % 65536) as i32 - 32768,
            };
            let v = [
                gen_v(&mut rng),
                gen_v(&mut rng),
                gen_v(&mut rng),
                gen_v(&mut rng),
            ];
            let mut buf = Vec::new();
            write_tag8_4s16(&mut buf, v);
            assert_eq!(
                read_tag8_4s16(&mut Reader::new(&buf), 2),
                Ok(v),
                "{v:?} {buf:?}"
            );
        }
    }

    #[test]
    fn tag8_4s16_v1_layout() {
        // selector: field0 = 16-bit (3), field1 = 8-bit (2), field2 = 4-bit (1), field3 = 4-bit (1)
        let sel = 0b01_01_10_11u8;
        let bytes = [sel, 0x34, 0x12, 0xFE, 0x7A];
        let got = read_tag8_4s16(&mut Reader::new(&bytes), 1).unwrap();
        assert_eq!(got, [0x1234, -2, -6, 7]);
    }

    #[test]
    fn elias_delta_examples_from_spec() {
        let cases: [(u32, &str); 12] = [
            (0, "1"),
            (1, "0100"),
            (2, "0101"),
            (3, "01100"),
            (7, "00100000"),
            (14, "00100111"),
            (15, "001010000"),
            (225, "00010001100010"),
            (4_294_967_292, "000001000001111111111111111111111111111101"),
            (4_294_967_293, "000001000001111111111111111111111111111110"),
            (4_294_967_294, "0000010000011111111111111111111111111111110"),
            (4_294_967_295, "0000010000011111111111111111111111111111111"),
        ];
        for (value, bits) in cases {
            let mut w = BitWriter::default();
            w.write_elias_delta_u32(value);
            let mut s = String::new();
            for i in 0..bits.len() {
                let b = w.bytes[i / 8] >> (7 - i % 8) & 1;
                s.push(if b == 1 { '1' } else { '0' });
            }
            assert_eq!(s, bits, "{value}");
            assert_eq!(read_elias_delta_u32(&mut Reader::new(&w.bytes)), Ok(value));
        }
        let mut rng = Rng(3);
        for _ in 0..5_000 {
            let v = rng.i32();
            let mut w = BitWriter::default();
            w.write_elias_delta_s32(v);
            w.write_elias_delta_s32(-v);
            let mut r = Reader::new(&w.bytes);
            assert_eq!(read_elias_delta_s32(&mut r), Ok(v));
            assert_eq!(read_elias_delta_s32(&mut r), Ok(-v));
        }
    }
}
