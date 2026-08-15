//! NSIS installer page structure.
//!
//! Each page describes one step in the installation wizard (license agreement,
//! directory selection, component selection, progress, etc.).
//!
//! Source: `fileform.h` from the NSIS source code.

use crate::{
    error::Error,
    util::{read_i32_le, read_u32_le},
};

// Page types (PWP_*).

/// Identifies the type of an installer page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    /// License agreement page.
    License,
    /// Component selection page.
    SelCom,
    /// Directory selection page.
    Dir,
    /// Installation progress page.
    InstFiles,
    /// Uninstall confirmation page.
    Uninst,
    /// Installation completed page.
    Completed,
    /// Custom page.
    Custom,
    /// Unknown page type.
    Unknown(i32),
}

impl PageType {
    /// Converts a `wndproc_id` value to a page type.
    pub fn from_wndproc_id(id: i32) -> Self {
        match id {
            0 => PageType::License,
            1 => PageType::SelCom,
            2 => PageType::Dir,
            3 => PageType::InstFiles,
            4 => PageType::Uninst,
            5 => PageType::Completed,
            6 => PageType::Custom,
            other => PageType::Unknown(other),
        }
    }
}

// Page flags (PF_*).

/// License is selected.
pub const PF_LICENSE_SELECTED: u32 = 1;
/// Next button enabled.
pub const PF_NEXT_ENABLE: u32 = 2;
/// Cancel button enabled.
pub const PF_CANCEL_ENABLE: u32 = 4;
/// Back button shown.
pub const PF_BACK_SHOW: u32 = 8;
/// License text is a stream.
pub const PF_LICENSE_STREAM: u32 = 16;
/// Force license selection.
pub const PF_LICENSE_FORCE_SELECTION: u32 = 32;
/// No forced license selection.
pub const PF_LICENSE_NO_FORCE_SELECTION: u32 = 64;
/// No next button focus.
pub const PF_NO_NEXT_FOCUS: u32 = 128;
/// Back button enabled.
pub const PF_BACK_ENABLE: u32 = 256;
/// Page created with `PageEx`.
pub const PF_PAGE_EX: u32 = 512;
/// Do not disable browse button when directory is invalid.
pub const PF_DIR_NO_BTN_DISABLE: u32 = 1024;

/// View type for an NSIS page descriptor (64 bytes).
///
/// # Layout (16 x i32, little-endian)
///
/// | Offset | Field | Description |
/// |--------|-------|-------------|
/// | 0x00 | `dlg_id` | Dialog resource ID |
/// | 0x04 | `wndproc_id` | Window procedure type (PWP_*) |
/// | 0x08 | `prefunc` | Callback before page creation |
/// | 0x0C | `showfunc` | Callback before page shown |
/// | 0x10 | `leavefunc` | Callback when leaving page |
/// | 0x14 | `flags` | PF_* flags |
/// | 0x18 | `caption` | String table offset for caption |
/// | 0x1C | `back` | String table offset for Back button |
/// | 0x20 | `next` | String table offset for Next button |
/// | 0x24 | `clicknext` | String table offset for clicknext |
/// | 0x28 | `cancel` | String table offset for Cancel button |
/// | 0x2C | `parms[0]` | Additional parameter 0 |
/// | 0x30 | `parms[1]` | Additional parameter 1 |
/// | 0x34 | `parms[2]` | Additional parameter 2 |
/// | 0x38 | `parms[3]` | Additional parameter 3 |
/// | 0x3C | `parms[4]` | Additional parameter 4 |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page<'a> {
    bytes: &'a [u8],
}

impl<'a> Page<'a> {
    /// The on-disk size of a page descriptor in bytes.
    pub const SIZE: usize = 64;

