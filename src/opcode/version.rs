//! NSIS version detection.
//!
//! Since the common header layout and opcode numbering vary between NSIS
//! versions, parsers must detect the version heuristically.
//!
//! Source: NRS `nsisfile.py` `_detect_version()` and Binary Refinery `xtnsis.py`.

use core::fmt;

use crate::strings::{self, StringEncoding};

/// Identifies the NSIS version for opcode resolution and header layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NsisVersion {
    /// NSIS 1.x (legacy `"nsisinstall"` signature).
    V1,
    /// NSIS 2.x (ANSI strings, ~67 opcodes).
    V2,
    /// NSIS 3.x (Unicode strings, ~71 opcodes).
    V3,
    /// Jim Park's Unicode fork.
    Park,
}

impl fmt::Display for NsisVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            NsisVersion::V1 => "NSIS 1",
            NsisVersion::V2 => "NSIS 2",
            NsisVersion::V3 => "NSIS 3",
            NsisVersion::Park => "NSIS Park",
        };
        f.write_str(s)
    }
}

/// Park sub-version, determined by the number of extra opcodes inserted.
///
/// The Park fork inserts extra opcodes into the opcode table:
/// - `Park1`: No extra opcodes before `EW_REGISTERDLL`.
/// - `Park2`: Inserts `GetFontVersion` at position 44.
/// - `Park3`: Inserts `GetFontVersion` and `GetFontName` at position 44.
///
/// Additionally, Unicode Park builds insert `EW_FPUTWS` and `EW_FGETWS`
/// before `EW_FSEEK`. Since Park is always Unicode, this always applies,
/// contributing a total shift of 2 (Park1), 3 (Park2), or 4 (Park3) for
/// opcodes >= `EW_FSEEK`.
///
/// Source: 7-Zip `NsisIn.cpp` `GetCmd()` and `DetectNsisType()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParkSubVersion {
    /// No extra opcodes before `EW_REGISTERDLL`.
    Park1,
    /// One extra opcode (`GetFontVersion`) before `EW_REGISTERDLL`.
    Park2,
    /// Two extra opcodes (`GetFontVersion`, `GetFontName`) before
    /// `EW_REGISTERDLL`.
    Park3,
}

/// Which NSIS 2.x variable layout an installer uses.
///
/// NSIS 2 gained built-in variables over its lifetime, and they were appended
/// to the same table the string encoding indexes into. An installer built
/// before those additions numbers its user-defined variables differently and
/// keeps the internal `$_OUTDIR` at a different index, so decoding a variable
/// reference correctly needs to know which layout applies.
///
/// | Variant | Built-in variables | `$_OUTDIR` index |
/// |---------|-------------------|------------------|
/// | [`UpTo203`](Self::UpTo203) | 29 | 29 |
/// | [`UpTo225`](Self::UpTo225) | 30 | 29 |
/// | [`From226`](Self::From226) | 32 | 31 |
///
/// # Source
///
/// 7-Zip `NsisIn.cpp`: `GET_NUM_INTERNAL_VARS`, `kVar_Spec_OUTDIR_225`, and
/// the `IsNsis200` / `IsNsis225` detection in `DetectNsisType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nsis2SubVersion {
    /// NSIS 2.03 and earlier.
    UpTo203,
    /// NSIS 2.04 through 2.25.
    UpTo225,
    /// NSIS 2.26 and later. Also the layout NSIS 3 inherited.
    From226,
}

impl Nsis2SubVersion {
    /// Returns the number of built-in variables in this layout.
    ///
    /// Variable indices at or above this are user-defined and render as
    /// `$_N_`, numbered from this base.
    #[inline]
    pub fn internal_var_count(self) -> u16 {
        match self {
            Nsis2SubVersion::UpTo203 => 29,
            Nsis2SubVersion::UpTo225 => 30,
            Nsis2SubVersion::From226 => 32,
        }
    }

    /// Returns the index of the internal `$_OUTDIR` variable.
    ///
    /// NSIS uses it to restore the output directory, and 7-Zip tracks writes
    /// to it when reconstructing destination paths.
    #[inline]
    pub fn spec_outdir_var_index(self) -> u16 {
        match self {
            Nsis2SubVersion::UpTo203 | Nsis2SubVersion::UpTo225 => 29,
            Nsis2SubVersion::From226 => 31,
        }
    }
}

impl fmt::Display for Nsis2SubVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Nsis2SubVersion::UpTo203 => "2.03 or earlier",
            Nsis2SubVersion::UpTo225 => "2.04-2.25",
            Nsis2SubVersion::From226 => "2.26 or later",
        };
        f.write_str(s)
    }
}

