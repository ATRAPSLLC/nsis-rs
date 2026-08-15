//! NSIS common header structure.
//!
//! The common header is the first structure in the decompressed header block.
//! It contains flags, 8 block descriptors, registry settings, colors, callbacks,
//! install types, and directory configuration.
//!
//! **Important**: The exact layout varies between NSIS versions due to conditional
//! compilation (`#ifdef`). This parser accounts for the most common layout (NSIS 2.x/3.x)
//! and validates block headers to detect misalignment.
//!
//! Source: `fileform.h` from the NSIS source code.

use crate::{
    error::Error,
    header::blockheader::{BLOCKS_NUM, BlockHeader, BlockType, EMPTY_BLOCK},
    util::{read_i32_le, read_u32_le},
};

// Common header flags (CH_FLAGS_*).

/// Show details by default.
pub const CH_FLAGS_DETAILS_SHOWDETAILS: u32 = 1;
/// Never show details.
pub const CH_FLAGS_DETAILS_NEVERSHOW: u32 = 2;
/// Colored progress bar.
pub const CH_FLAGS_PROGRESS_COLORED: u32 = 4;
/// Silent installation.
pub const CH_FLAGS_SILENT: u32 = 8;
/// Silent with log.
pub const CH_FLAGS_SILENT_LOG: u32 = 16;
/// Auto-close after install.
pub const CH_FLAGS_AUTO_CLOSE: u32 = 32;
/// Do not show directory page.
pub const CH_FLAGS_DIR_NO_SHOW: u32 = 64;
/// No root directory.
pub const CH_FLAGS_NO_ROOT_DIR: u32 = 128;
/// Components only on custom install type.
pub const CH_FLAGS_COMP_ONLY_ON_CUSTOM: u32 = 256;
/// No custom install type.
pub const CH_FLAGS_NO_CUSTOM: u32 = 512;

/// Maximum number of install types.
pub const NSIS_MAX_INST_TYPES: usize = 32;

/// Byte offset of `install_directory_ptr` in the standard layout.
///
/// The install-type table sits between the callbacks and the directory fields
/// and is easy to overlook: `NSIS_MAX_INST_TYPES + 1` integers, 132 bytes, so
/// the directory pointer is *not* at 148 where the callbacks end.
///
/// | Offset | Field |
/// |---|---|
/// | 108..148 | 10 callback entry indices |
/// | 148..280 | install types (`NSIS_MAX_INST_TYPES + 1` ints) |
/// | 280 | `install_directory_ptr` |
/// | 284 | `install_directory_auto_append` |
/// | 288 | `str_uninstchild` |
/// | 292 | `str_uninstcmd` |
/// | 296 | `str_wininit` |
///
/// Source: `fileform.h`, `header` struct.
const INSTALL_DIR_OFFSET: usize = 148 + (NSIS_MAX_INST_TYPES + 1) * 4;

/// Minimum size of the common header (flags + 8 block headers).
///
/// This is the absolute minimum; the full header is larger but the exact
/// size depends on the NSIS version and compile-time options.
pub const COMMON_HEADER_MIN_SIZE: usize = 4 + (BLOCKS_NUM * BlockHeader::SIZE);

/// View type for the NSIS common header.
///
/// The common header sits at the start of the decompressed header block.
/// It provides flags, block descriptors (which give offsets into the rest
/// of the decompressed data), callback entry indices, and install configuration.
#[derive(Debug)]
pub struct CommonHeader<'a> {
    bytes: &'a [u8],
    blocks: [BlockHeader<'a>; BLOCKS_NUM],
}

impl<'a> CommonHeader<'a> {
    /// Parses the common header from the start of the decompressed header data.
    ///
    /// # Errors
    ///
    /// - [`Error::TooShort`] if `data` is smaller than the minimum header size
    /// - [`Error::InvalidBlockOffset`] if any block header points outside `data`
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < COMMON_HEADER_MIN_SIZE {
            return Err(Error::TooShort {
                expected: COMMON_HEADER_MIN_SIZE,
                actual: data.len(),
                context: "CommonHeader",
            });
        }

