//! NSIS control colors structure.
//!
//! Control color definitions come in two variants: 32-bit (24 bytes)
//! and 64-bit (32 bytes), depending on the target platform.
//!
//! Source: `fileform.h` from the NSIS source code.

use crate::{
    error::Error,
    util::{Blob, read_i32_le, read_u32_le},
};

// Color flags (CC_*).

/// Text color is set.
pub const CC_TEXT: u32 = 1;
/// Text color is a system color index.
pub const CC_TEXT_SYS: u32 = 2;
/// Background color is set.
pub const CC_BK: u32 = 4;
/// Background color is a system color index.
pub const CC_BK_SYS: u32 = 8;
/// Background brush is set.
pub const CC_BKB: u32 = 16;
/// Mask for all valid color flags.
pub const CC_FLAGSMASK: u32 = 0x1F;

/// View type for NSIS control colors (handles both 32-bit and 64-bit variants).
///
/// The caller specifies the variant size at parse time based on the
/// detected installer target platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtlColors<'a> {
    bytes: Blob<&'a [u8]>,
    is_64bit: bool,
}

impl<'a> CtlColors<'a> {
    /// The on-disk size of the 32-bit variant.
    pub const SIZE_32: usize = 24;
    /// The on-disk size of the 64-bit variant.
    pub const SIZE_64: usize = 32;

    /// Parses a 32-bit control colors structure.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 24`.
    pub fn parse_32(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE_32 {
            return Err(Error::TooShort {
                expected: Self::SIZE_32,
                actual: data.len(),
                context: "CtlColors32",
            });
        }
        Ok(Self {
            bytes: Blob(data.get(..Self::SIZE_32).ok_or(Error::TooShort {
                expected: Self::SIZE_32,
                actual: data.len(),
                context: "CtlColors32",
            })?),
            is_64bit: false,
        })
    }

    /// Parses a 64-bit control colors structure.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 32`.
    pub fn parse_64(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE_64 {
            return Err(Error::TooShort {
                expected: Self::SIZE_64,
                actual: data.len(),
                context: "CtlColors64",
            });
        }
        Ok(Self {
            bytes: Blob(data.get(..Self::SIZE_64).ok_or(Error::TooShort {
                expected: Self::SIZE_64,
                actual: data.len(),
                context: "CtlColors64",
            })?),
            is_64bit: true,
        })
    }

    /// Returns `true` if this is the 64-bit variant.
    #[inline]
    pub fn is_64bit(&self) -> bool {
        self.is_64bit
    }

    /// Returns the element size (24 for 32-bit, 32 for 64-bit).
    #[inline]
    pub fn element_size(&self) -> usize {
        if self.is_64bit {
            Self::SIZE_64
        } else {
            Self::SIZE_32
        }
    }

    /// Returns the text color (COLORREF).
    #[inline]
    pub fn text(&self) -> u32 {
        read_u32_le(&self.bytes, 0)
    }

    /// Returns the background color (COLORREF).
    #[inline]
    pub fn bkc(&self) -> u32 {
        read_u32_le(&self.bytes, 4)
    }

    /// Returns the color flags (`CC_*`).
    pub fn flags(&self) -> u32 {
        if self.is_64bit {
            // 64-bit layout: text(4), bkc(4), bkb(8), lbStyle(4), bkmode(4), flags(4)
            read_u32_le(&self.bytes, 28)
        } else {
            // 32-bit layout: text(4), bkc(4), lbStyle(4), bkb(4), bkmode(4), flags(4)
            read_u32_le(&self.bytes, 20)
        }
    }

    /// Returns the background mode.
    pub fn bkmode(&self) -> i32 {
        if self.is_64bit {
            read_i32_le(&self.bytes, 24)
        } else {
            read_i32_le(&self.bytes, 16)
        }
    }

    /// Returns `true` if the text color is set.
    #[inline]
    pub fn has_text_color(&self) -> bool {
        self.flags() & CC_TEXT != 0
    }

    /// Returns `true` if the background color is set.
    #[inline]
    pub fn has_bk_color(&self) -> bool {
        self.flags() & CC_BK != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_32_valid() {
        let mut buf = [0u8; 24];
        buf[0..4].copy_from_slice(&0x00FF00u32.to_le_bytes()); // text = green
        buf[4..8].copy_from_slice(&0xFF0000u32.to_le_bytes()); // bkc = blue
        buf[20..24].copy_from_slice(&(CC_TEXT | CC_BK).to_le_bytes());
        let cc = CtlColors::parse_32(&buf).unwrap();
        assert!(!cc.is_64bit());
        assert_eq!(cc.text(), 0x00FF00);
        assert_eq!(cc.bkc(), 0xFF0000);
        assert!(cc.has_text_color());
        assert!(cc.has_bk_color());
    }

    #[test]
    fn parse_64_valid() {
        let mut buf = [0u8; 32];
        buf[0..4].copy_from_slice(&0x0000FFu32.to_le_bytes()); // text = red
        buf[28..32].copy_from_slice(&CC_TEXT.to_le_bytes());
        let cc = CtlColors::parse_64(&buf).unwrap();
        assert!(cc.is_64bit());
        assert_eq!(cc.text(), 0x0000FF);
        assert!(cc.has_text_color());
        assert!(!cc.has_bk_color());
    }

    #[test]
    fn parse_32_too_short() {
        let buf = [0u8; 23];
        assert!(CtlColors::parse_32(&buf).is_err());
    }

    #[test]
    fn parse_64_too_short() {
        let buf = [0u8; 31];
        assert!(CtlColors::parse_64(&buf).is_err());
    }

    #[test]
    fn element_size() {
        let buf32 = [0u8; 24];
        let cc32 = CtlColors::parse_32(&buf32).unwrap();
        assert_eq!(cc32.element_size(), 24);

        let buf64 = [0u8; 32];
        let cc64 = CtlColors::parse_64(&buf64).unwrap();
        assert_eq!(cc64.element_size(), 32);
    }
}