impl NsisVersion {
    /// Detects the NSIS version from the string encoding and table contents.
    ///
    /// # Heuristics
    ///
    /// 1. A legacy `"nsisinstall"` signature means NSIS 1.x.
    /// 2. Park's private-use special codes mean the Park fork.
    /// 3. A UTF-16LE table means NSIS 3.x — NSIS 2 has no Unicode build.
    /// 4. An ANSI table means NSIS 2.x *or* an NSIS 3 build that omitted
    ///    `Unicode true`. The two are told apart by which special-code range
    ///    the table uses; see [`strings::detect_ansi_nsis3`].
    ///
    /// `string_table` is the raw string block, used only for step 4.
    ///
    /// # Source
    ///
    /// 7-Zip `NsisIn.cpp`, `DetectNsisType`.
    pub fn detect(
        encoding: StringEncoding,
        is_legacy_signature: bool,
        string_table: &[u8],
    ) -> Self {
        if is_legacy_signature {
            return NsisVersion::V1;
        }

        match encoding {
            StringEncoding::Unicode => NsisVersion::V3,
            StringEncoding::Park => NsisVersion::Park,
            StringEncoding::Ansi => {
                if strings::detect_ansi_nsis3(string_table) {
                    NsisVersion::V3
                } else {
                    NsisVersion::V2
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ANSI table using the NSIS 2 code range: `0xFD` introduces a variable.
    const NSIS2_TABLE: &[u8] = &[0x00, 0xFD, 0x95, 0x80, 0x00];

    /// An ANSI table using the NSIS 3 code range: `0x03` introduces a variable,
    /// followed by the two-byte coded number whose first byte has bit 7 set.
    const NSIS3_TABLE: &[u8] = &[0x00, 0x03, 0x95, 0x80, 0x00];

    #[test]
    fn detect_v1_from_legacy() {
        assert_eq!(
            NsisVersion::detect(StringEncoding::Ansi, true, NSIS3_TABLE),
            NsisVersion::V1,
            "a legacy signature outranks the code range"
        );
    }

    #[test]
    fn detect_v2_from_ansi() {
        assert_eq!(
            NsisVersion::detect(StringEncoding::Ansi, false, NSIS2_TABLE),
            NsisVersion::V2
        );
    }

    #[test]
    fn detect_v3_from_ansi_with_nsis3_codes() {
        // ANSI does not imply NSIS 2: makensis 3.x builds an ANSI target
        // whenever a script omits `Unicode true`.
        assert_eq!(
            NsisVersion::detect(StringEncoding::Ansi, false, NSIS3_TABLE),
            NsisVersion::V3
        );
    }

    #[test]
    fn latin1_text_is_not_mistaken_for_nsis2_codes() {
        // 0xFC-0xFF are `ü ý þ ÿ` in ordinary text. On their own they say
        // nothing about the version, so an NSIS 3 table keeps its verdict.
        let mut table = NSIS3_TABLE.to_vec();
        // "grüße.txt" as Windows-1252 bytes.
        table.extend_from_slice(&[b'g', b'r', 0xFC, 0xDF, b'e', b'.', b't', b'x', b't', 0x00]);
        assert_eq!(
            NsisVersion::detect(StringEncoding::Ansi, false, &table),
            NsisVersion::V3
        );
    }

    #[test]
    fn detect_v3_from_unicode() {
        assert_eq!(
            NsisVersion::detect(StringEncoding::Unicode, false, &[]),
            NsisVersion::V3
        );
    }

    #[test]
    fn detect_park() {
        assert_eq!(
            NsisVersion::detect(StringEncoding::Park, false, &[]),
            NsisVersion::Park
        );
    }

    #[test]
    fn nsis2_layouts_have_distinct_variable_tables() {
        assert_eq!(Nsis2SubVersion::UpTo203.internal_var_count(), 29);
        assert_eq!(Nsis2SubVersion::UpTo225.internal_var_count(), 30);
        assert_eq!(Nsis2SubVersion::From226.internal_var_count(), 32);

        // `$_OUTDIR` moved only at 2.26.
        assert_eq!(Nsis2SubVersion::UpTo203.spec_outdir_var_index(), 29);
        assert_eq!(Nsis2SubVersion::UpTo225.spec_outdir_var_index(), 29);
        assert_eq!(Nsis2SubVersion::From226.spec_outdir_var_index(), 31);
    }
}
