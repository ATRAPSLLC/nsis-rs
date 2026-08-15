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

use crate::{error::Error, strings::ansi::AnsiCodeRange};

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
///
/// References are resolved to their names while decoding, not while rendering,
/// because resolving them needs the installer's variable layout and access to
/// the string table itself. The raw indices are kept alongside the resolved
/// form so callers can still work from them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StringSegment {
    /// Literal text content.
    Literal(String),
    /// Variable reference, e.g. `$INSTDIR`.
    Variable {
        /// Variable index, as stored in the string table.
        index: u16,
        /// Rendered name, e.g. `$INSTDIR` for a built-in or `$_3_` for a
        /// user-defined variable. Borrowed for built-ins.
        name: Cow<'static, str>,
    },
    /// Shell folder constant, e.g. `$APPDATA`.
    ///
    /// NSIS stores two folder ids: a primary and a fallback used when the
    /// primary is not available on the running system.
    ShellFolder {
        /// Primary folder id (a CSIDL constant), or a registry lookup when bit
        /// 7 is set.
        primary: u8,
        /// Fallback folder id, used when `primary` names nothing known.
        fallback: u8,
        /// What the pair resolves to.
        target: ShellTarget,
    },
    /// Language string reference, resolved from the language table at install
    /// time and so unresolvable here.
    LangString(u16),
}

/// What a [`StringSegment::ShellFolder`] refers to.
///
/// Most shell folders are a CSIDL constant, but NSIS also encodes two
/// directories that Windows keeps in the registry rather than as a CSIDL. In
/// that case the low six bits of the primary id are an offset to the registry
/// value name *within the same string table*.
///
/// # Source
///
/// 7-Zip `NsisIn.cpp` `GetShellString`; Binary Refinery `xtnsis.py`
/// `_string_code_shell`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShellTarget {
    /// A known shell folder, named without the leading `$` (e.g. `APPDATA`).
    Csidl(&'static str),
    /// The Program Files directory, read from the registry at install time.
    ProgramFiles {
        /// Whether the 64-bit view of the registry is used.
        wow64: bool,
    },
    /// The Common Files directory, read from the registry at install time.
    CommonFiles {
        /// Whether the 64-bit view of the registry is used.
        wow64: bool,
    },
    /// Some other registry value under
    /// `HKLM\Software\Microsoft\Windows\CurrentVersion`.
    RegistryValue {
        /// The value name NSIS reads.
        name: String,
        /// Whether the 64-bit view of the registry is used.
        wow64: bool,
    },
    /// Neither id names a known folder, and it is not a registry lookup.
    Unresolved,
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

    /// Renders this string as a relative filesystem path safe to extract to.
    ///
    /// NSIS variable references become directory names, since none of them can
    /// be resolved without running the installer:
    ///
    /// | Reference | Mapped to |
    /// |----------|-----------|
    /// | `$INSTDIR`, `$OUTDIR` | *(extraction root — no prefix)* |
    /// | `$PLUGINSDIR` | `_plugins` |
    /// | `$TEMP` | `_temp` |
    /// | `$EXEDIR` | `_exedir` |
    /// | Other `$VAR` | `_VAR` |
    /// | Shell folder | `_DESKTOP`, `_SMPROGRAMS`, … |
    /// | Unknown shell folder | `_shell_<primary>_<fallback>` |
    /// | Language string | `_lang_<id>` |
    ///
    /// The result is then made safe to join onto an output directory:
    /// separators are normalised to `/`, repeated separators collapse, `.`
    /// components are dropped, `..` components become `_`, and any leading
    /// drive letter, UNC prefix or root slash is removed. A string that
    /// renders to nothing yields an empty path.
    ///
    /// For the path as the installer itself would write it, use
    /// [`to_install_path`](Self::to_install_path).
    pub fn to_path(&self) -> String {
        let mut out = String::with_capacity(self.render_hint());
        self.write_path(&mut out, PathStyle::Extraction);
        out
    }

    /// Renders this string as the installer's own destination path.
    ///
    /// Variable and shell-folder references are kept verbatim (`$INSTDIR`,
    /// `$PLUGINSDIR`, `$DESKTOP`), separators stay as backslashes, and nothing
    /// is sanitised — this is a faithful reproduction of the path the
    /// installer would write to, matching what 7-Zip lists for the same
    /// archive.
    ///
    /// A leading `$INSTDIR\` is removed, so paths under the install directory
    /// come out relative to it, and an empty result becomes `file`. Both match
    /// 7-Zip's `GetReducedName`.
    ///
    /// Use [`to_path`](Self::to_path) when the result will be joined onto an
    /// output directory: this one can produce absolute paths and `..`
    /// components if the installer contains them.
    pub fn to_install_path(&self) -> String {
        let mut out = String::with_capacity(self.render_hint());
        self.write_path(&mut out, PathStyle::Installer);
        out
    }

    /// Renders this string into `out` in the given style.
    ///
    /// The allocation-free form of [`to_path`](Self::to_path) and
    /// [`to_install_path`](Self::to_install_path), for callers rendering many
    /// paths into a reused buffer. Existing contents of `out` are kept.
    pub fn write_path(&self, out: &mut String, style: PathStyle) {
        let start = out.len();
        for segment in &self.segments {
            match segment {
                StringSegment::Literal(text) => out.push_str(text),
                StringSegment::Variable { index, name } => write_variable(out, *index, name, style),
                StringSegment::ShellFolder {
                    primary,
                    fallback,
                    target,
                } => write_shell_folder(out, *primary, *fallback, target, style),
                StringSegment::LangString(id) => match style {
                    // 7-Zip prints the language-string reference unresolved.
                    PathStyle::Installer => {
                        out.push_str("$(LSTR_");
                        push_u16(out, *id);
                        out.push(')');
                    }
                    // Dropping it could collide two different files onto one
                    // path, so keep an identifying stand-in.
                    PathStyle::Extraction => {
                        out.push_str("_lang_");
                        push_u16(out, *id);
                    }
                },
            }
        }

        // `start` is a char boundary and everything appended above is valid
        // UTF-8, so this split is safe.
        let rendered: String = out.split_off(start);
        match style {
            PathStyle::Installer => out.push_str(reduce_install_path(&rendered)),
            PathStyle::Extraction => sanitize_extraction_path(&rendered, out),
        }
    }

    /// Estimates the rendered length, to size the output buffer once.
    fn render_hint(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| match segment {
                StringSegment::Literal(text) => text.len(),
                // Long enough for the common `$INSTDIR` / `_plugins` forms.
                _ => 12,
            })
            .sum()
    }
}

