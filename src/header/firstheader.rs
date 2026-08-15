//! NSIS FirstHeader structure.
//!
//! The FirstHeader is a 28-byte structure found in the PE overlay at a
//! 512-byte aligned offset. It contains the NSIS signature, flags, and
//! size information needed to locate and decompress the header block.
//!
//! Source: `fileform.h` from the NSIS source code.

use crate::{
    error::Error,
    util::{read_i32_le, read_u32_le},
};

/// NSIS standard signature: `0xDEADBEEF`.
pub const FH_SIG: u32 = 0xDEAD_BEEF;

/// Alternate signature found in some NSIS variants: `0xDEADBEED`.
pub const FH_SIG_ALT: u32 = 0xDEAD_BEED;

/// `"Null"` as a little-endian `u32`.
pub const FH_INT1: u32 = 0x6C6C_754E;
/// `"soft"` as a little-endian `u32`.
pub const FH_INT2: u32 = 0x7466_6F73;
/// `"Inst"` as a little-endian `u32`.
pub const FH_INT3: u32 = 0x7473_6E49;

/// `"nsis"` as a little-endian `u32` (NSIS 1.x legacy).
pub const FH_INT1_LEGACY: u32 = 0x7369_736E;
/// `"inst"` as a little-endian `u32` (NSIS 1.x legacy).
pub const FH_INT2_LEGACY: u32 = 0x7473_6E69;
/// `"all\0"` as a little-endian `u32` (NSIS 1.x legacy).
pub const FH_INT3_LEGACY: u32 = 0x006C_6C61;

/// Mask for valid FirstHeader flags.
pub const FH_FLAGS_MASK: u32 = 0x0F;
/// Flag: this is an uninstaller.
pub const FH_FLAGS_UNINSTALL: u32 = 0x01;
/// Flag: silent mode.
pub const FH_FLAGS_SILENT: u32 = 0x02;
/// Flag: CRC checking is disabled.
pub const FH_FLAGS_NO_CRC: u32 = 0x04;
/// Flag: force CRC even if `/NCRC` was passed.
pub const FH_FLAGS_FORCE_CRC: u32 = 0x08;

/// NSIS 1.x: the data block is followed by a CRC32.
///
/// NSIS 2.0 renumbered these flags. 1.x spends bit 0 on the CRC and has no way
/// to say a CRC is absent — where 2.x and later spend bit 0 on
/// [`FH_FLAGS_UNINSTALL`] and bit 2 on [`FH_FLAGS_NO_CRC`]. Reading a 1.x
/// installer through the later constants reports every CRC-protected one as an
/// uninstaller, so which set applies depends on the version. See
/// [`NsisInstaller::is_uninstaller`](crate::NsisInstaller::is_uninstaller).
///
/// From `FH_FLAGS_*` in the NSIS 1.98 `Source/exehead/fileform.h`.
pub const FH_V1_FLAGS_CRC: u32 = 0x01;

/// NSIS 1.x: this is an uninstaller. See [`FH_V1_FLAGS_CRC`].
pub const FH_V1_FLAGS_UNINSTALL: u32 = 0x02;

/// NSIS 1.x: the installer runs silently. See [`FH_V1_FLAGS_CRC`].
pub const FH_V1_FLAGS_SILENT: u32 = 0x04;

/// View type for the NSIS FirstHeader (28 bytes).
///
/// This is the first structure found in the PE overlay. It provides the
/// NSIS signature for identification and size fields for locating the
/// compressed header block and data block.
///
/// # Layout (7 x i32, little-endian)
///
/// | Offset | Field | Description |
/// |--------|-------|-------------|
/// | 0x00 | `flags` | `FH_FLAGS_*` bitmask |
/// | 0x04 | `siginfo` | Must be `0xDEADBEEF` |
/// | 0x08 | `nsinst[0]` | `0x6C6C754E` ("Null") |
/// | 0x0C | `nsinst[1]` | `0x74666F73` ("soft") |
/// | 0x10 | `nsinst[2]` | `0x74736E49` ("Inst") |
/// | 0x14 | `length_of_header` | Decompressed header size |
/// | 0x18 | `length_of_all_following_data` | Total size including CRC |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstHeader<'a> {
    bytes: &'a [u8],
}

impl<'a> FirstHeader<'a> {
    /// The on-disk size of a FirstHeader in bytes.
    pub const SIZE: usize = 28;

