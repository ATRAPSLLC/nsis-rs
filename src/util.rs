//! Low-level byte-reading utilities for little-endian structure access.
//!
//! These functions use checked indexing to avoid panics, returning zero when
//! the offset is out of bounds.
//!
//! # The contract these rely on
//!
//! Returning zero for a short read is safe only because it never happens: every
//! view type validates its length in `parse` and stores a slice trimmed to
//! exactly the structure size, so its accessors read within bounds by
//! construction. Each has a `parse_too_short` test holding that up.
//!
//! A new view type must do the same. Reading a truncated structure through
//! these helpers would not fail - it would report a field full of zeros, which
//! is indistinguishable from a real value and is exactly the kind of quiet
//! wrongness this crate exists to avoid.

use core::{fmt, ops::Deref};

/// Byte count up to which [`Blob`]'s `Debug` output shows every byte.
const BLOB_DEBUG_FULL: usize = 32;

/// Number of leading bytes [`Blob`]'s `Debug` output shows for a longer blob.
const BLOB_DEBUG_PREVIEW: usize = 16;

/// Wraps a byte buffer so a derived `Debug` prints a summary, not every byte.
///
/// Without it, deriving `Debug` on a type that holds the installer file or a
/// decompressed header prints hundreds of kilobytes of decimal numbers. The
/// wrapper prints the length and a short hex preview instead, so small
/// records (an entry, a block header) stay fully visible and large buffers
/// stay readable.
///
/// `B` is `&[u8]` for borrowed views and `Vec<u8>` for owned buffers.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct Blob<B>(pub B);

impl<B: AsRef<[u8]>> Deref for Blob<B> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<B: AsRef<[u8]>> fmt::Debug for Blob<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0.as_ref();
        write!(f, "Blob({} bytes", bytes.len())?;
        let shown = if bytes.len() <= BLOB_DEBUG_FULL {
            bytes
        } else {
            bytes.get(..BLOB_DEBUG_PREVIEW).unwrap_or(bytes)
        };
        for (i, byte) in shown.iter().enumerate() {
            let sep = if i == 0 { ": " } else { " " };
            write!(f, "{sep}{byte:02x}")?;
        }
        if shown.len() < bytes.len() {
            f.write_str(" ..")?;
        }
        f.write_str(")")
    }
}

/// Reads a little-endian `u16` from `data` at the given byte `offset`.
///
/// Returns `0` if `offset + 2 > data.len()`.
#[inline(always)]
pub(crate) fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    data.get(offset..)
        .and_then(|s| s.first_chunk::<2>())
        .copied()
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

/// Reads a little-endian `u32` from `data` at the given byte `offset`.
///
/// Returns `0` if `offset + 4 > data.len()`.
#[inline(always)]
pub(crate) fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    data.get(offset..)
        .and_then(|s| s.first_chunk::<4>())
        .copied()
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

/// Reads a little-endian `i32` from `data` at the given byte `offset`.
///
/// Returns `0` if `offset + 4 > data.len()`.
#[inline(always)]
pub(crate) fn read_i32_le(data: &[u8], offset: usize) -> i32 {
    data.get(offset..)
        .and_then(|s| s.first_chunk::<4>())
        .copied()
        .map(i32::from_le_bytes)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16_le() {
        assert_eq!(read_u16_le(&[0x34, 0x12], 0), 0x1234);
    }

    #[test]
    fn test_read_u16_le_offset() {
        assert_eq!(read_u16_le(&[0x00, 0x34, 0x12], 1), 0x1234);
    }

    #[test]
    fn test_read_u32_le() {
        assert_eq!(read_u32_le(&[0x78, 0x56, 0x34, 0x12], 0), 0x12345678);
    }

    #[test]
    fn test_read_u32_le_offset() {
        assert_eq!(read_u32_le(&[0xFF, 0x78, 0x56, 0x34, 0x12], 1), 0x12345678);
    }

    #[test]
    fn test_read_i32_le_positive() {
        assert_eq!(read_i32_le(&[0x01, 0x00, 0x00, 0x00], 0), 1);
    }

    #[test]
    fn test_read_i32_le_negative() {
        assert_eq!(read_i32_le(&[0xFF, 0xFF, 0xFF, 0xFF], 0), -1);
    }

    #[test]
    fn test_read_u16_le_out_of_bounds() {
        assert_eq!(read_u16_le(&[0x00], 0), 0);
    }

    #[test]
    fn test_read_u32_le_out_of_bounds() {
        assert_eq!(read_u32_le(&[0x00, 0x01, 0x02], 0), 0);
    }

    #[test]
    fn test_read_i32_le_out_of_bounds() {
        assert_eq!(read_i32_le(&[], 0), 0);
    }

    #[test]
    fn test_blob_debug_short_shows_every_byte() {
        let bytes = [0x21, 0x00, 0xff];
        assert_eq!(format!("{:?}", Blob(&bytes[..])), "Blob(3 bytes: 21 00 ff)");
    }

    #[test]
    fn test_blob_debug_empty() {
        assert_eq!(format!("{:?}", Blob(&[][..])), "Blob(0 bytes)");
    }

    #[test]
    fn test_blob_debug_at_limit_shows_every_byte() {
        let debug = format!("{:?}", Blob(vec![0xab; BLOB_DEBUG_FULL]));
        assert_eq!(debug.matches("ab").count(), BLOB_DEBUG_FULL);
        assert!(!debug.contains(".."));
    }

    #[test]
    fn test_blob_debug_long_shows_preview() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(
            format!("{:?}", Blob(bytes)),
            "Blob(256 bytes: 00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f ..)"
        );
    }
}