/// How [`NsisString::write_path`] should render a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    /// The installer's own destination path, as 7-Zip lists it: references
    /// kept verbatim, backslash separators, a leading `$INSTDIR\` removed, and
    /// no sanitising. See [`NsisString::to_install_path`].
    Installer,
    /// A relative path safe to join onto an output directory. See
    /// [`NsisString::to_path`].
    Extraction,
}

/// Appends a variable reference in the given style.
fn write_variable(out: &mut String, index: u16, name: &str, style: PathStyle) {
    match style {
        PathStyle::Installer => out.push_str(name),
        PathStyle::Extraction => match index {
            // The install and output directories *are* the extraction root.
            VAR_INSTDIR | VAR_OUTDIR => {}
            VAR_TEMP => out.push_str("_temp"),
            VAR_PLUGINSDIR => out.push_str("_plugins"),
            VAR_EXEDIR => out.push_str("_exedir"),
            _ => {
                out.push('_');
                out.push_str(name.strip_prefix('$').unwrap_or(name));
            }
        },
    }
}

/// Removes 7-Zip's `$INSTDIR\` prefix and substitutes a name for empty paths.
fn reduce_install_path(rendered: &str) -> &str {
    const INSTDIR_PREFIX: &str = "$INSTDIR\\";

    let reduced = if rendered.len() >= INSTDIR_PREFIX.len()
        && rendered
            .get(..INSTDIR_PREFIX.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(INSTDIR_PREFIX))
    {
        rendered
            .get(INSTDIR_PREFIX.len()..)
            .unwrap_or(rendered)
            .trim_start_matches('\\')
    } else {
        rendered
    };

    if reduced.is_empty() { "file" } else { reduced }
}

/// Appends `rendered` to `out` as a relative, traversal-free path.
///
/// Splits on both separators so a path that mixes them is handled, drops empty
/// and `.` components, rewrites `..`, and skips a leading drive letter — on
/// Windows, joining an absolute path onto an output directory discards the
/// output directory entirely, which would let an installer write anywhere.
fn sanitize_extraction_path(rendered: &str, out: &mut String) {
    let mut first = true;
    for (position, component) in rendered.split(['\\', '/']).enumerate() {
        if component.is_empty() || component == "." {
            continue;
        }
        // A leading `C:` (or `C:\` after splitting) is a drive-relative root.
        if position == 0 && is_drive_letter(component) {
            continue;
        }
        if !first {
            out.push('/');
        }
        first = false;
        if component == ".." {
            out.push('_');
        } else {
            out.push_str(component);
        }
    }
}

