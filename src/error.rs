//! Error types for NSIS installer parsing.
//!
//! A single flat [`Error`] enum covers all failure modes across PE parsing,
//! overlay detection, decompression, header validation, string decoding,
//! and opcode resolution.

use core::fmt;
use std::error;

use goblin::error::Error as GoblinError;

/// All errors that can occur during NSIS installer parsing.
///
/// Each variant carries enough context for a useful diagnostic message.
/// The enum is intentionally flat (not hierarchical) to keep the API surface simple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    // -- PE-level errors --
    /// The underlying PE parser (`goblin`) failed.
    ///
    /// The inner string contains the stringified goblin error.
    /// We stringify because `goblin::error::Error` does not implement `Clone`/`Eq`.
    Goblin(String),

    /// The PE optional header magic is not `0x010B` (PE32).
    ///
    /// NSIS installers are always 32-bit PE executables.
    Not32Bit {
        /// The actual optional header magic value encountered.
        magic: u16,
    },

    /// A buffer or structure is too short.
    ///
    /// The parser expected at least `expected` bytes but found only `actual`.
    TooShort {
        /// Minimum bytes required.
        expected: usize,
        /// Actual bytes available.
        actual: usize,
        /// Human-readable name of the structure being parsed.
        context: &'static str,
    },

    // -- Overlay / FirstHeader errors --
    /// No PE overlay was found after the last section.
    ///
    /// NSIS data is appended as a PE overlay; a file with no overlay
    /// cannot be an NSIS installer.
    OverlayNotFound,

    /// No valid NSIS FirstHeader signature was found in the overlay.
    ///
    /// The scanner checked 512-byte aligned offsets first and then
    /// non-aligned fallback offsets for a structurally usable `0xDEADBEEF` +
    /// `"NullsoftInst"` magic sequence.
    SignatureNotFound,

    /// The FirstHeader flags field contains invalid bits.
    InvalidFirstHeaderFlags {
        /// The raw flags value that failed validation.
        flags: u32,
    },

    // -- Decompression errors --
    /// Decompression of an NSIS data block failed.
    DecompressionFailed {
        /// The compression method that was attempted (e.g., `"deflate"`).
        method: &'static str,
        /// A description of the failure.
        detail: String,
    },

    /// None of the supported compression methods could decompress the data.
    UnsupportedCompression,

    /// Decompression produced more output than the configured budget allows.
    ///
    /// Raised when a stream of unknown decompressed size (per-file, solid, or
    /// uninstaller data) expands past the caller's `max_decompressed_size`
    /// budget. This guards against decompression bombs; the offending stream is
    /// rejected rather than silently truncated.
    OutputTooLarge {
        /// The byte budget that was exceeded.
        limit: usize,
    },

    // -- Header structure errors --
    /// A block header references an offset beyond the decompressed data.
    InvalidBlockOffset {
        /// Name of the block (e.g., `"Sections"`).
        block: &'static str,
        /// The invalid offset value.
        offset: u32,
    },

    /// A magic value did not match the expected constant.
    InvalidMagic {
        /// The expected magic value.
        expected: u32,
        /// The actual value found.
        got: u32,
    },

    /// A block index is out of the valid range (0..8).
    InvalidBlockIndex {
        /// The invalid block index.
        index: usize,
    },

    // -- String errors --
    /// A string table offset is out of range.
    InvalidStringOffset {
        /// The offset that was out of bounds.
        offset: u32,
    },

    /// An unrecognized special code was encountered in the string table.
    InvalidSpecialCode {
        /// The code byte value.
        code: u8,
    },

    // -- Opcode errors --
    /// An entry's `which` field does not map to a known opcode.
    InvalidOpcode {
        /// The raw opcode value.
        which: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Goblin(msg) => write!(f, "PE parsing error: {msg}"),
            Error::Not32Bit { magic } => {
                write!(f, "not a PE32 file (optional header magic: 0x{magic:04X})")
            }
            Error::TooShort {
                expected,
                actual,
                context,
            } => write!(
                f,
                "{context}: expected at least {expected} bytes, got {actual}"
            ),
            Error::OverlayNotFound => {
                write!(f, "no PE overlay found after the last section")
            }
            Error::SignatureNotFound => {
                write!(f, "no usable NSIS signature found in the PE overlay")
            }
            Error::InvalidFirstHeaderFlags { flags } => {
                write!(f, "invalid FirstHeader flags: 0x{flags:08X}")
            }
            Error::DecompressionFailed { method, detail } => {
                write!(f, "{method} decompression failed: {detail}")
            }
            Error::UnsupportedCompression => {
                write!(f, "none of the supported compression methods succeeded")
            }
            Error::OutputTooLarge { limit } => {
                write!(f, "decompressed output exceeds limit of {limit} bytes")
            }
            Error::InvalidBlockOffset { block, offset } => {
                write!(f, "block {block}: offset 0x{offset:08X} is out of range")
            }
            Error::InvalidMagic { expected, got } => {
                write!(f, "bad magic: expected 0x{expected:08X}, got 0x{got:08X}")
            }
            Error::InvalidBlockIndex { index } => {
                write!(f, "block index {index} out of range (max 7)")
            }
            Error::InvalidStringOffset { offset } => {
                write!(f, "string table offset 0x{offset:08X} is out of range")
            }
            Error::InvalidSpecialCode { code } => {
                write!(f, "unrecognized special code 0x{code:02X} in string table")
            }
            Error::InvalidOpcode { which } => {
                write!(f, "invalid opcode: {which}")
            }
        }
    }
}