        // Parse the 8 block headers starting at offset 4 (after the flags field).
        let zero_block = BlockHeader::parse(&[0u8; 8])?;
        let mut blocks: [BlockHeader<'a>; BLOCKS_NUM] = [zero_block; BLOCKS_NUM];
        for (i, block) in blocks.iter_mut().enumerate() {
            let block_offset = i
                .checked_mul(BlockHeader::SIZE)
                .and_then(|n| n.checked_add(4))
                .ok_or(Error::TooShort {
                    expected: COMMON_HEADER_MIN_SIZE,
                    actual: data.len(),
                    context: "CommonHeader",
                })?;
            let slice = data.get(block_offset..).ok_or(Error::TooShort {
                expected: COMMON_HEADER_MIN_SIZE,
                actual: data.len(),
                context: "CommonHeader",
            })?;
            *block = BlockHeader::parse(slice)?;
        }

        // Validate block offsets are within bounds (except Data block which may
        // reference the original file, not the decompressed header).
        for (i, block) in blocks.iter().enumerate() {
            if i == BlockType::Data as usize {
                continue;
            }
            let off = block.offset() as usize;
            if off > data.len() && block.num() > 0 {
                let bt = BlockType::from_index(i).unwrap_or(BlockType::Pages);
                return Err(Error::InvalidBlockOffset {
                    block: bt.name(),
                    offset: block.offset(),
                });
            }
        }

        Ok(Self {
            bytes: data,
            blocks,
        })
    }

    /// Returns the common header flags (`CH_FLAGS_*`).
    #[inline]
    pub fn flags(&self) -> u32 {
        read_u32_le(self.bytes, 0)
    }