    /// Parses a page from the start of `data`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 64`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "Page",
            });
        }
        Ok(Self {
            bytes: data.get(..Self::SIZE).ok_or(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "Page",
            })?,
        })
    }

    /// Returns the dialog resource ID.
    #[inline]
    pub fn dlg_id(&self) -> i32 {
        read_i32_le(self.bytes, 0)
    }

    /// Returns the window procedure type (maps to [`PageType`]).
    #[inline]
    pub fn wndproc_id(&self) -> i32 {
        read_i32_le(self.bytes, 4)
    }

    /// Returns the page type derived from `wndproc_id`.
    #[inline]
    pub fn page_type(&self) -> PageType {
        PageType::from_wndproc_id(self.wndproc_id())
    }

    /// Returns the pre-creation callback entry index (-1 if unused).
    #[inline]
    pub fn prefunc(&self) -> i32 {
        read_i32_le(self.bytes, 8)
    }

    /// Returns the pre-show callback entry index (-1 if unused).
    #[inline]
    pub fn showfunc(&self) -> i32 {
        read_i32_le(self.bytes, 12)
    }

    /// Returns the leave callback entry index (-1 if unused).
    #[inline]
    pub fn leavefunc(&self) -> i32 {
        read_i32_le(self.bytes, 16)
    }

    /// Returns the page flags (`PF_*`).
    #[inline]
    pub fn flags(&self) -> u32 {
        read_u32_le(self.bytes, 20)
    }

    /// Returns the string table offset for the page caption.
    #[inline]
    pub fn caption(&self) -> i32 {
        read_i32_le(self.bytes, 24)
    }

    /// Returns the string table offset for the Back button text.
    #[inline]
    pub fn back(&self) -> i32 {
        read_i32_le(self.bytes, 28)
    }

    /// Returns the string table offset for the Next button text.
    #[inline]
    pub fn next(&self) -> i32 {
        read_i32_le(self.bytes, 32)
    }

    /// Returns the string table offset for the clicknext text.
    #[inline]
    pub fn clicknext(&self) -> i32 {
        read_i32_le(self.bytes, 36)
    }

    /// Returns the string table offset for the Cancel button text.
    #[inline]
    pub fn cancel(&self) -> i32 {
        read_i32_le(self.bytes, 40)
    }

    /// Returns the additional parameters array.
    #[inline]
    pub fn parms(&self) -> [i32; 5] {
        [
            read_i32_le(self.bytes, 44),
            read_i32_le(self.bytes, 48),
            read_i32_le(self.bytes, 52),
            read_i32_le(self.bytes, 56),
            read_i32_le(self.bytes, 60),
        ]
    }
}

/// Iterator over NSIS pages in the page block.
#[derive(Debug)]
pub struct PageIter<'a> {
    data: &'a [u8],
    remaining: usize,
    offset: usize,
}

impl<'a> PageIter<'a> {
    /// Creates a new page iterator over the page block data.
    pub fn new(data: &'a [u8], count: usize) -> Self {
        Self {
            data,
            remaining: count,
            offset: 0,
        }
    }
}

impl<'a> Iterator for PageIter<'a> {
    type Item = Result<Page<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining = self.remaining.saturating_sub(1);
        let slice = self.data.get(self.offset..).unwrap_or(&[]);
        let result = Page::parse(slice);
        self.offset = self.offset.saturating_add(Page::SIZE);
        Some(result)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for PageIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_page(wndproc_id: i32, flags: u32) -> [u8; 64] {
        let mut buf = [0u8; 64];
        buf[0..4].copy_from_slice(&105i32.to_le_bytes()); // dlg_id
        buf[4..8].copy_from_slice(&wndproc_id.to_le_bytes());
        buf[8..12].copy_from_slice(&(-1i32).to_le_bytes()); // prefunc
        buf[12..16].copy_from_slice(&(-1i32).to_le_bytes()); // showfunc
        buf[16..20].copy_from_slice(&(-1i32).to_le_bytes()); // leavefunc
        buf[20..24].copy_from_slice(&flags.to_le_bytes());
        buf
    }

    #[test]
    fn parse_valid() {
        let buf = make_page(2, PF_NEXT_ENABLE | PF_BACK_SHOW);
        let p = Page::parse(&buf).unwrap();
        assert_eq!(p.dlg_id(), 105);
        assert_eq!(p.page_type(), PageType::Dir);
        assert_eq!(p.prefunc(), -1);
        assert_eq!(p.flags() & PF_NEXT_ENABLE, PF_NEXT_ENABLE);
        assert_eq!(p.flags() & PF_BACK_SHOW, PF_BACK_SHOW);
    }

    #[test]
    fn parse_too_short() {
        let buf = [0u8; 63];
        assert!(Page::parse(&buf).is_err());
    }

    #[test]
    fn page_types() {
        assert_eq!(PageType::from_wndproc_id(0), PageType::License);
        assert_eq!(PageType::from_wndproc_id(3), PageType::InstFiles);
        assert_eq!(PageType::from_wndproc_id(6), PageType::Custom);
        assert_eq!(PageType::from_wndproc_id(99), PageType::Unknown(99));
    }

    #[test]
    fn iterator_count() {
        let p1 = make_page(0, 0);
        let p2 = make_page(2, 0);
        let mut data = Vec::new();
        data.extend_from_slice(&p1);
        data.extend_from_slice(&p2);
        let iter = PageIter::new(&data, 2);
        assert_eq!(iter.len(), 2);
        assert_eq!(iter.count(), 2);
    }
}
