//! NSIS install section structure.
//!
//! Each section describes a logical group of installation operations.
//! Sections reference a range of entries (bytecode instructions) and
//! carry metadata like name, flags, and estimated disk usage.
//!
//! The on-disk section size varies: the base is 24 bytes (6 x i32), but
//! most NSIS builds append an inline `name[NSIS_MAX_STRLEN]` buffer.
//! The actual size is computed from the gap between the Sections and
//! Entries block offsets.
//!
//! Source: `fileform.h`, 7-Zip `NsisIn.cpp` (line 5260).

use crate::{
    error::Error,
    util::{read_i32_le, read_u32_le},
};

/// Section is selected by default.
pub const SF_SELECTED: u32 = 1;
/// Section group begin marker.
pub const SF_SECGRP: u32 = 2;
/// Section group end marker.
pub const SF_SECGRPEND: u32 = 4;
/// Bold text in component list.
pub const SF_BOLD: u32 = 8;
/// Read-only (cannot be deselected).
pub const SF_RO: u32 = 16;
/// Expanded by default (for groups).
pub const SF_EXPAND: u32 = 32;
/// Partially selected.
pub const SF_PSELECTED: u32 = 64;
/// Toggled state.
pub const SF_TOGGLED: u32 = 128;
/// Name was changed at runtime.
pub const SF_NAMECHG: u32 = 256;

/// View type for an NSIS section descriptor.
///
/// The base layout is 24 bytes (6 x i32), but the actual on-disk size
/// is larger when an inline name buffer is present (see [`inline_name`](Self::inline_name)).
///
/// # Base layout
///
/// | Offset | Field | Description |
/// |--------|-------|-------------|
/// | 0x00 | `name_ptr` | String table offset for section name |
/// | 0x04 | `install_types` | Bitmask of install types |
/// | 0x08 | `flags` | `SF_*` flags |
/// | 0x0C | `code` | Entry index where section code starts |
/// | 0x10 | `code_size` | Number of entries in this section |
/// | 0x14 | `size_kb` | Estimated disk space in KB |
/// | 0x18+ | `name[]` | Inline name buffer (variable length) |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section<'a> {
    bytes: &'a [u8],
    is_unicode: bool,
}

impl<'a> Section<'a> {
    /// Minimum on-disk size of a section (the 6 base i32 fields).
    pub const BASE_SIZE: usize = 24;

    /// Parses a section from the start of `data`.
    ///
    /// `section_size` is the full on-disk size of each section entry,
    /// computed from the block header offsets. `is_unicode` indicates
    /// whether the inline name buffer uses UTF-16LE encoding.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < section_size`.
    pub fn parse(data: &'a [u8], section_size: usize, is_unicode: bool) -> Result<Self, Error> {
        let size = section_size.max(Self::BASE_SIZE);
        if data.len() < size {
            return Err(Error::TooShort {
                expected: size,
                actual: data.len(),
                context: "Section",
            });
        }
        Ok(Self {
            bytes: data.get(..size).ok_or(Error::TooShort {
                expected: size,
                actual: data.len(),
                context: "Section",
            })?,
            is_unicode,
        })
    }

    /// Returns the string table offset for the section name.
    #[inline]
    pub fn name_ptr(&self) -> i32 {
        read_i32_le(self.bytes, 0)
    }

    /// Returns the install types bitmask.
    #[inline]
    pub fn install_types(&self) -> u32 {
        read_u32_le(self.bytes, 4)
    }

    /// Returns the section flags (`SF_*`).
    #[inline]
    pub fn flags(&self) -> u32 {
        read_u32_le(self.bytes, 8)
    }

    /// Returns the entry index where this section's code starts.
    #[inline]
    pub fn code(&self) -> i32 {
        read_i32_le(self.bytes, 12)
    }

    /// Returns the number of entries (instructions) in this section.
    #[inline]
    pub fn code_size(&self) -> i32 {
        read_i32_le(self.bytes, 16)
    }

    /// Returns the estimated disk space usage in kilobytes.
    #[inline]
    pub fn size_kb(&self) -> i32 {
        read_i32_le(self.bytes, 20)
    }