    /// Returns the block header for the given block type.
    ///
    /// The `BlockType` enum discriminants are always within `0..BLOCKS_NUM`,
    /// so this lookup never returns `None` for a well-formed `BlockType`;
    /// the `.first()` fallback is purely defensive.
    #[inline]
    pub fn block(&self, bt: BlockType) -> &BlockHeader<'a> {
        self.blocks
            .get(bt as usize)
            .or_else(|| self.blocks.first())
            .unwrap_or(&EMPTY_BLOCK)
    }

    /// Returns all 8 block headers.
    #[inline]
    pub fn blocks(&self) -> &[BlockHeader<'a>; BLOCKS_NUM] {
        &self.blocks
    }

    /// Returns the install registry root key.
    ///
    /// Located after the block headers at offset `4 + 8*8 = 68`.
    #[inline]
    pub fn install_reg_rootkey(&self) -> i32 {
        if self.bytes.len() >= 72 {
            read_i32_le(self.bytes, 68)
        } else {
            0
        }
    }

    /// Returns the language table entry size.
    ///
    /// This field determines the size of each entry in the language table block.
    /// Located at a version-dependent offset after the color fields.
    pub fn langtable_size(&self) -> i32 {
        // In the standard NSIS 2.x/3.x layout, langtable_size is at offset 100.
        // offset 68: install_reg_rootkey (4)
        // offset 72: install_reg_key_ptr (4)
        // offset 76: install_reg_value_ptr (4)
        // offset 80: bg_color1 (4)
        // offset 84: bg_color2 (4)
        // offset 88: bg_textcolor (4)
        // offset 92: lb_bg (4)
        // offset 96: lb_fg (4)
        // offset 100: langtable_size (4)
        if self.bytes.len() >= 104 {
            read_i32_le(self.bytes, 100)
        } else {
            0
        }
    }

    /// Returns the callback entry index for `.onInit` (-1 if unused).
    pub fn code_on_init(&self) -> i32 {
        // offset 108 in standard layout (after license_bg at 104).
        if self.bytes.len() >= 112 {
            read_i32_le(self.bytes, 108)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onInstSuccess` (-1 if unused).
    pub fn code_on_inst_success(&self) -> i32 {
        if self.bytes.len() >= 116 {
            read_i32_le(self.bytes, 112)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onInstFailed` (-1 if unused).
    pub fn code_on_inst_failed(&self) -> i32 {
        if self.bytes.len() >= 120 {
            read_i32_le(self.bytes, 116)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onUserAbort` (-1 if unused).
    pub fn code_on_user_abort(&self) -> i32 {
        if self.bytes.len() >= 124 {
            read_i32_le(self.bytes, 120)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onGUIInit` (-1 if unused).
    pub fn code_on_gui_init(&self) -> i32 {
        if self.bytes.len() >= 128 {
            read_i32_le(self.bytes, 124)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onGUIEnd` (-1 if unused).
    pub fn code_on_gui_end(&self) -> i32 {
        if self.bytes.len() >= 132 {
            read_i32_le(self.bytes, 128)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onMouseOverSection` (-1 if unused).
    pub fn code_on_mouse_over_section(&self) -> i32 {
        if self.bytes.len() >= 136 {
            read_i32_le(self.bytes, 132)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onVerifyInstDir` (-1 if unused).
    pub fn code_on_verify_inst_dir(&self) -> i32 {
        if self.bytes.len() >= 140 {
            read_i32_le(self.bytes, 136)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onSelChange` (-1 if unused).
    pub fn code_on_sel_change(&self) -> i32 {
        if self.bytes.len() >= 144 {
            read_i32_le(self.bytes, 140)
        } else {
            -1
        }
    }

    /// Returns the callback entry index for `.onRebootFailed` (-1 if unused).
    pub fn code_on_reboot_failed(&self) -> i32 {
        if self.bytes.len() >= 148 {
            read_i32_le(self.bytes, 144)
        } else {
            -1
        }
    }

    /// Returns the string offset of the registry key the installer records
    /// itself under, or `-1` if unset.
    ///
    /// Paired with [`install_reg_rootkey`](Self::install_reg_rootkey) and
    /// [`install_reg_value_ptr`](Self::install_reg_value_ptr), this is where
    /// the installer stores its own install directory for later uninstallation.
    pub fn install_reg_key_ptr(&self) -> i32 {
        self.field_at(72).unwrap_or(-1)
    }

    /// Returns the string offset of the registry value name, or `-1` if unset.
    pub fn install_reg_value_ptr(&self) -> i32 {
        self.field_at(76).unwrap_or(-1)
    }

    /// Returns the string offset of the default install directory, or `-1`.
    ///
    /// This is the `InstallDir` from the script, and typically the most
    /// interesting string in the header — it is where the installer writes by
    /// default and usually begins with a shell folder such as `$PROGRAMFILES`.
    pub fn install_dir_ptr(&self) -> i32 {
        self.field_at(INSTALL_DIR_OFFSET).unwrap_or(-1)
    }

    /// Returns the string offset of the directory name appended to a
    /// user-chosen install location, or `-1`.
    ///
    /// Set by `InstallDirRegKey`'s auto-append behaviour.
    pub fn install_dir_auto_append_ptr(&self) -> i32 {
        self.field_at(INSTALL_DIR_OFFSET.saturating_add(4))
            .unwrap_or(-1)
    }

    /// Returns the string offset of the uninstaller's child path, or `-1`.
    pub fn uninstall_child_ptr(&self) -> i32 {
        self.field_at(INSTALL_DIR_OFFSET.saturating_add(8))
            .unwrap_or(-1)
    }

    /// Returns the string offset of the uninstaller command line, or `-1`.
    ///
    /// The command NSIS registers so Windows can run the uninstaller.
    pub fn uninstall_command_ptr(&self) -> i32 {
        self.field_at(INSTALL_DIR_OFFSET.saturating_add(12))
            .unwrap_or(-1)
    }

    /// Returns the string offset of the `wininit.ini` path used for
    /// delete-on-reboot, or `-1`.
    pub fn wininit_ptr(&self) -> i32 {
        self.field_at(INSTALL_DIR_OFFSET.saturating_add(16))
            .unwrap_or(-1)
    }

    /// Reads a header field, or `None` if this installer's header stops short.
    ///
    /// The tail fields are all compiled conditionally in NSIS, so a header
    /// built without a feature simply ends before them.
    fn field_at(&self, offset: usize) -> Option<i32> {
        let end = offset.checked_add(4)?;
        (self.bytes.len() >= end).then(|| read_i32_le(self.bytes, offset))
    }

    /// Returns a slice of the decompressed header data for the given block type.
    ///
    /// The returned slice starts at the block's offset and contains `num` items
    /// (the interpretation of "num" depends on the block type).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidBlockOffset`] if the block offset or computed
    /// end is beyond the available data.
    pub fn block_data(&self, bt: BlockType, header_data: &'a [u8]) -> Result<&'a [u8], Error> {
        let bh = self.block(bt);
        let offset = bh.offset() as usize;
        if offset > header_data.len() {
            return Err(Error::InvalidBlockOffset {
                block: bt.name(),
                offset: bh.offset(),
            });
        }
        header_data.get(offset..).ok_or(Error::InvalidBlockOffset {
            block: bt.name(),
            offset: bh.offset(),
        })
    }

    /// Returns `true` if the installer uses silent mode.
    #[inline]
    pub fn is_silent(&self) -> bool {
        self.flags() & CH_FLAGS_SILENT != 0
    }

    /// Returns `true` if the installer auto-closes after installation.
    #[inline]
    pub fn is_auto_close(&self) -> bool {
        self.flags() & CH_FLAGS_AUTO_CLOSE != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid common header (flags + 8 block headers).
    fn make_common_header(flags: u32, blocks: &[(u32, i32); BLOCKS_NUM]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&flags.to_le_bytes());
        for &(offset, num) in blocks {
            buf.extend_from_slice(&offset.to_le_bytes());
            buf.extend_from_slice(&num.to_le_bytes());
        }
        // Pad to at least 256 bytes for callback fields.
        buf.resize(256, 0);
        buf
    }

    #[test]
    fn parse_minimal_valid() {
        let blocks = [(0, 0); BLOCKS_NUM];
        let data = make_common_header(0, &blocks);
        let ch = CommonHeader::parse(&data).unwrap();
        assert_eq!(ch.flags(), 0);
    }

    #[test]
    fn parse_with_flags() {
        let blocks = [(0, 0); BLOCKS_NUM];
        let data = make_common_header(CH_FLAGS_SILENT | CH_FLAGS_AUTO_CLOSE, &blocks);
        let ch = CommonHeader::parse(&data).unwrap();
        assert!(ch.is_silent());
        assert!(ch.is_auto_close());
    }

    #[test]
    fn parse_too_short() {
        let data = [0u8; COMMON_HEADER_MIN_SIZE - 1];
        assert!(CommonHeader::parse(&data).is_err());
    }

    #[test]
    fn block_data_returns_slice() {
        let mut blocks = [(0u32, 0i32); BLOCKS_NUM];
        // Place sections block at offset 100 with 3 items.
        blocks[BlockType::Sections as usize] = (100, 3);
        let mut data = make_common_header(0, &blocks);
        data.resize(200, 0xAA);
        let ch = CommonHeader::parse(&data).unwrap();
        let section_data = ch.block_data(BlockType::Sections, &data).unwrap();
        assert_eq!(section_data.len(), 100); // 200 - 100
    }

    #[test]
    fn block_data_out_of_range() {
        let mut blocks = [(0u32, 0i32); BLOCKS_NUM];
        blocks[BlockType::Entries as usize] = (9999, 1);
        let data = make_common_header(0, &blocks);
        let ch = CommonHeader::parse(&data);
        assert!(ch.is_err());
    }

    #[test]
    fn callbacks_default_to_minus_one_if_short() {
        let blocks = [(0, 0); BLOCKS_NUM];
        // Only COMMON_HEADER_MIN_SIZE bytes — callback fields absent.
        let data = vec![0u8; COMMON_HEADER_MIN_SIZE];
        // Manually set the flags.
        let mut data = data;
        data[0..4].copy_from_slice(&0u32.to_le_bytes());
        let ch = CommonHeader::parse(&data).unwrap();
        let _ = blocks; // suppress unused warning for test clarity
        assert_eq!(ch.code_on_init(), -1);
        assert_eq!(ch.code_on_inst_success(), -1);
    }
}