/// Returns `true` if `text` begins with a drive specifier such as `C:`.
///
/// A path that starts this way is drive-relative rather than relative to
/// whatever directory it is joined onto.
pub fn starts_with_drive_letter(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some(letter), Some(':'))
        if letter.is_ascii_alphabetic())
}

/// Returns `true` for a path component that is exactly a drive specifier.
fn is_drive_letter(component: &str) -> bool {
    component.len() == 2 && starts_with_drive_letter(component)
}

/// Appends a `u16` without going through the formatting machinery.
fn push_u16(out: &mut String, value: u16) {
    let mut buffer = [0u8; 5];
    let mut length = 0;
    let mut remaining = value;
    loop {
        let digit = (remaining % 10) as u8;
        if let Some(slot) = buffer.get_mut(length) {
            *slot = b'0'.saturating_add(digit);
        }
        length = length.saturating_add(1);
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    for index in (0..length).rev() {
        if let Some(&digit) = buffer.get(index) {
            out.push(char::from(digit));
        }
    }
}

/// `$INSTDIR`, the install directory chosen at run time.
pub const VAR_INSTDIR: u16 = 21;
/// `$OUTDIR`, the current output directory.
pub const VAR_OUTDIR: u16 = 22;
/// `$EXEDIR`, the directory holding the installer.
pub const VAR_EXEDIR: u16 = 23;
/// `$TEMP`, the system temporary directory.
pub const VAR_TEMP: u16 = 25;
/// `$PLUGINSDIR`, the temporary directory plugins are unpacked into.
pub const VAR_PLUGINSDIR: u16 = 26;

impl fmt::Display for NsisString {
    /// Renders the string as it appears in a decompiled script: literal text
    /// with every reference spelled out, e.g. `$INSTDIR\docs\readme.txt`.
    ///
    /// Language strings render as `$(LSTR_n)` and unresolvable shell folders as
    /// `$_ERROR_UNSUPPORTED_SHELL_[primary,fallback]`, matching 7-Zip.
    ///
    /// This is the unreduced form: unlike
    /// [`to_install_path`](NsisString::to_install_path) it keeps a leading
    /// `$INSTDIR\`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.segments {
            match segment {
                StringSegment::Literal(text) => f.write_str(text)?,
                StringSegment::Variable { name, .. } => f.write_str(name)?,
                StringSegment::ShellFolder {
                    primary,
                    fallback,
                    target,
                } => {
                    let mut rendered = String::new();
                    write_shell_folder(
                        &mut rendered,
                        *primary,
                        *fallback,
                        target,
                        PathStyle::Installer,
                    );
                    f.write_str(&rendered)?;
                }
                StringSegment::LangString(id) => write!(f, "$(LSTR_{id})")?,
            }
        }
        Ok(())
    }
}

/// Reads a string from a table at the given **byte** offset.
///
/// Dispatches to the encoding-specific reader. Prefer
/// [`StringTable::read`], which takes the TCHAR offsets that NSIS structures
/// actually store.
///
/// # Errors
///
/// Returns [`crate::error::Error::InvalidStringOffset`] if the offset is beyond
/// the string table.
pub fn read_nsis_string(context: &StringTable<'_>, offset: usize) -> Result<NsisString, Error> {
    if offset >= context.bytes().len() {
        return Err(Error::InvalidStringOffset {
            offset: offset as u32,
        });
    }

    match context.encoding() {
        StringEncoding::Ansi => ansi::read_ansi_string(context, offset),
        StringEncoding::Unicode => unicode::read_unicode_string(context, offset),
        StringEncoding::Park => park::read_park_string(context, offset),
    }
}

/// An installer's string table, and everything needed to decode from it.
///
/// Decoding a string is not a pure function of its bytes. The character size
/// and the special-code range depend on the encoding and the NSIS version; the
/// names of variables depend on which variable layout the compiler used; and a
/// registry-backed shell folder is a *reference to another string in the same
/// table*, so the decoder has to be able to read the table it is decoding from.
/// This type carries all of it.
///
/// Obtain one from
/// [`NsisInstaller::string_table`](crate::installer::NsisInstaller::string_table),
/// which builds it from the detected encoding, version and variable layout.
#[derive(Debug, Clone, Copy)]
pub struct StringTable<'a> {
    /// The decompressed header block.
    data: &'a [u8],
    /// Byte offset of the string table within `data`.
    base: usize,
    encoding: StringEncoding,
    ansi_codes: AnsiCodeRange,
    /// Number of built-in variables in this installer's layout.
    internal_vars: u16,
}

