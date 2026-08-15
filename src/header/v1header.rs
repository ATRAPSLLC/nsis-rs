//! The NSIS 1.x install header.
//!
//! NSIS 1.x predates the block table. Where 2.x and 3.x open their header with
//! flags and eight [`BlockHeader`](super::BlockHeader) descriptors that say
//! where each table lives, 1.x writes a fixed 240-byte struct followed by the
//! tables laid end to end:
//!
//! ```text
//! 0                240            240+n*20        +m*24
//! ├─ V1Header ─────┼─ sections ───┼─ entries ─────┼─ string table ─┤
//! ```
//!
//! There is nothing to look up: the tables are found by adding up the sizes,
//! which is why [`block_layout`](V1Header::block_layout) can hand the rest of
//! the crate the same `(offset, count)` pairs a 2.x block table would.
//!
//! # Where this layout comes from
//!
//! `header` in `Source/exehead/fileform.h` of the NSIS 1.98 source, which
//! ships inside the 1.98 distribution installer (`nsis198.exe`, SourceForge
//! *Legacy NSIS/1.98*). The struct is a `common_header` followed by
//! installer-only fields; the offsets below are that struct compiled with the
//! defines a released makensis 1.98 reports under `/HDRINFO`.
//!
//! # Builds that move these fields
//!
//! Almost every field in the struct is inside an `#ifdef`, so a makensis built
//! with a different configuration produces a different — and smaller — header.
//! A released 1.98 defines them all, which is what installers in the wild were
//! built with. [`parse`](V1Header::parse) checks that the tables it derives
//! actually fit the header it was handed, so a header laid out differently is
//! rejected rather than read as nonsense.

use crate::{
    error::Error,
    header::blockheader::{BLOCKS_NUM, BlockType},
    nsis::{entry::Entry, section::Section},
    util::read_i32_le,
};

/// Offsets of the fields this crate reads, in bytes from the start of the
/// header. Named after the fields in the 1.98 `header` struct.
mod field {
    /// Installer name, as `Name` set it.
    pub const NAME_PTR: usize = 20;
    /// Window caption.
    pub const CAPTION_PTR: usize = 24;
    /// Number of entries in the instruction table.
    pub const NUM_ENTRIES: usize = 52;
    /// `.onInit`, then `.onInstSuccess`, `.onInstFailed`, `.onUserAbort`,
    /// `.onNextPage`, each one entry index or -1.
    pub const CODE_ON_INIT: usize = 76;
    /// Registry root key the install directory is read from.
    pub const INSTALL_REG_ROOTKEY: usize = 160;
    /// Registry key and value naming the install directory.
    pub const INSTALL_REG_KEY_PTR: usize = 164;
    /// Registry value name.
    pub const INSTALL_REG_VALUE_PTR: usize = 168;
    /// Default install directory.
    pub const INSTALL_DIRECTORY_PTR: usize = 204;
    /// Offset of the uninstaller data in the data block, or -1.
    pub const UNINSTDATA_OFFSET: usize = 208;
    /// Number of sections.
    pub const NUM_SECTIONS: usize = 224;
    /// `.onPrevPage`, then `.onVerifyInstDir` and `.onSelChange`.
    pub const CODE_ON_PREV_PAGE: usize = 228;
}

/// The number of `.on*` callbacks 1.x stores, across both runs of fields.
const CALLBACK_COUNT: usize = 8;

/// The 240-byte install header of an NSIS 1.x installer.
///
/// See the [module documentation](self) for the layout and its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V1Header<'a> {
    bytes: &'a [u8],
    /// Byte offset of the section table, always [`V1Header::SIZE`].
    sections_offset: usize,
    /// Byte offset of the entry table.
    entries_offset: usize,
    /// Byte offset of the string table.
    strings_offset: usize,
    /// Length of the whole header block, which the string table runs to.
    block_len: usize,
}

impl<'a> V1Header<'a> {
    /// Size of the header struct in bytes.
    pub const SIZE: usize = 240;