    /// Returns the inline section name, if present.
    ///
    /// Most NSIS builds include a `name[NSIS_MAX_STRLEN]` buffer after the
    /// base 24-byte fields. This buffer contains the section name as a
    /// null-terminated string (ANSI or UTF-16LE depending on the build).
    ///
    /// Returns `None` if the section has no inline name buffer (24-byte
    /// base layout only), or if the name is empty.
    pub fn inline_name(&self) -> Option<String> {
        if self.bytes.len() <= Self::BASE_SIZE {
            return None;
        }
        let name_buf = self.bytes.get(Self::BASE_SIZE..)?;
        if self.is_unicode {
            // UTF-16LE null-terminated
            let mut chars = Vec::new();
            for chunk in name_buf.chunks_exact(2) {
                let Some(pair) = chunk.first_chunk::<2>() else {
                    break;
                };
                let ch = u16::from_le_bytes(*pair);
                if ch == 0 {
                    break;
                }
                chars.push(ch);
            }
            let s = String::from_utf16_lossy(&chars);
            if s.is_empty() { None } else { Some(s) }
        } else {
            // ANSI null-terminated
            let end = name_buf
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(name_buf.len());
            let s = String::from_utf8_lossy(name_buf.get(..end).unwrap_or(&[])).into_owned();
            if s.is_empty() { None } else { Some(s) }
        }
    }

    /// Returns `true` if this section is selected by default.
    #[inline]
    pub fn is_selected(&self) -> bool {
        self.flags() & SF_SELECTED != 0
    }

    /// Returns `true` if this is a section group begin marker.
    #[inline]
    pub fn is_section_group(&self) -> bool {
        self.flags() & SF_SECGRP != 0
    }

    /// Returns `true` if this is a section group end marker.
    #[inline]
    pub fn is_section_group_end(&self) -> bool {
        self.flags() & SF_SECGRPEND != 0
    }

    /// Returns `true` if the section name is bold in the component list.
    #[inline]
    pub fn is_bold(&self) -> bool {
        self.flags() & SF_BOLD != 0
    }

    /// Returns `true` if the section is read-only (cannot be deselected).
    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.flags() & SF_RO != 0
    }

    /// Returns `true` if the section group is expanded by default.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.flags() & SF_EXPAND != 0
    }

    /// Returns `true` if `entry_idx` falls within this section's code range.
    ///
    /// The range is `[code, code + code_size)`. Negative values for
    /// [`code`](Self::code) or [`code_size`](Self::code_size) — which the
    /// raw common header may report — are treated as zero, so a section
    /// with no code never contains any entry.
    pub fn contains_entry(&self, entry_idx: usize) -> bool {
        let start = self.code().max(0) as usize;
        let len = self.code_size().max(0) as usize;
        let Some(end) = start.checked_add(len) else {
            return false;
        };
        entry_idx >= start && entry_idx < end
    }
}

/// Iterator over NSIS sections in a section block.
#[derive(Debug)]
pub struct SectionIter<'a> {
    data: &'a [u8],
    remaining: usize,
    offset: usize,
    section_size: usize,
    is_unicode: bool,
}

impl<'a> SectionIter<'a> {
    /// Creates a new section iterator.
    ///
    /// `section_size` is the stride between entries (computed from block offsets).
    /// `is_unicode` indicates whether inline name buffers use UTF-16LE.
    pub fn new(data: &'a [u8], count: usize, section_size: usize, is_unicode: bool) -> Self {
        Self {
            data,
            remaining: count,
            offset: 0,
            section_size,
            is_unicode,
        }
    }
}

