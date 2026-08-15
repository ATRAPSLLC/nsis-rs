//! NSIS string table parsing.
//!
//! NSIS strings use special encoding with embedded variable references,
//! shell folder constants, and language string references. Three encoding
//! variants exist depending on the NSIS version:
//!
//! - **ANSI** (NSIS 2.x): Single-byte characters with 1-byte special codes.
//! - **Unicode** (NSIS 3.x): UTF-16LE characters with 16-bit special codes.
//! - **Park** (Jim Park's fork): Hybrid ANSI/Unicode encoding.
//!
//! Source: `fileform.h` and `strings.py` from the NRS parser.

pub mod ansi;
pub mod park;
pub mod unicode;

use core::fmt;
use std::borrow::Cow;

use crate::error::Error;

/// Identifies the string encoding used by an NSIS installer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringEncoding {
    /// Single-byte ANSI encoding (NSIS 2.x default).
    Ansi,
    /// UTF-16LE encoding (NSIS 3.x).
    Unicode,
    /// Jim Park's Unicode fork (hybrid encoding).
    Park,
}

impl fmt::Display for StringEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            StringEncoding::Ansi => "ANSI",
            StringEncoding::Unicode => "Unicode",
            StringEncoding::Park => "Park",
        };
        f.write_str(s)
    }
}

/// Detects the string encoding from the first bytes of the string table.
///
/// The three encoding variants are:
///
/// - **ANSI** (NSIS 2.x and 3.x ANSI builds): Single-byte characters.
///   String index 0 is a single `0x00` null, so `byte[0]=0, byte[1]!=0`.
///   Special codes are `0x01-0x04` (NSIS 3) or `0xFC-0xFF` (NSIS 2) —
///   the ANSI reader handles both transparently.
///
/// - **Unicode** (NSIS 3.x Unicode builds): UTF-16LE characters.
///   String index 0 is `0x00 0x00` (2-byte null). Special codes are
///   `0x0001-0x0004` as u16 code units.
///
/// - **Park** (Jim Park's Unicode fork): UTF-16LE characters.
///   String index 0 is `0x00 0x00` (2-byte null). Special codes are
///   `0xE000-0xE003` (Unicode Private Use Area).
///
/// Detection: if the table starts with `0x00 0x00` it's UTF-16LE (Unicode
/// or Park). We then scan for the first special code to distinguish them.
/// If it starts with `0x00 XX` where `XX != 0`, it's ANSI.
///
/// Sources: 7-Zip `NsisIn.cpp`, Binary Refinery `xtnsis.py`, NRS `strings/`.
pub fn detect_encoding(string_table: &[u8]) -> StringEncoding {
    if string_table.len() < 4 {
        return StringEncoding::Ansi;
    }

    // ANSI tables start with a single 0x00 null byte for string index 0,
    // followed immediately by non-zero content. UTF-16LE tables start
    // with 0x00 0x00 (a 2-byte null).
    if string_table.first().copied() != Some(0) || string_table.get(1).copied() != Some(0) {
        return StringEncoding::Ansi;
    }

    // First two bytes are 0x00 0x00 — this is a UTF-16LE string table.
    // Scan for the first special code to distinguish NSIS 3 Unicode from Park.
    let limit = string_table.len().min(4096) & !1;
    for i in (2..limit).step_by(2) {
        let Some(pair) = string_table.get(i..).and_then(|s| s.first_chunk::<2>()) else {
            break;
        };
        let ch = u16::from_le_bytes(*pair);
        if ch == 0 {
            continue;
        }
        // NSIS 3 Unicode special codes.
        if (0x0001..=0x0004).contains(&ch) {
            return StringEncoding::Unicode;
        }
        // Park special codes (Unicode Private Use Area).
        if (0xE000..=0xE003).contains(&ch) {
            return StringEncoding::Park;
        }
    }

    // No special codes found — default to Unicode (more common than Park).
    StringEncoding::Unicode
}