    /// Parses a FirstHeader from the start of `data`.
    ///
    /// Validates the signature (`siginfo` and `nsinst` magic) and flags.
    ///
    /// # Errors
    ///
    /// - [`Error::TooShort`] if `data.len() < 28`
    /// - [`Error::InvalidMagic`] if `siginfo` is not `0xDEADBEEF` or `0xDEADBEED`
    /// - [`Error::SignatureNotFound`] if `nsinst` does not match any known variant
    /// - [`Error::InvalidFirstHeaderFlags`] if flags contain bits outside `FH_FLAGS_MASK`
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "FirstHeader",
            });
        }

        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "FirstHeader",
        })?;
        let header = Self { bytes };

        // Validate siginfo.
        let sig = header.siginfo();
        if sig != FH_SIG && sig != FH_SIG_ALT {
            return Err(Error::InvalidMagic {
                expected: FH_SIG,
                got: sig,
            });
        }

        // Validate nsinst magic (standard or legacy).
        let ns = header.nsinst();
        let standard = ns[0] == FH_INT1 && ns[1] == FH_INT2 && ns[2] == FH_INT3;
        let legacy = ns[0] == FH_INT1_LEGACY && ns[1] == FH_INT2_LEGACY && ns[2] == FH_INT3_LEGACY;
        if !standard && !legacy {
            return Err(Error::SignatureNotFound);
        }

        // Validate flags.
        let flags = header.flags();
        if flags & !FH_FLAGS_MASK != 0 {
            return Err(Error::InvalidFirstHeaderFlags { flags });
        }

        Ok(header)
    }

    /// Returns the raw flags field.
    #[inline]
    pub fn flags(&self) -> u32 {
        read_u32_le(self.bytes, 0)
    }

    /// Returns the signature info field (should be `0xDEADBEEF` or `0xDEADBEED`).
    #[inline]
    pub fn siginfo(&self) -> u32 {
        read_u32_le(self.bytes, 4)
    }

    /// Returns the three `nsinst` magic values.
    #[inline]
    pub fn nsinst(&self) -> [u32; 3] {
        [
            read_u32_le(self.bytes, 8),
            read_u32_le(self.bytes, 12),
            read_u32_le(self.bytes, 16),
        ]
    }

    /// Returns the decompressed header size in bytes.
    ///
    /// This is the expected size after decompressing the header block that
    /// immediately follows the FirstHeader.
    #[inline]
    pub fn length_of_header(&self) -> i32 {
        read_i32_le(self.bytes, 20)
    }

    /// Returns the total size of all data following (and including) the FirstHeader.
    ///
    /// This includes the FirstHeader itself, compressed header, data block,
    /// and optional CRC.
    #[inline]
    pub fn length_of_all_following_data(&self) -> i32 {
        read_i32_le(self.bytes, 24)
    }

    /// Returns `true` if this is an uninstaller (`FH_FLAGS_UNINSTALL`).
    #[inline]
    pub fn is_uninstaller(&self) -> bool {
        self.flags() & FH_FLAGS_UNINSTALL != 0
    }

    /// Returns `true` if silent mode is enabled (`FH_FLAGS_SILENT`).
    #[inline]
    pub fn is_silent(&self) -> bool {
        self.flags() & FH_FLAGS_SILENT != 0
    }

    /// Returns `true` if CRC checking is disabled (`FH_FLAGS_NO_CRC`).
    #[inline]
    pub fn has_no_crc(&self) -> bool {
        self.flags() & FH_FLAGS_NO_CRC != 0
    }

    /// Returns `true` if CRC is forced even with `/NCRC` (`FH_FLAGS_FORCE_CRC`).
    #[inline]
    pub fn has_force_crc(&self) -> bool {
        self.flags() & FH_FLAGS_FORCE_CRC != 0
    }

    /// Returns `true` if this is a legacy NSIS 1.x signature (`"nsisinstall"`).
    #[inline]
    pub fn is_legacy(&self) -> bool {
        let ns = self.nsinst();
        ns[0] == FH_INT1_LEGACY && ns[1] == FH_INT2_LEGACY && ns[2] == FH_INT3_LEGACY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: builds a standard FirstHeader byte buffer.
    fn make_standard(flags: u32, header_len: i32, all_len: i32) -> [u8; 28] {
        let mut buf = [0u8; 28];
        buf[0..4].copy_from_slice(&flags.to_le_bytes());
        buf[4..8].copy_from_slice(&FH_SIG.to_le_bytes());
        buf[8..12].copy_from_slice(&FH_INT1.to_le_bytes());
        buf[12..16].copy_from_slice(&FH_INT2.to_le_bytes());
        buf[16..20].copy_from_slice(&FH_INT3.to_le_bytes());
        buf[20..24].copy_from_slice(&header_len.to_le_bytes());
        buf[24..28].copy_from_slice(&all_len.to_le_bytes());
        buf
    }

    /// Helper: builds a legacy NSIS 1.x FirstHeader byte buffer.
    fn make_legacy(flags: u32) -> [u8; 28] {
        let mut buf = [0u8; 28];
        buf[0..4].copy_from_slice(&flags.to_le_bytes());
        buf[4..8].copy_from_slice(&FH_SIG.to_le_bytes());
        buf[8..12].copy_from_slice(&FH_INT1_LEGACY.to_le_bytes());
        buf[12..16].copy_from_slice(&FH_INT2_LEGACY.to_le_bytes());
        buf[16..20].copy_from_slice(&FH_INT3_LEGACY.to_le_bytes());
        buf[20..24].copy_from_slice(&512i32.to_le_bytes());
        buf[24..28].copy_from_slice(&1024i32.to_le_bytes());
        buf
    }

    #[test]
    fn parse_valid_standard() {
        let buf = make_standard(0, 4096, 8192);
        let fh = FirstHeader::parse(&buf).unwrap();
        assert_eq!(fh.flags(), 0);
        assert_eq!(fh.siginfo(), FH_SIG);
        assert_eq!(fh.nsinst(), [FH_INT1, FH_INT2, FH_INT3]);
        assert_eq!(fh.length_of_header(), 4096);
        assert_eq!(fh.length_of_all_following_data(), 8192);
        assert!(!fh.is_uninstaller());
        assert!(!fh.is_silent());
        assert!(!fh.has_no_crc());
        assert!(!fh.has_force_crc());
        assert!(!fh.is_legacy());
    }

    #[test]
    fn parse_uninstaller_flag() {
        let buf = make_standard(FH_FLAGS_UNINSTALL, 100, 200);
        let fh = FirstHeader::parse(&buf).unwrap();
        assert!(fh.is_uninstaller());
        assert!(!fh.is_silent());
    }

    #[test]
    fn parse_silent_flag() {
        let buf = make_standard(FH_FLAGS_SILENT, 100, 200);
        let fh = FirstHeader::parse(&buf).unwrap();
        assert!(fh.is_silent());
    }

    #[test]
    fn parse_no_crc_flag() {
        let buf = make_standard(FH_FLAGS_NO_CRC, 100, 200);
        let fh = FirstHeader::parse(&buf).unwrap();
        assert!(fh.has_no_crc());
    }

    #[test]
    fn parse_combined_flags() {
        let flags = FH_FLAGS_UNINSTALL | FH_FLAGS_SILENT | FH_FLAGS_FORCE_CRC;
        let buf = make_standard(flags, 100, 200);
        let fh = FirstHeader::parse(&buf).unwrap();
        assert!(fh.is_uninstaller());
        assert!(fh.is_silent());
        assert!(fh.has_force_crc());
    }

    #[test]
    fn parse_legacy_signature() {
        let buf = make_legacy(0);
        let fh = FirstHeader::parse(&buf).unwrap();
        assert!(fh.is_legacy());
    }

    #[test]
    fn parse_alt_sig() {
        let mut buf = make_standard(0, 100, 200);
        // Replace siginfo with 0xDEADBEED.
        buf[4..8].copy_from_slice(&FH_SIG_ALT.to_le_bytes());
        let fh = FirstHeader::parse(&buf).unwrap();
        assert_eq!(fh.siginfo(), FH_SIG_ALT);
    }

    #[test]
    fn parse_too_short() {
        let buf = [0u8; 27];
        assert_eq!(
            FirstHeader::parse(&buf),
            Err(Error::TooShort {
                expected: 28,
                actual: 27,
                context: "FirstHeader",
            })
        );
    }

    #[test]
    fn parse_bad_siginfo() {
        let mut buf = make_standard(0, 100, 200);
        buf[4..8].copy_from_slice(&0x12345678u32.to_le_bytes());
        assert_eq!(
            FirstHeader::parse(&buf),
            Err(Error::InvalidMagic {
                expected: FH_SIG,
                got: 0x12345678,
            })
        );
    }

    #[test]
    fn parse_bad_nsinst() {
        let mut buf = make_standard(0, 100, 200);
        // Corrupt the nsinst[0] field.
        buf[8..12].copy_from_slice(&0x00000000u32.to_le_bytes());
        assert_eq!(FirstHeader::parse(&buf), Err(Error::SignatureNotFound));
    }

    #[test]
    fn parse_invalid_flags() {
        let buf = make_standard(0x10, 100, 200); // bit 4 is outside mask
        assert_eq!(
            FirstHeader::parse(&buf),
            Err(Error::InvalidFirstHeaderFlags { flags: 0x10 })
        );
    }

    #[test]
    fn parse_with_extra_trailing_data() {
        let fh_bytes = make_standard(0, 100, 200);
        let mut buf = vec![0u8; 1024];
        buf[..28].copy_from_slice(&fh_bytes);
        let fh = FirstHeader::parse(&buf).unwrap();
        assert_eq!(fh.length_of_header(), 100);
    }

    #[test]
    fn first_header_is_copy() {
        let buf = make_standard(0, 100, 200);
        let fh1 = FirstHeader::parse(&buf).unwrap();
        let fh2 = fh1; // Copy
        assert_eq!(fh1, fh2);
    }
}
