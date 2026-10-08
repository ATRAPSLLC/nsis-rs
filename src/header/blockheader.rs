//! NSIS block header structure.
//!
//! Each decompressed header contains 8 block descriptors, one per block type.
//! Each descriptor gives the byte offset (within the decompressed header) and
//! item count for that block.
//!
//! Source: `fileform.h` from the NSIS source code.

use crate::{
    error::Error,
    util::{Blob, read_i32_le, read_u32_le},
};

/// Number of block headers in an NSIS common header.
pub const BLOCKS_NUM: usize = 8;

/// Identifies the type of data in a block.
///
/// Source: `fileform.h` `enum` block indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockType {
    /// Installer page definitions.
    Pages = 0,
    /// Install section descriptors.
    Sections = 1,
    /// Bytecode instructions (entry array).
    Entries = 2,
    /// String table (`num` = total byte length).
    Strings = 3,
    /// Language tables.
    LangTables = 4,
    /// Control color structures.
    CtlColors = 5,
    /// Background font (LOGFONT structure).
    BgFont = 6,
    /// Data block offset.
    Data = 7,
}

impl BlockType {
    /// Returns a human-readable name for this block type.
    pub fn name(self) -> &'static str {
        match self {
            BlockType::Pages => "Pages",
            BlockType::Sections => "Sections",
            BlockType::Entries => "Entries",
            BlockType::Strings => "Strings",
            BlockType::LangTables => "LangTables",
            BlockType::CtlColors => "CtlColors",
            BlockType::BgFont => "BgFont",
            BlockType::Data => "Data",
        }
    }

    /// Converts an index (0..8) to a block type.
    pub fn from_index(index: usize) -> Result<Self, Error> {
        match index {
            0 => Ok(BlockType::Pages),
            1 => Ok(BlockType::Sections),
            2 => Ok(BlockType::Entries),
            3 => Ok(BlockType::Strings),
            4 => Ok(BlockType::LangTables),
            5 => Ok(BlockType::CtlColors),
            6 => Ok(BlockType::BgFont),
            7 => Ok(BlockType::Data),
            _ => Err(Error::InvalidBlockIndex { index }),
        }
    }
}

/// View type for an NSIS block header (8 bytes).
///
/// # Layout (2 x i32, little-endian)
///
/// | Offset | Field | Description |
/// |--------|-------|-------------|
/// | 0x00 | `offset` | Byte offset within decompressed header data |
/// | 0x04 | `num` | Item count (or total byte length for strings) |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHeader<'a> {
    bytes: Blob<&'a [u8]>,
}

/// An all-zero `BlockHeader` (offset = 0, num = 0).
///
/// Used as a default placeholder when constructing arrays of block headers
/// before parsing populates them.
pub const EMPTY_BLOCK: BlockHeader<'static> = BlockHeader {
    bytes: Blob(&[0u8; 8]),
};

impl<'a> BlockHeader<'a> {
    /// The on-disk size of a block header in bytes.
    pub const SIZE: usize = 8;

    /// Parses a block header from the start of `data`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 8`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "BlockHeader",
            });
        }
        Ok(Self {
            bytes: Blob(data.get(..Self::SIZE).ok_or(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "BlockHeader",
            })?),
        })
    }

    /// Returns the byte offset of this block within the decompressed header.
    #[inline]
    pub fn offset(&self) -> u32 {
        read_u32_le(&self.bytes, 0)
    }

    /// Returns the item count for this block.
    ///
    /// For most blocks, this is the number of items. For the string block
    /// ([`BlockType::Strings`]), this is the total byte length of the string table.
    #[inline]
    pub fn num(&self) -> i32 {
        read_i32_le(&self.bytes, 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid() {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&256u32.to_le_bytes());
        buf[4..8].copy_from_slice(&10i32.to_le_bytes());
        let bh = BlockHeader::parse(&buf).unwrap();
        assert_eq!(bh.offset(), 256);
        assert_eq!(bh.num(), 10);
    }

    #[test]
    fn parse_too_short() {
        let buf = [0u8; 7];
        assert_eq!(
            BlockHeader::parse(&buf),
            Err(Error::TooShort {
                expected: 8,
                actual: 7,
                context: "BlockHeader",
            })
        );
    }

    #[test]
    fn block_type_from_index() {
        assert_eq!(BlockType::from_index(0).unwrap(), BlockType::Pages);
        assert_eq!(BlockType::from_index(7).unwrap(), BlockType::Data);
        assert_eq!(
            BlockType::from_index(8),
            Err(Error::InvalidBlockIndex { index: 8 })
        );
    }

    #[test]
    fn block_type_name() {
        assert_eq!(BlockType::Pages.name(), "Pages");
        assert_eq!(BlockType::Strings.name(), "Strings");
        assert_eq!(BlockType::Data.name(), "Data");
    }

    #[test]
    fn block_header_is_copy() {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&100u32.to_le_bytes());
        buf[4..8].copy_from_slice(&5i32.to_le_bytes());
        let bh1 = BlockHeader::parse(&buf).unwrap();
        let bh2 = bh1; // Copy
        assert_eq!(bh1, bh2);
    }

    #[test]
    fn roundtrip_all_block_types() {
        for i in 0..BLOCKS_NUM {
            let bt = BlockType::from_index(i).unwrap();
            assert_eq!(bt as u8 as usize, i);
        }
    }
}