impl<'a> StringTable<'a> {
    /// Creates a string table view over a decompressed header block.
    ///
    /// `base` is the byte offset of the string block within `data`, and
    /// `internal_vars` the number of built-in variables in the installer's
    /// layout — [`Nsis2SubVersion::internal_var_count`] for an NSIS 2
    /// installer, [`DEFAULT_INTERNAL_VARS`] otherwise.
    ///
    /// [`Nsis2SubVersion::internal_var_count`]: crate::opcode::Nsis2SubVersion::internal_var_count
    pub fn new(
        data: &'a [u8],
        base: usize,
        encoding: StringEncoding,
        ansi_codes: AnsiCodeRange,
        internal_vars: u16,
    ) -> Self {
        Self {
            data,
            base,
            encoding,
            ansi_codes,
            internal_vars,
        }
    }

    /// Returns the encoding this table is stored in.
    #[inline]
    pub fn encoding(&self) -> StringEncoding {
        self.encoding
    }

    /// Returns the number of built-in variables in this installer's layout.
    #[inline]
    pub fn internal_vars(&self) -> u16 {
        self.internal_vars
    }

    /// Returns the header block these strings live in.
    #[inline]
    pub(crate) fn bytes(&self) -> &'a [u8] {
        self.data
    }

    /// Returns the special-code range for an ANSI table.
    #[inline]
    pub(crate) fn ansi_code_range(&self) -> AnsiCodeRange {
        self.ansi_codes
    }

    /// Returns the number of bytes per character in this table.
    #[inline]
    pub fn char_size(&self) -> usize {
        // Both Unicode and Park are UTF-16LE.
        match self.encoding {
            StringEncoding::Unicode | StringEncoding::Park => 2,
            StringEncoding::Ansi => 1,
        }
    }

    /// Reads the string at a TCHAR offset.
    ///
    /// String references inside NSIS structures — section `name_ptr` fields,
    /// entry parameter slots — are character indices rather than byte offsets,
    /// so the offset is scaled by [`char_size`](Self::char_size). A negative
    /// offset is not a reference at all and yields an empty string.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidStringOffset`] if the offset lies beyond the
    /// table.
    pub fn read(&self, offset: i32) -> Result<NsisString, Error> {
        if offset < 0 {
            return Ok(NsisString {
                segments: Vec::new(),
            });
        }
        let byte_offset = self
            .base
            .saturating_add((offset as usize).saturating_mul(self.char_size()));
        read_nsis_string(self, byte_offset)
    }

    /// Resolves a variable index to the name this installer's layout gives it.
    ///
    /// Indices below the layout's built-in count name a built-in variable;
    /// the rest are user-defined and render as `$_N_`, numbered from that
    /// count. NSIS 2.25 and earlier also shift the indices at and above
    /// `$EXEPATH`, since two variables were added there in 2.26.
    ///
    /// # Source
    ///
    /// 7-Zip `NsisIn.cpp` `GetVar2` and `GET_NUM_INTERNAL_VARS`.
    pub fn variable_name(&self, index: u16) -> Cow<'static, str> {
        // 2.03 and 2.25 layouts stop short of the modern table; anything at or
        // above their count is user-defined.
        if index >= self.internal_vars {
            return Cow::Owned(format!("$_{}_", index.saturating_sub(self.internal_vars)));
        }
        // The 2.04-2.25 layout lacks $EXEPATH and $EXEFILE, so the two
        // variables that follow them sit two slots lower than in the table.
        let table_index = if self.internal_vars == NSIS_2_25_INTERNAL_VARS && index >= VAR_EXEPATH {
            index.saturating_add(2)
        } else {
            index
        };
        VARIABLE_NAMES.get(table_index as usize).map_or_else(
            || Cow::Owned(format!("${index}")),
            |name| Cow::Borrowed(*name),
        )
    }

    /// Resolves a shell-folder id pair.
    ///
    /// A primary id with bit 7 set is not a CSIDL constant but a registry
    /// lookup: its low six bits are the offset of the value name within this
    /// table, and bit 6 selects the 64-bit registry view.
    pub fn shell_target(&self, primary: u8, fallback: u8) -> ShellTarget {
        if primary & 0x80 != 0 {
            let wow64 = primary & 0x40 != 0;
            // Read the value name as plain text rather than recursively
            // decoding it. A registry value name is ASCII in practice, and a
            // crafted file could otherwise point two shell references at each
            // other and recurse until the stack gives out. 7-Zip notes the same
            // hazard and declines to follow the reference either.
            let name = self.read_literal(i32::from(primary & 0x3F));
            return match name.as_str() {
                "ProgramFilesDir" => ShellTarget::ProgramFiles { wow64 },
                "CommonFilesDir" => ShellTarget::CommonFiles { wow64 },
                _ => ShellTarget::RegistryValue { name, wow64 },
            };
        }

        // The fallback applies when the primary names nothing known.
        for id in [primary, fallback] {
            if let Some(Some(name)) = SHELL_FOLDER_NAMES.get(id as usize) {
                return ShellTarget::Csidl(name);
            }
        }
        ShellTarget::Unresolved
    }

    /// Reads the plain text of the string at a TCHAR offset, stopping at the
    /// first special code.
    ///
    /// Used where a string is known to be plain data — a registry value name —
    /// and following its references would be neither meaningful nor safe.
    fn read_literal(&self, offset: i32) -> String {
        if offset < 0 {
            return String::new();
        }
        let start = self
            .base
            .saturating_add((offset as usize).saturating_mul(self.char_size()));
        let Some(rest) = self.data.get(start..) else {
            return String::new();
        };

        let mut text = String::new();
        match self.encoding {
            StringEncoding::Ansi => {
                for &byte in rest {
                    if byte == 0 || ansi::is_special_code(byte, self.ansi_codes) {
                        break;
                    }
                    text.push(char::from(byte));
                }
            }
            StringEncoding::Unicode | StringEncoding::Park => {
                for pair in rest.chunks_exact(2) {
                    let unit = u16::from_le_bytes([
                        pair.first().copied().unwrap_or(0),
                        pair.get(1).copied().unwrap_or(0),
                    ]);
                    if unit == 0 || unit <= 0x0004 || (0xE000..=0xE003).contains(&unit) {
                        break;
                    }
                    text.extend(char::from_u32(u32::from(unit)));
                }
            }
        }
        text
    }
}

