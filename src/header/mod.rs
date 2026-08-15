//! NSIS header structures.
//!
//! This module contains parsers for the three header layers:
//!
//! - [`FirstHeader`]: The 28-byte signature and size block found in the PE overlay.
//! - [`BlockHeader`]: An 8-byte descriptor (offset + count) for each data block.
//! - [`CommonHeader`]: The main header containing flags, block descriptors, callbacks,
//!   and install configuration.
//!
//! # Structure Chain
//!
//! ```text
//! PE Overlay
//! └─ FirstHeader (28 bytes, usually at a 512-byte aligned offset)
//!    └─ Compressed Header Block
//!       └─ CommonHeader
//!          ├─ BlockHeader[0] → Pages
//!          ├─ BlockHeader[1] → Sections
//!          ├─ BlockHeader[2] → Entries
//!          ├─ BlockHeader[3] → Strings
//!          ├─ BlockHeader[4] → LangTables
//!          ├─ BlockHeader[5] → CtlColors
//!          ├─ BlockHeader[6] → BgFont
//!          └─ BlockHeader[7] → Data
//! ```

pub mod blockheader;
pub mod commonheader;
pub mod firstheader;
pub mod v1header;

pub use blockheader::{BLOCKS_NUM, BlockHeader, BlockType};
pub use commonheader::CommonHeader;
pub use firstheader::FirstHeader;
pub use v1header::V1Header;

use crate::error::Error;

/// Scans the overlay for a valid NSIS FirstHeader.
///
/// Returns the byte offset within the overlay and the parsed [`FirstHeader`].
///
/// Per the NSIS forums: "All IDPOS values for the ID 'NullsoftInst' can be
/// calculated using the formula in hex format: `O=8+512*n`." The 7-zip NSIS
/// handler searches at 512-byte steps after the PE stub. Some packed installers
/// have extra overlay bytes before the NSIS data, so this function tries the
/// aligned offsets first and then falls back to a byte-wise scan.
///
/// # Errors
///
/// Returns [`Error::SignatureNotFound`] if no valid signature is found.
pub fn scan_for_first_header(overlay: &[u8]) -> Result<(usize, FirstHeader<'_>), Error> {
    // Scan at 512-byte aligned offsets, starting from offset 0.
    let mut offset: usize = 0;
    while let Some(slice) = overlay.get(offset..) {
        if slice.len() < FirstHeader::SIZE {
            break;
        }
        if let Ok(fh) = FirstHeader::parse(slice)
            && has_plausible_lengths(&fh)
        {
            return Ok((offset, fh));
        }
        let Some(next) = offset.checked_add(512) else {
            break;
        };
        offset = next;
    }

    // Some NSIS installers found in the wild have extra data inserted between
    // the end of the PE image and the FirstHeader. In those files the header is
    // still structurally valid, but no longer overlay-relative 512-byte aligned.
    for (offset, slice) in overlay.windows(FirstHeader::SIZE).enumerate() {
        if offset % 512 == 0 {
            continue;
        }
        if let Ok(fh) = FirstHeader::parse(slice)
            && has_plausible_lengths(&fh)
        {
            return Ok((offset, fh));
        }
    }

    Err(Error::SignatureNotFound)
}

/// Returns whether FirstHeader length fields look structurally usable.
fn has_plausible_lengths(header: &FirstHeader<'_>) -> bool {
    header.length_of_header() > 0
        && header.length_of_all_following_data() >= FirstHeader::SIZE as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: builds a valid FirstHeader byte buffer.
    fn make_first_header(flags: u32) -> [u8; 28] {
        let mut buf = [0u8; 28];
        buf[0..4].copy_from_slice(&flags.to_le_bytes());
        buf[4..8].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        buf[8..12].copy_from_slice(&0x6C6C754Eu32.to_le_bytes()); // "Null"
        buf[12..16].copy_from_slice(&0x74666F73u32.to_le_bytes()); // "soft"
        buf[16..20].copy_from_slice(&0x74736E49u32.to_le_bytes()); // "Inst"
        buf[20..24].copy_from_slice(&1024i32.to_le_bytes()); // length_of_header
        buf[24..28].copy_from_slice(&2048i32.to_le_bytes()); // length_of_all
        buf
    }

    #[test]
    fn scan_finds_header_at_offset_zero() {
        let fh = make_first_header(0);
        let mut overlay = vec![0u8; 2048];
        overlay[..28].copy_from_slice(&fh);

        let (off, _hdr) = scan_for_first_header(&overlay).unwrap();
        assert_eq!(off, 0);
    }

    #[test]
    fn scan_finds_header_at_512() {
        let fh = make_first_header(0);
        let mut overlay = vec![0u8; 2560];
        overlay[512..512 + 28].copy_from_slice(&fh);

        let (off, _hdr) = scan_for_first_header(&overlay).unwrap();
        assert_eq!(off, 512);
    }

    #[test]
    fn scan_finds_header_at_1024() {
        let fh = make_first_header(0x01); // uninstaller flag
        let mut overlay = vec![0u8; 3072];
        overlay[1024..1024 + 28].copy_from_slice(&fh);

        let (off, hdr) = scan_for_first_header(&overlay).unwrap();
        assert_eq!(off, 1024);
        assert!(hdr.is_uninstaller());
    }

    #[test]
    fn scan_fails_on_empty_overlay() {
        let overlay = vec![0u8; 512];
        assert_eq!(
            scan_for_first_header(&overlay),
            Err(Error::SignatureNotFound)
        );
    }

    #[test]
    fn scan_finds_header_at_non_aligned_offset() {
        let fh = make_first_header(0);
        let mut overlay = vec![0u8; 2304];
        // Place header at non-aligned offset 256, matching packed installers
        // that insert extra overlay bytes before the NSIS data.
        overlay[256..256 + 28].copy_from_slice(&fh);

        let (off, _hdr) = scan_for_first_header(&overlay).unwrap();
        assert_eq!(off, 256);
    }

    #[test]
    fn scan_ignores_non_aligned_header_with_implausible_lengths() {
        let mut fh = make_first_header(0);
        fh[20..24].copy_from_slice(&0i32.to_le_bytes());
        let mut overlay = vec![0u8; 2048];
        overlay[256..256 + 28].copy_from_slice(&fh);

        assert_eq!(
            scan_for_first_header(&overlay),
            Err(Error::SignatureNotFound)
        );
    }

    #[test]
    fn scan_accepts_aligned_header_that_extends_past_overlay() {
        let mut fh = make_first_header(0);
        fh[24..28].copy_from_slice(&4096i32.to_le_bytes());
        let mut overlay = vec![0u8; 2048];
        overlay[..28].copy_from_slice(&fh);

        let (off, _hdr) = scan_for_first_header(&overlay).unwrap();
        assert_eq!(off, 0);
    }
}
