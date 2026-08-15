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
//! these helpers would not fail — it would report a field full of zeros, which
//! is indistinguishable from a real value and is exactly the kind of quiet
//! wrongness this crate exists to avoid.

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
}
