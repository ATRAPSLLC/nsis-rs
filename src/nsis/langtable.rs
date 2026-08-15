//! NSIS language table structure.
//!
//! Language tables provide localized strings. Each table contains a language ID
//! and an array of string offsets into the main string table.
//!
//! The `langtable_size` field from the common header gives the byte size of
//! each language table entry.

use crate::{
    error::Error,
    util::{read_i32_le, read_u16_le},
};

/// View type for an NSIS language table entry.
///
/// The layout is variable-sized, determined by `langtable_size` from the
/// common header.
///
/// # Layout
///
/// | Offset | Field | Description |
/// |--------|-------|-------------|
/// | 0x00 | `lang_id` | Windows LANGID (u16) |
/// | 0x02 | (padding) | 2 bytes padding to align |
/// | 0x04 | `dlg_offset` | Dialog string offset (i32) |
/// | 0x08+ | `string_ptrs[]` | Variable-length array of string table offsets |
#[derive(Debug, Clone)]
pub struct LangTable<'a> {
    bytes: &'a [u8],
    entry_size: usize,
}

impl<'a> LangTable<'a> {
    /// Minimum size of a language table entry (lang_id + padding + dlg_offset).
    pub const MIN_SIZE: usize = 8;

    /// Parses a language table entry of the given size.
    ///
    /// # Arguments
    ///
    /// - `data`: bytes starting at the language table entry
    /// - `entry_size`: the size of each entry (from `CommonHeader::langtable_size()`)
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < entry_size`.
    pub fn parse(data: &'a [u8], entry_size: usize) -> Result<Self, Error> {
        let size = entry_size.max(Self::MIN_SIZE);
        if data.len() < size {
            return Err(Error::TooShort {
                expected: size,
                actual: data.len(),
                context: "LangTable",
            });
        }
        Ok(Self {
            bytes: data.get(..size).ok_or(Error::TooShort {
                expected: size,
                actual: data.len(),
                context: "LangTable",
            })?,
            entry_size: size,
        })
    }

    /// Returns the Windows language identifier (LANGID).
    #[inline]
    pub fn lang_id(&self) -> u16 {
        read_u16_le(self.bytes, 0)
    }

    /// Returns the dialog string offset.
    #[inline]
    pub fn dlg_offset(&self) -> i32 {
        read_i32_le(self.bytes, 4)
    }

    /// Returns the string table offset at the given index in the string pointer array.
    ///
    /// Returns `None` if the index is out of range for this language table size.
    pub fn string_ptr(&self, index: usize) -> Option<i32> {
        let offset = index.checked_mul(4)?.checked_add(Self::MIN_SIZE)?;
        let end = offset.checked_add(4)?;
        if end <= self.entry_size {
            Some(read_i32_le(self.bytes, offset))
        } else {
            None
        }
    }

    /// Returns the number of string pointers in this language table.
    #[inline]
    pub fn string_count(&self) -> usize {
        self.entry_size.saturating_sub(Self::MIN_SIZE) / 4
    }

    /// Returns the byte size of this language table entry.
    #[inline]
    pub fn entry_size(&self) -> usize {
        self.entry_size
    }
}

/// Iterator over language tables in the language table block.
#[derive(Debug)]
pub struct LangTableIter<'a> {
    data: &'a [u8],
    remaining: usize,
    offset: usize,
    entry_size: usize,
}

impl<'a> LangTableIter<'a> {
    /// Creates a new language table iterator.
    ///
    /// `count` is the number of language tables, and `entry_size` is the
    /// per-entry byte size from the common header.
    pub fn new(data: &'a [u8], count: usize, entry_size: usize) -> Self {
        Self {
            data,
            remaining: count,
            offset: 0,
            entry_size,
        }
    }
}

impl<'a> Iterator for LangTableIter<'a> {
    type Item = Result<LangTable<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining = self.remaining.saturating_sub(1);
        let Some(slice) = self.data.get(self.offset..) else {
            return Some(Err(Error::TooShort {
                expected: self.offset.saturating_add(self.entry_size),
                actual: self.data.len(),
                context: "LangTable",
            }));
        };
        let result = LangTable::parse(slice, self.entry_size);
        self.offset = self.offset.saturating_add(self.entry_size);
        Some(result)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for LangTableIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lang_table(lang_id: u16, dlg_offset: i32, strings: &[i32]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&lang_id.to_le_bytes());
        buf.extend_from_slice(&[0u8; 2]); // padding
        buf.extend_from_slice(&dlg_offset.to_le_bytes());
        for &s in strings {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        buf
    }

    #[test]
    fn parse_valid() {
        let data = make_lang_table(1033, 100, &[200, 300, 400]);
        let lt = LangTable::parse(&data, data.len()).unwrap();
        assert_eq!(lt.lang_id(), 1033); // English
        assert_eq!(lt.dlg_offset(), 100);
        assert_eq!(lt.string_count(), 3);
        assert_eq!(lt.string_ptr(0), Some(200));
        assert_eq!(lt.string_ptr(1), Some(300));
        assert_eq!(lt.string_ptr(2), Some(400));
        assert_eq!(lt.string_ptr(3), None);
    }

    #[test]
    fn parse_too_short() {
        let data = [0u8; 7];
        assert!(LangTable::parse(&data, 8).is_err());
    }

    #[test]
    fn iterator_count() {
        let lt1 = make_lang_table(1033, 0, &[10, 20]);
        let lt2 = make_lang_table(1031, 0, &[30, 40]);
        let entry_size = lt1.len();
        let mut data = Vec::new();
        data.extend_from_slice(&lt1);
        data.extend_from_slice(&lt2);
        let iter = LangTableIter::new(&data, 2, entry_size);
        assert_eq!(iter.len(), 2);
        let tables: Vec<_> = iter.collect();
        assert_eq!(tables[0].as_ref().unwrap().lang_id(), 1033);
        assert_eq!(tables[1].as_ref().unwrap().lang_id(), 1031);
    }
}