/// Helpers shared by the string decoders' tests.
#[cfg(test)]
pub(crate) mod testing {
    use super::{
        Cow, DEFAULT_INTERNAL_VARS, ShellTarget, StringEncoding, StringSegment, StringTable,
        ansi::AnsiCodeRange,
    };

    /// The name a modern-layout installer gives a variable index.
    pub(crate) fn variable_name_for(index: u16) -> Cow<'static, str> {
        StringTable::new(
            &[],
            0,
            StringEncoding::Unicode,
            AnsiCodeRange::Nsis3,
            DEFAULT_INTERNAL_VARS,
        )
        .variable_name(index)
    }

    /// The segment a shell reference packed as `raw` decodes to, for tables
    /// with no registry-backed folders.
    pub(crate) fn shell_segment(raw: u16) -> StringSegment {
        let (primary, fallback) = ((raw & 0xFF) as u8, (raw >> 8) as u8);
        let target = super::csidl_name(primary)
            .or_else(|| super::csidl_name(fallback))
            .map_or(ShellTarget::Unresolved, ShellTarget::Csidl);
        StringSegment::ShellFolder {
            primary,
            fallback,
            target,
        }
    }
}

/// Number of built-in variables in the NSIS 2.04-2.25 layout.
const NSIS_2_25_INTERNAL_VARS: u16 = 30;

/// Index of `$EXEPATH`, the first variable added in NSIS 2.26.
const VAR_EXEPATH: u16 = 27;

/// Number of built-in variables in NSIS 2.26 and later, which NSIS 3 inherited.
///
/// Earlier NSIS 2 layouts have fewer; see
/// [`Nsis2SubVersion::internal_var_count`](crate::opcode::Nsis2SubVersion::internal_var_count).
pub const DEFAULT_INTERNAL_VARS: u16 = 32;

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

