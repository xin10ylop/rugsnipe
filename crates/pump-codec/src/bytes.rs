//! Minimal little-endian / borsh cursor over a borrowed byte slice.
//!
//! Every read is bounds-checked and returns [`DecodeError`] instead of
//! panicking. Nothing allocates: strings and vectors are returned as borrowed
//! slices of the input.

use crate::error::{DecodeError, Result};
use solana_pubkey::Pubkey;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    #[inline]
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    #[inline]
    pub(crate) fn rest(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    #[inline]
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(DecodeError::UnexpectedEof {
                offset: self.pos,
                needed: n - self.remaining(),
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    #[inline]
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(s);
        Ok(out)
    }

    #[inline]
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    #[inline]
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn u128(&mut self) -> Result<u128> {
        Ok(u128::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn i128(&mut self) -> Result<i128> {
        Ok(i128::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn bool(&mut self) -> Result<bool> {
        let offset = self.pos;
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(DecodeError::InvalidBool { offset, value }),
        }
    }

    #[inline]
    pub(crate) fn pubkey(&mut self) -> Result<Pubkey> {
        Ok(Pubkey::new_from_array(self.array()?))
    }

    /// Borsh `String`: u32 LE length + UTF-8 bytes.
    #[inline]
    pub(crate) fn string(&mut self) -> Result<&'a str> {
        let len = self.u32()? as usize;
        let offset = self.pos;
        let bytes = self.take(len)?;
        core::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8 { offset })
    }

    /// Anchor 8-byte discriminator check.
    #[inline]
    pub(crate) fn expect_discriminator(&mut self, expected: &[u8; 8]) -> Result<()> {
        let found: [u8; 8] = self.array()?;
        if &found != expected {
            return Err(DecodeError::BadDiscriminator {
                expected: *expected,
                found,
            });
        }
        Ok(())
    }

    // Optional trailing fields: `None` only when the buffer is exhausted;
    // a partially present field is an error.
    #[inline]
    pub(crate) fn opt_u64(&mut self) -> Result<Option<u64>> {
        if self.is_empty() {
            Ok(None)
        } else {
            self.u64().map(Some)
        }
    }
    #[inline]
    pub(crate) fn opt_i64(&mut self) -> Result<Option<i64>> {
        if self.is_empty() {
            Ok(None)
        } else {
            self.i64().map(Some)
        }
    }
    #[inline]
    pub(crate) fn opt_i128(&mut self) -> Result<Option<i128>> {
        if self.is_empty() {
            Ok(None)
        } else {
            self.i128().map(Some)
        }
    }
    #[inline]
    pub(crate) fn opt_u8(&mut self) -> Result<Option<u8>> {
        if self.is_empty() {
            Ok(None)
        } else {
            self.u8().map(Some)
        }
    }
    #[inline]
    pub(crate) fn opt_bool(&mut self) -> Result<Option<bool>> {
        if self.is_empty() {
            Ok(None)
        } else {
            self.bool().map(Some)
        }
    }
    #[inline]
    pub(crate) fn opt_pubkey(&mut self) -> Result<Option<Pubkey>> {
        if self.is_empty() {
            Ok(None)
        } else {
            self.pubkey().map(Some)
        }
    }
    #[inline]
    pub(crate) fn opt_string(&mut self) -> Result<Option<&'a str>> {
        if self.is_empty() {
            Ok(None)
        } else {
            self.string().map(Some)
        }
    }
}

/// Read the 8-byte discriminator at the start of `data`, if present.
#[inline]
pub fn discriminator(data: &[u8]) -> Option<[u8; 8]> {
    data.get(..8).map(|d| {
        let mut out = [0u8; 8];
        out.copy_from_slice(d);
        out
    })
}

/// Solana compact-u16 ("short_vec") decode. Returns (value, bytes consumed).
#[inline]
pub(crate) fn read_compact_u16(buf: &[u8], offset: usize) -> Result<(u16, usize)> {
    let mut value: u32 = 0;
    for i in 0..3 {
        let b = *buf.get(offset + i).ok_or(DecodeError::UnexpectedEof {
            offset: offset + i,
            needed: 1,
        })?;
        value |= ((b & 0x7f) as u32) << (7 * i);
        if b & 0x80 == 0 {
            // Reject non-canonical encodings (trailing zero continuation byte).
            if i > 0 && b == 0 {
                return Err(DecodeError::InvalidTransaction("non-canonical compact-u16"));
            }
            if value > u16::MAX as u32 {
                return Err(DecodeError::InvalidTransaction("compact-u16 overflow"));
            }
            return Ok((value as u16, i + 1));
        }
    }
    Err(DecodeError::InvalidTransaction(
        "compact-u16 longer than 3 bytes",
    ))
}
