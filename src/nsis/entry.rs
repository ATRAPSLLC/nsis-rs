//! NSIS entry (instruction) structure.
//!
//! Each entry represents a single bytecode instruction in the NSIS script.
//! An entry consists of an opcode (`which`) and 6 parameter offsets.
//!
//! Source: `fileform.h` from the NSIS source code.

use crate::{error::Error, util::read_i32_le};

/// Maximum number of parameter offsets per entry.
pub const MAX_ENTRY_OFFSETS: usize = 6;

/// View type for an NSIS entry/instruction (28 bytes).
///
/// Per Malcat: "Every instruction is encoded on 7 DWORDs: first DWORD is
/// for the opcode (about 70 different opcodes) and the other 6 DWORD encode
/// arguments."
///
/// # Layout (7 x i32, little-endian)
///
/// | Offset | Field | Description |
/// |--------|-------|-------------|
/// | 0x00 | `which` | `EW_*` opcode index |
/// | 0x04 | `offsets[0]` | Parameter 0 |
/// | 0x08 | `offsets[1]` | Parameter 1 |
/// | 0x0C | `offsets[2]` | Parameter 2 |
/// | 0x10 | `offsets[3]` | Parameter 3 |
/// | 0x14 | `offsets[4]` | Parameter 4 |
/// | 0x18 | `offsets[5]` | Parameter 5 |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    bytes: &'a [u8],
}

impl<'a> Entry<'a> {
    /// The on-disk size of an entry in bytes, in NSIS 2.x and 3.x.
    pub const SIZE: usize = 28;

    /// The on-disk size of an entry in bytes, in NSIS 1.x.
    ///
    /// 1.x gives an instruction five parameter slots where later versions give
    /// six (`MAX_ENTRY_OFFSETS` in its `Source/exehead/fileform.h`), so its
    /// entries are one `int` shorter.
    pub const V1_SIZE: usize = 24;

    /// Parses an entry from the start of `data`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 28`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        Self::parse_sized(data, Self::SIZE)
    }

    /// Parses an entry of `size` bytes from the start of `data`.
    ///
    /// The sixth parameter of an [`V1_SIZE`](Self::V1_SIZE) entry reads as `0`,
    /// which is what it means: 1.x instructions have five parameters, and the
    /// opcode table gives none of them a sixth.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` is shorter than `size`.
    pub fn parse_sized(data: &'a [u8], size: usize) -> Result<Self, Error> {
        Ok(Self {
            bytes: data.get(..size).ok_or(Error::TooShort {
                expected: size,
                actual: data.len(),
                context: "Entry",
            })?,
        })
    }

    /// Returns the opcode index (`EW_*`).
    #[inline]
    pub fn which(&self) -> i32 {
        read_i32_le(self.bytes, 0)
    }

    /// Returns the parameter at the given index (0..5).
    ///
    /// Returns `0` if `index >= 6`.
    #[inline]
    pub fn offset(&self, index: usize) -> i32 {
        if index >= MAX_ENTRY_OFFSETS {
            return 0;
        }
        // index < MAX_ENTRY_OFFSETS = 6, so 4 + 4*index <= 24, no overflow.
        read_i32_le(self.bytes, 4_usize.saturating_add(index.saturating_mul(4)))
    }

    /// Returns all 6 parameter offsets.
    ///
    /// The sixth is always `0` for an NSIS 1.x entry, which stores five.
    #[inline]
    pub fn offsets(&self) -> [i32; MAX_ENTRY_OFFSETS] {
        [
            read_i32_le(self.bytes, 4),
            read_i32_le(self.bytes, 8),
            read_i32_le(self.bytes, 12),
            read_i32_le(self.bytes, 16),
            read_i32_le(self.bytes, 20),
            read_i32_le(self.bytes, 24),
        ]
    }
}

/// Iterator over NSIS entries in the entry block.
#[derive(Debug)]
pub struct EntryIter<'a> {
    data: &'a [u8],
    remaining: usize,
    offset: usize,
    stride: usize,
}

impl<'a> EntryIter<'a> {
    /// Creates a new entry iterator over the entry block data.
    ///
    /// `count` is the number of entries (from the block header's `num` field).
    pub fn new(data: &'a [u8], count: usize) -> Self {
        Self::with_stride(data, count, Entry::SIZE)
    }

    /// Creates an iterator over entries of `stride` bytes each.
    ///
    /// Use [`Entry::V1_SIZE`] for an NSIS 1.x entry block.
    pub fn with_stride(data: &'a [u8], count: usize, stride: usize) -> Self {
        Self {
            data,
            remaining: count,
            offset: 0,
            stride: stride.max(1),
        }
    }
}

impl<'a> Iterator for EntryIter<'a> {
    type Item = Result<Entry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining = self.remaining.saturating_sub(1);
        let slice = self.data.get(self.offset..).unwrap_or(&[]);
        let result = Entry::parse_sized(slice, self.stride);
        self.offset = self.offset.saturating_add(self.stride);
        Some(result)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for EntryIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(which: i32, offsets: [i32; 6]) -> [u8; 28] {
        let mut buf = [0u8; 28];
        buf[0..4].copy_from_slice(&which.to_le_bytes());
        for (i, &val) in offsets.iter().enumerate() {
            let start = 4 + i * 4;
            buf[start..start + 4].copy_from_slice(&val.to_le_bytes());
        }
        buf
    }

    #[test]
    fn parse_valid() {
        let buf = make_entry(20, [1, 100, 200, 0, 0, 0]);
        let e = Entry::parse(&buf).unwrap();
        assert_eq!(e.which(), 20); // EW_EXTRACTFILE
        assert_eq!(e.offset(0), 1);
        assert_eq!(e.offset(1), 100);
        assert_eq!(e.offset(2), 200);
        assert_eq!(e.offsets(), [1, 100, 200, 0, 0, 0]);
    }

    #[test]
    fn parse_too_short() {
        let buf = [0u8; 27];
        assert!(Entry::parse(&buf).is_err());
    }

    #[test]
    fn offset_out_of_range_returns_zero() {
        let buf = make_entry(0, [0; 6]);
        let e = Entry::parse(&buf).unwrap();
        assert_eq!(e.offset(6), 0);
        assert_eq!(e.offset(100), 0);
    }

    #[test]
    fn iterator_yields_correct_count() {
        let e1 = make_entry(1, [0; 6]); // EW_RET
        let e2 = make_entry(2, [10, 0, 0, 0, 0, 0]); // EW_NOP
        let mut data = Vec::new();
        data.extend_from_slice(&e1);
        data.extend_from_slice(&e2);

        let iter = EntryIter::new(&data, 2);
        assert_eq!(iter.len(), 2);
        let entries: Vec<_> = iter.collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].as_ref().unwrap().which(), 1);
        assert_eq!(entries[1].as_ref().unwrap().which(), 2);
    }

    #[test]
    fn entry_is_copy() {
        let buf = make_entry(5, [1, 2, 3, 4, 5, 6]);
        let e1 = Entry::parse(&buf).unwrap();
        let e2 = e1;
        assert_eq!(e1, e2);
    }
}