impl<'a> Iterator for SectionIter<'a> {
    type Item = Result<Section<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining = self.remaining.saturating_sub(1);
        let Some(slice) = self.data.get(self.offset..) else {
            return Some(Err(Error::TooShort {
                expected: self.offset.saturating_add(self.section_size),
                actual: self.data.len(),
                context: "Section",
            }));
        };
        let result = Section::parse(slice, self.section_size, self.is_unicode);
        self.offset = self.offset.saturating_add(self.section_size);
        Some(result)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for SectionIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_section(name_ptr: i32, flags: u32, code: i32, code_size: i32) -> [u8; 24] {
        let mut buf = [0u8; 24];
        buf[0..4].copy_from_slice(&name_ptr.to_le_bytes());
        buf[4..8].copy_from_slice(&0u32.to_le_bytes());
        buf[8..12].copy_from_slice(&flags.to_le_bytes());
        buf[12..16].copy_from_slice(&code.to_le_bytes());
        buf[16..20].copy_from_slice(&code_size.to_le_bytes());
        buf[20..24].copy_from_slice(&0i32.to_le_bytes());
        buf
    }

    #[test]
    fn parse_valid() {
        let buf = make_section(100, SF_SELECTED | SF_BOLD, 0, 10);
        let s = Section::parse(&buf, 24, false).unwrap();
        assert_eq!(s.name_ptr(), 100);
        assert!(s.is_selected());
        assert!(s.is_bold());
        assert!(!s.is_read_only());
        assert_eq!(s.code(), 0);
        assert_eq!(s.code_size(), 10);
    }

    #[test]
    fn parse_too_short() {
        let buf = [0u8; 23];
        assert!(Section::parse(&buf, 24, false).is_err());
    }

    #[test]
    fn section_group_flags() {
        let buf = make_section(0, SF_SECGRP, 0, 0);
        let s = Section::parse(&buf, 24, false).unwrap();
        assert!(s.is_section_group());
        assert!(!s.is_section_group_end());
    }

    #[test]
    fn inline_name_ansi() {
        let mut buf = vec![0u8; 24 + 1024];
        buf[0..4].copy_from_slice(&0i32.to_le_bytes());
        buf[24..29].copy_from_slice(b"Hello");
        let s = Section::parse(&buf, buf.len(), false).unwrap();
        assert_eq!(s.inline_name(), Some("Hello".into()));
    }

    #[test]
    fn inline_name_unicode() {
        let mut buf = vec![0u8; 24 + 2048];
        // "Hi" in UTF-16LE at offset 24
        buf[24] = 0x48;
        buf[25] = 0x00;
        buf[26] = 0x69;
        buf[27] = 0x00;
        let s = Section::parse(&buf, buf.len(), true).unwrap();
        assert_eq!(s.inline_name(), Some("Hi".into()));
    }

    #[test]
    fn inline_name_empty() {
        let buf = vec![0u8; 24 + 1024];
        let s = Section::parse(&buf, buf.len(), false).unwrap();
        assert_eq!(s.inline_name(), None);
    }

    #[test]
    fn iterator_with_stride() {
        // 2 sections with section_size=32 (24 base + 8 extra)
        let mut data = vec![0u8; 64];
        // code_size is at offset 16 within each section
        data[16..20].copy_from_slice(&5i32.to_le_bytes());
        data[32 + 16..32 + 20].copy_from_slice(&3i32.to_le_bytes());
        let iter = SectionIter::new(&data, 2, 32, false);
        assert_eq!(iter.len(), 2);
        let sections: Vec<_> = iter.collect();
        assert_eq!(sections[0].as_ref().unwrap().code_size(), 5);
        assert_eq!(sections[1].as_ref().unwrap().code_size(), 3);
    }

    #[test]
    fn iterator_empty() {
        let iter = SectionIter::new(&[], 0, 24, false);
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn contains_entry_basic() {
        let buf = make_section(0, 0, 100, 5);
        let s = Section::parse(&buf, 24, false).unwrap();
        assert!(!s.contains_entry(99));
        assert!(s.contains_entry(100));
        assert!(s.contains_entry(104));
        assert!(!s.contains_entry(105));
    }

    #[test]
    fn contains_entry_empty_section() {
        let buf = make_section(0, 0, 100, 0);
        let s = Section::parse(&buf, 24, false).unwrap();
        assert!(!s.contains_entry(100));
    }

    #[test]
    fn contains_entry_negative_code_size() {
        let buf = make_section(0, 0, 0, -1);
        let s = Section::parse(&buf, 24, false).unwrap();
        assert!(!s.contains_entry(0));
    }
}