/// Returns the variable name for an index in the modern (NSIS 2.26+) layout.
///
/// Borrows for the 32 built-in variables and allocates only for user-defined
/// ones, shown as `$_N_`.
///
/// Prefer [`StringTable::variable_name`] where an installer is at hand: NSIS 2
/// releases before 2.26 have fewer built-ins, so this function names their
/// variables wrongly. It remains for callers holding a bare index with no
/// installer context.
///
/// Source: 7-Zip `NsisIn.cpp` `GetVar2`, `state.h`.
pub fn variable_name(index: u16) -> Cow<'static, str> {
    if let Some(name) = VARIABLE_NAMES.get(index as usize) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!(
            "$_{}_",
            index.saturating_sub(DEFAULT_INTERNAL_VARS)
        ))
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

/// Renders a resolved shell folder the way a decompiled script would show it.
///
/// Registry-backed folders need the string table to resolve, so this takes an
/// already-resolved [`ShellTarget`] — see [`StringTable::shell_target`]. The
/// raw ids are only used for the unresolved form.
///
/// Source: 7-Zip `NsisIn.cpp` `GetShellString`.
pub fn shell_folder_name(primary: u8, fallback: u8, target: &ShellTarget) -> String {
    let mut out = String::new();
    write_shell_folder(&mut out, primary, fallback, target, PathStyle::Installer);
    out
}

/// Looks up the name of a shell folder id, if it names a known one.
///
/// Returns the name without its leading `$`, e.g. `APPDATA`.
pub fn csidl_name(id: u8) -> Option<&'static str> {
    SHELL_FOLDER_NAMES.get(id as usize).copied().flatten()
}