/// Returns `true` if an ANSI string table uses the NSIS 3 special-code range.
///
/// ANSI alone does not imply NSIS 2: makensis 3.x compiles an ANSI target
/// whenever a script omits `Unicode true`, and NSIS 3 moved the special codes
/// from `0xFC-0xFF` down to `0x01-0x04`. The two ranges cannot be told apart
/// by looking at a single byte, because `0xFC-0xFF` are also the ordinary
/// Latin-1 characters `ü ý þ ÿ` and `0x01-0x04` are legitimate control
/// characters.
///
/// The reliable marker is a variable reference at the start of a string: a
/// null terminator, then the NSIS 3 variable code `0x03`, then the first byte
/// of the two-byte coded number, which always has its high bit set. NSIS 2
/// writes `0xFD` there instead.
///
/// # Source
///
/// 7-Zip `NsisIn.cpp`, `DetectNsisType` (the non-Unicode branch).
pub fn detect_ansi_nsis3(string_table: &[u8]) -> bool {
    // A string table always starts with the empty string, so index 0 is a
    // terminator and the scan can look at every following byte triple.
    string_table.windows(3).any(|w| {
        w.first() == Some(&0)
            && w.get(1) == Some(&NS3_VAR)
            && w.get(2).is_some_and(|c| c & 0x80 != 0)
    })
}

/// NSIS 3 ANSI special code introducing a variable reference.
///
/// Mirrors `NS_3_CODE_VAR` in 7-Zip's `NsisIn.cpp`, and the ANSI reader's own
/// `NS3_VAR`.
const NS3_VAR: u8 = 0x03;

/// Decodes a 14-bit NSIS coded short from 2 bytes.
///
/// The NSIS encoding stores a 14-bit value across two bytes, each with
/// the high bit set (OR'd with 0x80):
///
/// ```text
/// CODE_SHORT(x) = ((x & 0x7F) | ((x & 0x3F80) << 1) | 0x8080)
/// DECODE_SHORT(c) = ((c[1] & 0x7F) << 7) | (c[0] & 0x7F)
/// ```
///
/// # Source
///
/// `fileform.h`: `CODE_SHORT` and `DECODE_SHORT` macros.
#[inline]
pub fn decode_short(b0: u8, b1: u8) -> u16 {
    (((b1 & 0x7F) as u16) << 7) | ((b0 & 0x7F) as u16)
}

/// Encodes a 14-bit value into the NSIS coded short format.
///
/// This is the inverse of [`decode_short`].
#[inline]
pub fn encode_short(value: u16) -> (u8, u8) {
    let b0 = ((value & 0x7F) | 0x80) as u8;
    let b1 = (((value >> 7) & 0x7F) | 0x80) as u8;
    (b0, b1)
}

/// A segment of a decoded NSIS string.
///
/// NSIS strings are not plain text — they contain embedded references to
/// variables, shell folders, and language strings that are resolved at
/// install time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringSegment {
    /// Literal text content.
    Literal(String),
    /// Variable reference, e.g., `$INSTDIR`.
    ///
    /// The value is the variable index (0..30).
    Variable(u16),
    /// Shell folder constant, e.g., `$APPDATA`.
    ///
    /// The value is the CSIDL constant.
    ShellFolder(u16),
    /// Language string reference.
    ///
    /// The value is the language string index.
    LangString(u16),
}

/// A decoded NSIS string composed of literal and special-code segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NsisString {
    /// The segments that make up this string.
    pub segments: Vec<StringSegment>,
}

impl NsisString {
    /// Returns `true` if the string has no segments.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Renders this string as a relative filesystem path suitable for extraction.
    ///
    /// NSIS variable references are mapped to directory names:
    ///
    /// | Variable | Mapped to |
    /// |----------|-----------|
    /// | `$INSTDIR`, `$OUTDIR` | *(root — no prefix)* |
    /// | `$PLUGINSDIR` | `_plugins/` |
    /// | `$TEMP` | `_temp/` |
    /// | `$EXEDIR` | `_exedir/` |
    /// | `${SHELL:N}` | `_shell_N/` |
    /// | Other `$VAR` | `_VAR/` |
    ///
    /// Backslashes are normalized to forward slashes, path traversal (`..`)
    /// is replaced with `_`, and leading slashes are stripped.
    pub fn to_path(&self) -> String {
        let mut result = String::new();
        for seg in &self.segments {
            match seg {
                StringSegment::Literal(s) => result.push_str(s),
                StringSegment::Variable(idx) => {
                    match *idx {
                        // $INSTDIR and $OUTDIR map to the extraction root.
                        21 | 22 => {}
                        25 => result.push_str("_temp"),
                        26 => result.push_str("_plugins"),
                        23 => result.push_str("_exedir"),
                        _ => {
                            let name = variable_name(*idx);
                            let stripped = name.strip_prefix('$').unwrap_or(&name);
                            result.push('_');
                            result.push_str(stripped);
                        }
                    }
                }
                StringSegment::ShellFolder(raw) => {
                    let name = shell_folder_name(*raw);
                    let stripped = name.strip_prefix('$').unwrap_or(&name);
                    result.push('_');
                    result.push_str(stripped);
                }
                StringSegment::LangString(_) => {
                    // Language strings can't be resolved statically; skip.
                }
            }
        }

        // Normalize separators and sanitize.
        result
            .replace('\\', "/")
            .replace("//", "/")
            .replace("..", "_")
            .trim_start_matches('/')
            .to_string()
    }
}