impl error::Error for Error {}

impl From<GoblinError> for Error {
    /// Converts a goblin parsing error into our [`Error::Goblin`] variant.
    fn from(e: GoblinError) -> Self {
        Error::Goblin(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_goblin() {
        let e = Error::Goblin("malformed PE".into());
        assert_eq!(e.to_string(), "PE parsing error: malformed PE");
    }

    #[test]
    fn display_not_32bit() {
        let e = Error::Not32Bit { magic: 0x020B };
        assert!(e.to_string().contains("0x020B"));
    }

    #[test]
    fn display_too_short() {
        let e = Error::TooShort {
            expected: 28,
            actual: 10,
            context: "FirstHeader",
        };
        let s = e.to_string();
        assert!(s.contains("FirstHeader"));
        assert!(s.contains("28"));
        assert!(s.contains("10"));
    }

    #[test]
    fn display_overlay_not_found() {
        let e = Error::OverlayNotFound;
        assert!(e.to_string().contains("overlay"));
    }

    #[test]
    fn display_signature_not_found() {
        let e = Error::SignatureNotFound;
        assert!(e.to_string().contains("NSIS signature"));
    }

    #[test]
    fn display_invalid_first_header_flags() {
        let e = Error::InvalidFirstHeaderFlags { flags: 0xFF };
        assert!(e.to_string().contains("000000FF"));
    }

    #[test]
    fn display_decompression_failed() {
        let e = Error::DecompressionFailed {
            method: "deflate",
            detail: "unexpected EOF".into(),
        };
        let s = e.to_string();
        assert!(s.contains("deflate"));
        assert!(s.contains("unexpected EOF"));
    }

    #[test]
    fn display_unsupported_compression() {
        let e = Error::UnsupportedCompression;
        assert!(e.to_string().contains("compression"));
    }

    #[test]
    fn display_output_too_large() {
        let e = Error::OutputTooLarge {
            limit: 64 * 1024 * 1024,
        };
        let s = e.to_string();
        assert!(s.contains("exceeds limit"));
        assert!(s.contains(&(64 * 1024 * 1024).to_string()));
    }

    #[test]
    fn display_invalid_block_offset() {
        let e = Error::InvalidBlockOffset {
            block: "Sections",
            offset: 0xFFFF,
        };
        let s = e.to_string();
        assert!(s.contains("Sections"));
        assert!(s.contains("0000FFFF"));
    }

    #[test]
    fn display_invalid_magic() {
        let e = Error::InvalidMagic {
            expected: 0xDEADBEEF,
            got: 0x00000000,
        };
        let s = e.to_string();
        assert!(s.contains("DEADBEEF"));
        assert!(s.contains("00000000"));
    }

    #[test]
    fn display_invalid_block_index() {
        let e = Error::InvalidBlockIndex { index: 9 };
        assert!(e.to_string().contains("9"));
    }

    #[test]
    fn display_invalid_string_offset() {
        let e = Error::InvalidStringOffset { offset: 0x1234 };
        assert!(e.to_string().contains("00001234"));
    }

    #[test]
    fn display_invalid_special_code() {
        let e = Error::InvalidSpecialCode { code: 0x05 };
        assert!(e.to_string().contains("0x05"));
    }

    #[test]
    fn display_invalid_opcode() {
        let e = Error::InvalidOpcode { which: 99 };
        assert!(e.to_string().contains("99"));
    }

    #[test]
    fn error_is_clone_eq() {
        let e1 = Error::OverlayNotFound;
        let e2 = e1.clone();
        assert_eq!(e1, e2);
    }

    #[test]
    fn error_trait_impl() {
        let e: Box<dyn error::Error> = Box::new(Error::OverlayNotFound);
        let _ = e.to_string();
    }

    #[test]
    fn from_goblin_error() {
        let e = Error::Goblin("test error".into());
        assert!(matches!(e, Error::Goblin(_)));
    }
}