/// Appends a shell-folder reference to `out` in the given style.
///
/// Splits the raw value the way NSIS encodes it — primary CSIDL in the low
/// byte, fallback in the high byte — and resolves the first of the two that
/// names a known folder. Writing straight into the caller's buffer keeps path
/// rendering allocation-free.
fn write_shell_folder(
    out: &mut String,
    primary: u8,
    fallback: u8,
    target: &ShellTarget,
    style: PathStyle,
) {
    let prefix = match style {
        PathStyle::Installer => '$',
        PathStyle::Extraction => '_',
    };

    match target {
        ShellTarget::Csidl(name) => {
            out.push(prefix);
            out.push_str(name);
        }
        ShellTarget::ProgramFiles { wow64 } => {
            out.push(prefix);
            out.push_str("PROGRAMFILES");
            if *wow64 {
                out.push_str("64");
            }
        }
        ShellTarget::CommonFiles { wow64 } => {
            out.push(prefix);
            out.push_str("COMMONFILES");
            if *wow64 {
                out.push_str("64");
            }
        }
        ShellTarget::RegistryValue { name, wow64 } => match style {
            // 7-Zip's wording for a registry value it does not recognise.
            PathStyle::Installer => {
                out.push_str("$_ERROR_UNSUPPORTED_VALUE_REGISTRY_(");
                out.push_str(name);
                out.push(')');
            }
            // Keep the extraction form free of parentheses.
            PathStyle::Extraction => {
                out.push_str("_reg");
                out.push_str(if *wow64 { "64_" } else { "_" });
                out.push_str(name);
            }
        },
        ShellTarget::Unresolved => match style {
            PathStyle::Installer => {
                out.push_str("$_ERROR_UNSUPPORTED_SHELL_[");
                push_u16(out, u16::from(primary));
                out.push(',');
                push_u16(out, u16::from(fallback));
                out.push(']');
            }
            // Brackets and commas are legal in file names but noisy; keep the
            // extraction form plain and unambiguous.
            PathStyle::Extraction => {
                out.push_str("_shell_");
                push_u16(out, u16::from(primary));
                out.push('_');
                push_u16(out, u16::from(fallback));
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::testing::{shell_segment, variable_name_for};

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
                StringSegment::Variable {
                    index: 21,
                    name: variable_name_for(21),
                },
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
                shell_segment(0x001A),
            ],
        };
        // 7-Zip prints an unresolved language string as `$(LSTR_n)`.
        assert_eq!(s.to_string(), "$(LSTR_5) in $APPDATA");
    }

    #[test]
    fn read_string_out_of_bounds() {
        let table = b"hello\0";
        let context = StringTable::new(
            table,
            0,
            StringEncoding::Ansi,
            AnsiCodeRange::Nsis2,
            DEFAULT_INTERNAL_VARS,
        );
        let result = read_nsis_string(&context, 100);
        assert!(result.is_err());
    }

    #[test]
    fn to_path_instdir() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable {
                    index: 21,
                    name: variable_name_for(21),
                }, // $INSTDIR
                StringSegment::Literal("\\program.exe".into()),
            ],
        };
        assert_eq!(s.to_path(), "program.exe");
    }

    #[test]
    fn to_path_pluginsdir() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable {
                    index: 26,
                    name: variable_name_for(26),
                }, // $PLUGINSDIR
                StringSegment::Literal("\\System.dll".into()),
            ],
        };
        assert_eq!(s.to_path(), "_plugins/System.dll");
    }

    #[test]
    fn to_path_temp() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable {
                    index: 25,
                    name: variable_name_for(25),
                }, // $TEMP
                StringSegment::Literal("\\payload.bin".into()),
            ],
        };
        assert_eq!(s.to_path(), "_temp/payload.bin");
    }

    #[test]
    fn to_path_nested() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable {
                    index: 21,
                    name: variable_name_for(21),
                }, // $INSTDIR
                StringSegment::Literal("\\Lang\\en_US.ini".into()),
            ],
        };
        assert_eq!(s.to_path(), "Lang/en_US.ini");
    }

    #[test]
    fn to_path_shell_folder() {
        let s = NsisString {
            segments: vec![
                shell_segment(0x1A),
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

    /// Builds a string from one literal, the common shape in these tests.
    fn literal(text: &str) -> NsisString {
        NsisString {
            segments: vec![StringSegment::Literal(text.into())],
        }
    }

    #[test]
    fn to_path_collapses_repeated_separators() {
        // Regression: a `replace("//", "/")` pass only collapses non-overlapping
        // matches, so `a///b` came out as `a//b`.
        assert_eq!(literal("a\\\\\\b").to_path(), "a/b");
        assert_eq!(literal("a///b").to_path(), "a/b");
        assert_eq!(literal("a\\/b").to_path(), "a/b");
    }

    #[test]
    fn to_path_rewrites_traversal_components_only() {
        // Regression: replacing the substring `..` mangled ordinary names.
        assert_eq!(literal("file..txt").to_path(), "file..txt");
        assert_eq!(literal("..").to_path(), "_");
        assert_eq!(literal("a\\..\\b").to_path(), "a/_/b");
        assert_eq!(literal("....").to_path(), "....");
    }

    #[test]
    fn to_path_drops_current_directory_components() {
        assert_eq!(literal("a\\.\\b").to_path(), "a/b");
        assert_eq!(literal(".\\b").to_path(), "b");
    }

    #[test]
    fn to_path_strips_absolute_prefixes() {
        // Joining an absolute path onto an output directory discards that
        // directory on Windows, which would let an installer write anywhere.
        assert_eq!(literal("C:\\evil.exe").to_path(), "evil.exe");
        assert_eq!(literal("c:\\a\\b.txt").to_path(), "a/b.txt");
        assert_eq!(literal("\\\\server\\share\\x").to_path(), "server/share/x");
        assert_eq!(literal("\\rooted.txt").to_path(), "rooted.txt");
        // A colon later in the path is not a drive specifier.
        assert_eq!(literal("dir\\c:\\x").to_path(), "dir/c:/x");
    }

    #[test]
    fn to_path_keeps_language_strings_distinguishable() {
        // Dropping the reference outright would collide two different files.
        let s = NsisString {
            segments: vec![
                StringSegment::LangString(7),
                StringSegment::Literal(".txt".into()),
            ],
        };
        assert_eq!(s.to_path(), "_lang_7.txt");
    }

    #[test]
    fn to_path_names_unmappable_shell_folders() {
        let s = NsisString {
            segments: vec![shell_segment(0x3C3C)],
        };
        assert_eq!(s.to_path(), "_shell_60_60");
    }

    #[test]
    fn to_install_path_reduces_against_instdir() {
        let s = NsisString {
            segments: vec![
                StringSegment::Variable {
                    index: 21,
                    name: variable_name_for(21),
                }, // $INSTDIR
                StringSegment::Literal("\\docs\\payload.txt".into()),
            ],
        };
        // 7-Zip lists this as `docs\payload.txt`.
        assert_eq!(s.to_install_path(), "docs\\payload.txt");
        // ...and the extraction form is the same path, made portable.
        assert_eq!(s.to_path(), "docs/payload.txt");
    }

    #[test]
    fn to_install_path_keeps_other_roots_verbatim() {
        // 7-Zip reduces only `$INSTDIR`; everything else keeps its own root.
        let s = NsisString {
            segments: vec![
                StringSegment::Variable {
                    index: 26,
                    name: variable_name_for(26),
                }, // $PLUGINSDIR
                StringSegment::Literal("\\app-64.7z".into()),
            ],
        };
        assert_eq!(s.to_install_path(), "$PLUGINSDIR\\app-64.7z");
    }

    #[test]
    fn to_install_path_substitutes_a_name_for_empty_paths() {
        assert_eq!(NsisString { segments: vec![] }.to_install_path(), "file");
    }

    #[test]
    fn to_install_path_reduces_only_the_full_prefix() {
        // 7-Zip strips `$INSTDIR\\`, backslash included, so a bare `$INSTDIR`
        // is left as it is rather than becoming an empty path.
        let s = NsisString {
            segments: vec![StringSegment::Variable {
                index: 21,
                name: variable_name_for(21),
            }],
        };
        assert_eq!(s.to_install_path(), "$INSTDIR");

        // The match ignores case, as 7-Zip's does.
        assert_eq!(literal("$instdir\\x.txt").to_install_path(), "x.txt");
    }

    #[test]
    fn to_install_path_does_not_sanitize() {
        // Traversal and absolute paths are reported as the installer wrote
        // them; sanitising belongs to `to_path`.
        assert_eq!(literal("C:\\evil.exe").to_install_path(), "C:\\evil.exe");
        assert_eq!(literal("a\\..\\b").to_install_path(), "a\\..\\b");
    }

    #[test]
    fn to_path_never_escapes_the_output_directory() {
        // The property `to_path` exists to guarantee. Checked on the rendered
        // string rather than through `Path`, because a Windows drive path looks
        // relative to a Unix `Path` and the check would pass for free on CI.
        for input in [
            "C:\\evil.exe",
            "c:/evil.exe",
            "\\\\server\\share\\payload",
            "\\rooted.txt",
            "/rooted.txt",
            "..\\..\\Windows\\System32\\drivers\\etc\\hosts",
            "../../etc/passwd",
            "a\\..\\..\\..\\b",
            "",
        ] {
            let path = literal(input).to_path();
            assert!(
                !path.starts_with('/') && !path.starts_with('\\'),
                "{input:?} rendered to a rooted path: {path:?}"
            );
            let first = path.split('/').next().unwrap_or_default();
            assert!(
                !is_drive_letter(first),
                "{input:?} kept its drive specifier: {path:?}"
            );
            assert!(
                !path.split('/').any(|component| component == ".."),
                "{input:?} kept a traversal component: {path:?}"
            );
        }
    }

    #[test]
    fn write_path_appends_to_the_buffer() {
        let mut buffer = String::from("out/");
        literal("a\\b").write_path(&mut buffer, PathStyle::Extraction);
        assert_eq!(buffer, "out/a/b");
    }

    #[test]
    fn shell_folder_name_matches_7zip_wording() {
        assert_eq!(
            shell_folder_name(0x1A, 0, &ShellTarget::Csidl("APPDATA")),
            "$APPDATA"
        );
        // Neither id names a known folder.
        assert_eq!(
            shell_folder_name(60, 60, &ShellTarget::Unresolved),
            "$_ERROR_UNSUPPORTED_SHELL_[60,60]"
        );
        // Registry-backed folders, with and without the 64-bit view.
        assert_eq!(
            shell_folder_name(0x80, 0, &ShellTarget::ProgramFiles { wow64: false }),
            "$PROGRAMFILES"
        );
        assert_eq!(
            shell_folder_name(0xC0, 0, &ShellTarget::CommonFiles { wow64: true }),
            "$COMMONFILES64"
        );
        // A registry value 7-Zip does not recognise.
        assert_eq!(
            shell_folder_name(
                0x81,
                0,
                &ShellTarget::RegistryValue {
                    name: "MediaPathUnexpanded".into(),
                    wow64: false,
                }
            ),
            "$_ERROR_UNSUPPORTED_VALUE_REGISTRY_(MediaPathUnexpanded)"
        );
    }

    #[test]
    fn shell_target_falls_back_to_the_second_id() {
        let table = StringTable::new(
            &[],
            0,
            StringEncoding::Unicode,
            AnsiCodeRange::Nsis3,
            DEFAULT_INTERNAL_VARS,
        );
        // Id 15 is unassigned, so the fallback decides: 0x1A is APPDATA.
        assert_eq!(table.shell_target(15, 0x1A), ShellTarget::Csidl("APPDATA"));
        // Neither is assigned.
        assert_eq!(table.shell_target(15, 15), ShellTarget::Unresolved);
    }
}