impl fmt::Display for NsisString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for seg in &self.segments {
            match seg {
                StringSegment::Literal(s) => write!(f, "{s}")?,
                StringSegment::Variable(idx) => {
                    write!(f, "{}", variable_name(*idx))?;
                }
                StringSegment::ShellFolder(raw) => {
                    write!(f, "{}", shell_folder_name(*raw))?;
                }
                StringSegment::LangString(idx) => {
                    write!(f, "${{LANG:{idx}}}")?;
                }
            }
        }
        Ok(())
    }
}

/// Reads a string from the string table at the given byte offset.
///
/// Dispatches to the appropriate encoding-specific reader.
///
/// # Errors
///
/// Returns [`crate::error::Error::InvalidStringOffset`] if the offset is beyond
/// the string table.
pub fn read_nsis_string(
    table: &[u8],
    offset: usize,
    encoding: StringEncoding,
) -> Result<NsisString, Error> {
    if offset >= table.len() {
        return Err(Error::InvalidStringOffset {
            offset: offset as u32,
        });
    }

    match encoding {
        StringEncoding::Ansi => ansi::read_ansi_string(table, offset),
        StringEncoding::Unicode => unicode::read_unicode_string(table, offset),
        StringEncoding::Park => park::read_park_string(table, offset),
    }
}

/// Reads a string from a header block's string table by TCHAR offset.
///
/// String references inside NSIS structures — section `name_ptr` fields, entry
/// parameter slots — are TCHAR indices rather than byte offsets, so the offset
/// is scaled by the encoding's character size before reading. A negative offset
/// is not a reference at all and yields an empty string.
///
/// # Arguments
///
/// - `header_data`: the decompressed header block
/// - `string_block_offset`: byte offset of the string table within it
/// - `encoding`: the table's encoding
/// - `offset`: TCHAR index of the string to read
///
/// # Errors
///
/// Returns [`Error::InvalidStringOffset`] if the offset lies beyond the table.
pub fn read_string_at(
    header_data: &[u8],
    string_block_offset: usize,
    encoding: StringEncoding,
    offset: i32,
) -> Result<NsisString, Error> {
    if offset < 0 {
        return Ok(NsisString {
            segments: Vec::new(),
        });
    }
    // Both Unicode and Park are UTF-16LE, so their TCHAR is 2 bytes.
    let char_size = match encoding {
        StringEncoding::Unicode | StringEncoding::Park => 2,
        StringEncoding::Ansi => 1,
    };
    let abs_offset =
        string_block_offset.saturating_add((offset as usize).saturating_mul(char_size));
    read_nsis_string(header_data, abs_offset, encoding)
}

/// Number of built-in (internal) NSIS variables.
const NUM_INTERNAL_VARS: u16 = 32;

/// Built-in variable names indexed by variable number.
///
/// Indices 0-9 are `$0`-`$9`, 10-19 are `$R0`-`$R9`, 20-31 are system
/// variables. This table covers all 32 built-in indices.
static VARIABLE_NAMES: [&str; 32] = [
    "$0",
    "$1",
    "$2",
    "$3",
    "$4",
    "$5",
    "$6",
    "$7",
    "$8",
    "$9", // 0-9
    "$R0",
    "$R1",
    "$R2",
    "$R3",
    "$R4",
    "$R5",
    "$R6",
    "$R7",
    "$R8",
    "$R9",         // 10-19
    "$CMDLINE",    // 20
    "$INSTDIR",    // 21
    "$OUTDIR",     // 22
    "$EXEDIR",     // 23
    "$LANGUAGE",   // 24
    "$TEMP",       // 25
    "$PLUGINSDIR", // 26
    "$EXEPATH",    // 27
    "$EXEFILE",    // 28
    "$HWNDPARENT", // 29
    "$_CLICK",     // 30
    "$_OUTDIR",    // 31
];