    /// Parses the install header at the start of a decompressed 1.x header
    /// block.
    ///
    /// # Arguments
    ///
    /// * `data` - The whole decompressed header block, header struct first.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data` cannot hold the header, and
    /// [`Error::InvalidBlockOffset`] if the section and entry counts describe
    /// tables that do not fit in it. Since nothing in the file says which
    /// generation wrote it, that second check is also what tells a 1.x header
    /// apart from a 2.x one — see [`NsisInstaller`](crate::NsisInstaller).
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "V1Header",
        })?;

        let out_of_range = |what: &'static str| Error::InvalidBlockOffset {
            block: what,
            offset: u32::MAX,
        };

        // A count that is negative, or large enough to overflow the offset
        // arithmetic below, is not a 1.x header.
        let num_sections = usize::try_from(read_i32_le(bytes, field::NUM_SECTIONS))
            .map_err(|_| out_of_range("V1 sections"))?;
        let num_entries = usize::try_from(read_i32_le(bytes, field::NUM_ENTRIES))
            .map_err(|_| out_of_range("V1 entries"))?;

        let sections_offset = Self::SIZE;
        let entries_offset = num_sections
            .checked_mul(Section::V1_SIZE)
            .and_then(|n| n.checked_add(sections_offset))
            .ok_or_else(|| out_of_range("V1 sections"))?;
        let strings_offset = num_entries
            .checked_mul(Entry::V1_SIZE)
            .and_then(|n| n.checked_add(entries_offset))
            .ok_or_else(|| out_of_range("V1 entries"))?;

        // The three tables and the string table share one block with nothing
        // to locate them, so the only thing that can be checked is that they
        // fit. A 2.x header read this way fails here: its block descriptors
        // sit where these counts do and are far too large.
        if strings_offset > data.len() {
            return Err(out_of_range("V1 strings"));
        }

        Ok(Self {
            bytes,
            sections_offset,
            entries_offset,
            strings_offset,
            block_len: data.len(),
        })
    }

    /// Returns the number of sections.
    #[inline]
    pub fn num_sections(&self) -> i32 {
        read_i32_le(self.bytes, field::NUM_SECTIONS)
    }

    /// Returns the number of entries in the instruction table.
    #[inline]
    pub fn num_entries(&self) -> i32 {
        read_i32_le(self.bytes, field::NUM_ENTRIES)
    }

    /// Returns the tables as the `(offset, count)` pairs a 2.x block table
    /// would carry, indexed by [`BlockType`].
    ///
    /// 1.x has no pages, language tables, control colours or background font,
    /// so those come back empty. The string table's count is its byte length,
    /// matching what 2.x stores.
    pub fn block_layout(&self) -> [(u32, i32); BLOCKS_NUM] {
        let mut blocks = [(0u32, 0i32); BLOCKS_NUM];
        // 1.x does not record the string table's length: it is whatever is
        // left of the header block after the three tables.
        let strings_len = self.block_len.saturating_sub(self.strings_offset);
        for (index, value) in [
            (
                BlockType::Sections,
                (self.sections_offset, self.num_sections()),
            ),
            (
                BlockType::Entries,
                (self.entries_offset, self.num_entries()),
            ),
            (
                BlockType::Strings,
                (self.strings_offset, strings_len as i32),
            ),
        ] {
            if let Some(slot) = blocks.get_mut(index as usize) {
                *slot = (value.0 as u32, value.1);
            }
        }
        blocks
    }

    /// Returns the byte offset of the string table within the header block.
    #[inline]
    pub fn strings_offset(&self) -> usize {
        self.strings_offset
    }

    /// Returns the installer name.
    #[inline]
    pub fn name_ptr(&self) -> i32 {
        read_i32_le(self.bytes, field::NAME_PTR)
    }

    /// Returns the window caption.
    #[inline]
    pub fn caption_ptr(&self) -> i32 {
        read_i32_le(self.bytes, field::CAPTION_PTR)
    }

    /// Returns the default install directory.
    #[inline]
    pub fn install_dir_ptr(&self) -> i32 {
        read_i32_le(self.bytes, field::INSTALL_DIRECTORY_PTR)
    }

    /// Returns the registry key naming the install directory.
    #[inline]
    pub fn install_reg_key_ptr(&self) -> i32 {
        read_i32_le(self.bytes, field::INSTALL_REG_KEY_PTR)
    }

    /// Returns the registry value naming the install directory.
    #[inline]
    pub fn install_reg_value_ptr(&self) -> i32 {
        read_i32_le(self.bytes, field::INSTALL_REG_VALUE_PTR)
    }

    /// Returns the registry root key the install directory is read from.
    #[inline]
    pub fn install_reg_rootkey(&self) -> i32 {
        read_i32_le(self.bytes, field::INSTALL_REG_ROOTKEY)
    }

    /// Returns the offset of the uninstaller data within the data block, or
    /// `-1` when the installer writes no uninstaller.
    #[inline]
    pub fn uninstall_data_offset(&self) -> i32 {
        read_i32_le(self.bytes, field::UNINSTDATA_OFFSET)
    }

    /// Returns the `.on*` callbacks, each an entry index or `-1`.
    ///
    /// In order: `.onInit`, `.onInstSuccess`, `.onInstFailed`, `.onUserAbort`,
    /// `.onNextPage`, `.onPrevPage`, `.onVerifyInstDir`, `.onSelChange`. The
    /// two runs are not adjacent in the struct — the first five are shared
    /// with uninstallers and the last three are installer-only.
    pub fn callbacks(&self) -> [i32; CALLBACK_COUNT] {
        let mut out = [-1; CALLBACK_COUNT];
        for (i, slot) in out.iter_mut().enumerate() {
            let base = if i < 5 {
                field::CODE_ON_INIT
            } else {
                // The installer-only run restarts the numbering.
                field::CODE_ON_PREV_PAGE.wrapping_sub(5 * 4)
            };
            *slot = read_i32_le(self.bytes, base.wrapping_add(i.wrapping_mul(4)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header block holding `sections` sections and `entries` entries, with
    /// `strings` bytes of string table after them.
    fn header_block(sections: i32, entries: i32, strings: usize) -> Vec<u8> {
        let mut data = vec![0u8; V1Header::SIZE];
        data[field::NUM_SECTIONS..field::NUM_SECTIONS + 4].copy_from_slice(&sections.to_le_bytes());
        data[field::NUM_ENTRIES..field::NUM_ENTRIES + 4].copy_from_slice(&entries.to_le_bytes());
        let tables = (sections.max(0) as usize) * Section::V1_SIZE
            + (entries.max(0) as usize) * Entry::V1_SIZE;
        data.resize(V1Header::SIZE + tables + strings, 0);
        data
    }

    #[test]
    fn the_tables_are_found_by_adding_up_the_sizes() {
        // The layout of tests/fixtures/nsis1x.exe, whose build log reports
        // 1 section, 3 instructions (72 bytes) and a 292 byte string table.
        let data = header_block(1, 3, 292);
        assert_eq!(data.len(), 624);

        let header = V1Header::parse(&data).expect("a 1.x header");
        let blocks = header.block_layout();
        assert_eq!(blocks[BlockType::Sections as usize], (240, 1));
        assert_eq!(blocks[BlockType::Entries as usize], (260, 3));
        assert_eq!(blocks[BlockType::Strings as usize], (332, 292));
    }

    #[test]
    fn blocks_1x_does_not_have_are_empty() {
        let data = header_block(1, 3, 16);
        let blocks = V1Header::parse(&data).expect("a 1.x header").block_layout();
        for block in [
            BlockType::Pages,
            BlockType::LangTables,
            BlockType::CtlColors,
            BlockType::BgFont,
            BlockType::Data,
        ] {
            assert_eq!(blocks[block as usize], (0, 0), "{}", block.name());
        }
    }

    #[test]
    fn tables_that_do_not_fit_are_rejected() {
        // This is what tells a 1.x header from a 2.x one: read as 1.x, a 2.x
        // header's block descriptors land on the count fields and describe
        // tables far larger than the block holding them.
        let mut data = header_block(1, 3, 0);
        data[field::NUM_ENTRIES..field::NUM_ENTRIES + 4].copy_from_slice(&9999i32.to_le_bytes());
        assert!(matches!(
            V1Header::parse(&data),
            Err(Error::InvalidBlockOffset { .. })
        ));
    }

    #[test]
    fn negative_counts_are_rejected() {
        for field_offset in [field::NUM_SECTIONS, field::NUM_ENTRIES] {
            let mut data = header_block(1, 3, 64);
            data[field_offset..field_offset + 4].copy_from_slice(&(-1i32).to_le_bytes());
            assert!(
                matches!(
                    V1Header::parse(&data),
                    Err(Error::InvalidBlockOffset { .. })
                ),
                "a negative count at {field_offset} should not parse"
            );
        }
    }

    #[test]
    fn parse_too_short() {
        assert!(matches!(
            V1Header::parse(&[0u8; V1Header::SIZE - 1]),
            Err(Error::TooShort { .. })
        ));
    }

    #[test]
    fn callbacks_come_from_both_runs_of_fields() {
        let mut data = header_block(0, 0, 0);
        // .onInit .. .onNextPage at 76, then .onPrevPage .. .onSelChange at 228.
        for (i, offset) in [76, 80, 84, 88, 92, 228, 232, 236].into_iter().enumerate() {
            data[offset..offset + 4].copy_from_slice(&(i as i32).to_le_bytes());
        }
        let header = V1Header::parse(&data).expect("a 1.x header");
        assert_eq!(header.callbacks(), [0, 1, 2, 3, 4, 5, 6, 7]);
    }
}