/// Returns the conventional NSIS variable name for a given index.
///
/// Returns a `&'static str` for built-in variables (0-31) and a heap-allocated
/// `String` only for user-defined variables (32+), displayed as `$_N_`.
///
/// Source: 7-Zip `NsisIn.cpp` `GetVar2`, `state.h`.
pub fn variable_name(index: u16) -> Cow<'static, str> {
    if let Some(name) = VARIABLE_NAMES.get(index as usize) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("$_{}_", index.saturating_sub(NUM_INTERNAL_VARS)))
    }
}

/// Shell folder name table, indexed by CSIDL constant.
///
/// Source: 7-Zip `NsisIn.cpp` `kShellStrings[]` array.
static SHELL_FOLDER_NAMES: &[Option<&str>] = &[
    Some("DESKTOP"),                // 0  CSIDL_DESKTOP
    Some("INTERNET"),               // 1  CSIDL_INTERNET
    Some("SMPROGRAMS"),             // 2  CSIDL_PROGRAMS
    Some("CONTROLS"),               // 3  CSIDL_CONTROLS
    Some("PRINTERS"),               // 4  CSIDL_PRINTERS
    Some("DOCUMENTS"),              // 5  CSIDL_PERSONAL
    Some("FAVORITES"),              // 6  CSIDL_FAVORITES
    Some("SMSTARTUP"),              // 7  CSIDL_STARTUP
    Some("RECENT"),                 // 8  CSIDL_RECENT
    Some("SENDTO"),                 // 9  CSIDL_SENDTO
    Some("BITBUCKET"),              // 10 CSIDL_BITBUCKET
    Some("STARTMENU"),              // 11 CSIDL_STARTMENU
    None,                           // 12 CSIDL_MYDOCUMENTS (= PERSONAL)
    Some("MUSIC"),                  // 13 CSIDL_MYMUSIC
    Some("VIDEOS"),                 // 14 CSIDL_MYVIDEO
    None,                           // 15
    Some("DESKTOP"),                // 16 CSIDL_DESKTOPDIRECTORY
    Some("DRIVES"),                 // 17 CSIDL_DRIVES
    Some("NETWORK"),                // 18 CSIDL_NETWORK
    Some("NETHOOD"),                // 19 CSIDL_NETHOOD
    Some("FONTS"),                  // 20 CSIDL_FONTS
    Some("TEMPLATES"),              // 21 CSIDL_TEMPLATES
    Some("STARTMENU"),              // 22 CSIDL_COMMON_STARTMENU
    Some("SMPROGRAMS"),             // 23 CSIDL_COMMON_PROGRAMS
    Some("SMSTARTUP"),              // 24 CSIDL_COMMON_STARTUP
    Some("DESKTOP"),                // 25 CSIDL_COMMON_DESKTOPDIRECTORY
    Some("APPDATA"),                // 26 CSIDL_APPDATA
    Some("PRINTHOOD"),              // 27 CSIDL_PRINTHOOD
    Some("LOCALAPPDATA"),           // 28 CSIDL_LOCAL_APPDATA
    Some("ALTSTARTUP"),             // 29 CSIDL_ALTSTARTUP
    Some("ALTSTARTUP"),             // 30 CSIDL_COMMON_ALTSTARTUP
    Some("FAVORITES"),              // 31 CSIDL_COMMON_FAVORITES
    Some("INTERNET_CACHE"),         // 32 CSIDL_INTERNET_CACHE
    Some("COOKIES"),                // 33 CSIDL_COOKIES
    Some("HISTORY"),                // 34 CSIDL_HISTORY
    Some("APPDATA"),                // 35 CSIDL_COMMON_APPDATA
    Some("WINDIR"),                 // 36 CSIDL_WINDOWS
    Some("SYSDIR"),                 // 37 CSIDL_SYSTEM
    Some("PROGRAMFILES"),           // 38 CSIDL_PROGRAM_FILES
    Some("PICTURES"),               // 39 CSIDL_MYPICTURES
    Some("PROFILE"),                // 40 CSIDL_PROFILE
    Some("SYSTEMX86"),              // 41 CSIDL_SYSTEMX86
    Some("PROGRAMFILESX86"),        // 42 CSIDL_PROGRAM_FILESX86
    Some("PROGRAMFILES_COMMON"),    // 43 CSIDL_PROGRAM_FILES_COMMON
    Some("PROGRAMFILES_COMMONX86"), // 44 CSIDL_PROGRAM_FILES_COMMONX86
    Some("TEMPLATES"),              // 45 CSIDL_COMMON_TEMPLATES
    Some("DOCUMENTS"),              // 46 CSIDL_COMMON_DOCUMENTS
    Some("ADMINTOOLS"),             // 47 CSIDL_COMMON_ADMINTOOLS
    Some("ADMINTOOLS"),             // 48 CSIDL_ADMINTOOLS
    Some("CONNECTIONS"),            // 49 CSIDL_CONNECTIONS
    None,                           // 50
    None,                           // 51
    None,                           // 52
    Some("MUSIC"),                  // 53 CSIDL_COMMON_MUSIC
    Some("PICTURES"),               // 54 CSIDL_COMMON_PICTURES
    Some("VIDEOS"),                 // 55 CSIDL_COMMON_VIDEO
    Some("RESOURCES"),              // 56 CSIDL_RESOURCES
    Some("RESOURCES_LOCALIZED"),    // 57 CSIDL_RESOURCES_LOCALIZED
    Some("COMMON_OEM_LINKS"),       // 58 CSIDL_COMMON_OEM_LINKS
    Some("CDBURN_AREA"),            // 59 CSIDL_CDBURN_AREA
    None,                           // 60
    Some("COMPUTERSNEARME"),        // 61 CSIDL_COMPUTERSNEARME
];

/// Resolves a shell folder value to a display name.
///
/// The `raw` value for NSIS 3 Unicode and Park is a u16 where:
/// - Low byte (`raw & 0xFF`): primary shell folder ID (CSIDL) or registry
///   mode flag (if bit 7 is set)
/// - High byte (`raw >> 8`): fallback shell folder ID
///
/// For ANSI mode, the value is a 14-bit decoded index that maps directly
/// to the CSIDL table.
///
/// Source: 7-Zip `NsisIn.cpp` `GetShellString`.
pub fn shell_folder_name(raw: u16) -> String {
    let index1 = (raw & 0xFF) as usize;
    let index2 = (raw >> 8) as usize;

    // Registry key lookup mode (bit 7 set in low byte).
    if index1 & 0x80 != 0 {
        let is_64 = index1 & 0x40 != 0;
        let suffix = if is_64 { "64" } else { "" };
        return format!("$PROGRAMFILES{suffix}");
    }

    // Standard CSIDL lookup — try primary, then fallback.
    if let Some(Some(name)) = SHELL_FOLDER_NAMES.get(index1) {
        return format!("${name}");
    }
    if let Some(Some(name)) = SHELL_FOLDER_NAMES.get(index2) {
        return format!("${name}");
    }

    format!("$SHELL({index1},{index2})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_encoding_unicode() {
        // Unicode string table: \0\0 (null terminator) then ASCII in UTF-16LE.
        // "H" in UTF-16LE is [0x48, 0x00].
        assert_eq!(
            detect_encoding(&[0x00, 0x00, 0x48, 0x00]),
            StringEncoding::Unicode
        );
        // Multiple nulls then UTF-16LE content.
        assert_eq!(
            detect_encoding(&[0x00, 0x00, 0x00, 0x00, 0x41, 0x00, 0x42, 0x00]),
            StringEncoding::Unicode
        );
    }

    #[test]
    fn detect_encoding_park() {
        // Park: UTF-16LE table (starts 0x00 0x00) with 0xE000+ special codes.
        // 0x00 0x00 (null), then 0xE001 (PARK_CODE_VAR) as LE bytes [0x01, 0xE0].
        assert_eq!(
            detect_encoding(&[0x00, 0x00, 0x01, 0xE0, 0x15, 0x00]),
            StringEncoding::Park
        );
        // 0xE002 (PARK_CODE_SHELL).
        assert_eq!(
            detect_encoding(&[0x00, 0x00, 0x02, 0xE0, 0x1A, 0x00]),
            StringEncoding::Park
        );
    }

    #[test]
    fn detect_encoding_ansi() {
        // ANSI: first byte is 0x00 but second byte is non-zero (single-byte null).
        assert_eq!(
            detect_encoding(&[0x00, 0x50, 0x72, 0x6F]),
            StringEncoding::Ansi
        );
        // ANSI with non-null first byte (direct string content).
        assert_eq!(
            detect_encoding(&[0x41, 0x42, 0x43, 0x00]),
            StringEncoding::Ansi
        );
        // NSIS 2 ANSI: \0 followed by 0xFE (NS2_SHELL_CODE) — still ANSI, not Park.
        assert_eq!(
            detect_encoding(&[0x00, 0xFE, 0x1A, 0x23]),
            StringEncoding::Ansi
        );
    }

    #[test]
    fn detect_encoding_empty_or_short() {
        assert_eq!(detect_encoding(&[]), StringEncoding::Ansi);
        assert_eq!(detect_encoding(&[0x00]), StringEncoding::Ansi);
        assert_eq!(detect_encoding(&[0x00, 0x00]), StringEncoding::Ansi);
    }

    #[test]
    fn decode_short_values() {
        // Encode value 0: (0x80, 0x80) → decode = 0
        assert_eq!(decode_short(0x80, 0x80), 0);

        // Encode value 1: b0 = 0x81, b1 = 0x80 → decode = 1
        assert_eq!(decode_short(0x81, 0x80), 1);

        // Maximum 14-bit value: 0x3FFF = 16383
        let (b0, b1) = encode_short(0x3FFF);
        assert_eq!(decode_short(b0, b1), 0x3FFF);
    }

    #[test]
    fn encode_decode_roundtrip() {
        for val in [0u16, 1, 127, 128, 255, 1000, 0x3FFF] {
            let (b0, b1) = encode_short(val);
            assert_eq!(decode_short(b0, b1), val, "roundtrip failed for {val}");
            // Both bytes must have high bit set.
            assert!(b0 & 0x80 != 0);
            assert!(b1 & 0x80 != 0);
        }
    }

    #[test]
    fn variable_names() {
        assert_eq!(variable_name(0), "$0");
        assert_eq!(variable_name(9), "$9");
        assert_eq!(variable_name(10), "$R0");
        assert_eq!(variable_name(19), "$R9");
        assert_eq!(variable_name(21), "$INSTDIR");
        assert_eq!(variable_name(25), "$TEMP");
        assert_eq!(variable_name(26), "$PLUGINSDIR");
        assert_eq!(variable_name(30), "$_CLICK");
        assert_eq!(variable_name(31), "$_OUTDIR");
        // User-defined variables: index 32+ → $_N_
        assert_eq!(variable_name(32).as_ref(), "$_0_");
        assert_eq!(variable_name(33).as_ref(), "$_1_");
    }

    #[test]
    fn nsis_string_display() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable(21),
                StringSegment::Literal("\\program.exe".into()),
            ],
        };
        assert_eq!(s.to_string(), "$INSTDIR\\program.exe");
    }

    #[test]
    fn nsis_string_display_complex() {
        let s = NsisString {
            segments: vec![
                StringSegment::LangString(5),
                StringSegment::Literal(" in ".into()),
                StringSegment::ShellFolder(0x001A),
            ],
        };
        assert_eq!(s.to_string(), "${LANG:5} in $APPDATA");
    }

    #[test]
    fn read_string_out_of_bounds() {
        let table = b"hello\0";
        let result = read_nsis_string(table, 100, StringEncoding::Ansi);
        assert!(result.is_err());
    }

    #[test]
    fn to_path_instdir() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable(21), // $INSTDIR
                StringSegment::Literal("\\program.exe".into()),
            ],
        };
        assert_eq!(s.to_path(), "program.exe");
    }

    #[test]
    fn to_path_pluginsdir() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable(26), // $PLUGINSDIR
                StringSegment::Literal("\\System.dll".into()),
            ],
        };
        assert_eq!(s.to_path(), "_plugins/System.dll");
    }

    #[test]
    fn to_path_temp() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable(25), // $TEMP
                StringSegment::Literal("\\payload.bin".into()),
            ],
        };
        assert_eq!(s.to_path(), "_temp/payload.bin");
    }

    #[test]
    fn to_path_nested() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable(21), // $INSTDIR
                StringSegment::Literal("\\Lang\\en_US.ini".into()),
            ],
        };
        assert_eq!(s.to_path(), "Lang/en_US.ini");
    }

    #[test]
    fn to_path_shell_folder() {
        let s = NsisString {
            segments: vec![
                StringSegment::ShellFolder(0x1A),
                StringSegment::Literal("\\MyApp\\config.ini".into()),
            ],
        };
        assert_eq!(s.to_path(), "_APPDATA/MyApp/config.ini");
    }

    #[test]
    fn to_path_no_variable() {
        let s = NsisString {
            segments: vec![StringSegment::Literal("readme.txt".into())],
        };
        assert_eq!(s.to_path(), "readme.txt");
    }
}
