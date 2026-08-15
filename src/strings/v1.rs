//! The NSIS 1.x string encoding.
//!
//! Every later version encodes a variable reference as a code byte followed by
//! an encoded index, leaving room for hundreds of variables and for the shell
//! folder and language-string references that 2.x added. NSIS 1.x has none of
//! that: a byte at or above [`VAR_CODES_START`] *is* the variable, one byte per
//! reference, and `0xFF` escapes the byte after it so a string can still
//! contain a literal in that range.
//!
//! That makes the encodings mutually unreadable rather than merely different.
//! A 1.x `$INSTDIR` is the single byte `0xF3`, which the NSIS 2 decoder reads
//! as ordinary text; a 2.x `$INSTDIR` is `0xFE` plus two index bytes, which
//! this decoder would read as an escape followed by a stray character.
//!
//! # Source
//!
//! `process_string` in `Source/exehead/util.c` of the NSIS 1.98 source, and
//! `VAR_CODES_START` in its `Source/exehead/fileform.h`.

use std::borrow::Cow;

use crate::{
    error::Error,
    strings::{NsisString, StringSegment, StringTable},
};

/// The first byte value that names a variable rather than being text.
///
/// `(256 - 35)` in the 1.98 headers: 34 variables plus `0xFF` for the escape.
pub const VAR_CODES_START: u8 = 221;

/// The byte that escapes the one after it, so text can contain a code byte.
pub const VAR_CODE_ESCAPE: u8 = 255;

/// The variables NSIS 1.x defines, in the order `process_string` switches on.
///
/// The index is the code byte minus [`VAR_CODES_START`]. NSIS 2.x renumbered
/// this list and 3.x extended it again, so these indices mean something else in
/// every later version.
pub static V1_VARIABLES: [&str; 34] = [
    "$HWNDPARENT",
    "$0",
    "$1",
    "$2",
    "$3",
    "$4",
    "$5",
    "$6",
    "$7",
    "$8",
    "$9",
    "$R0",
    "$R1",
    "$R2",
    "$R3",
    "$R4",
    "$R5",
    "$R6",
    "$R7",
    "$R8",
    "$R9",
    "$CMDLINE",
    "$INSTDIR",
    "$OUTDIR",
    "$EXEDIR",
    "$PROGRAMFILES",
    "$SMPROGRAMS",
    "$SMSTARTUP",
    "$DESKTOP",
    "$STARTMENU",
    "$QUICKLAUNCH",
    "$TEMP",
    "$WINDIR",
    "$SYSDIR",
];

/// Index of `$INSTDIR` in the 1.x variable list.
pub const V1_VAR_INSTDIR: u16 = 22;
/// Index of `$OUTDIR` in the 1.x variable list.
pub const V1_VAR_OUTDIR: u16 = 23;
/// Index of `$EXEDIR` in the 1.x variable list.
pub const V1_VAR_EXEDIR: u16 = 24;
/// Index of `$TEMP` in the 1.x variable list.
pub const V1_VAR_TEMP: u16 = 31;

/// Returns the name NSIS 1.x gives a variable index.
///
/// An index past the list is rendered as `$_N_`, matching how this crate names
/// user-defined variables elsewhere — though 1.x has none, so it only happens
/// for a byte this crate does not recognise.
pub fn variable_name_v1(index: u16) -> Cow<'static, str> {
    match V1_VARIABLES.get(index as usize) {
        Some(name) => Cow::Borrowed(name),
        None => Cow::Owned(format!(
            "$_{}_",
            index.saturating_sub(V1_VARIABLES.len() as u16)
        )),
    }
}

/// Reads an NSIS 1.x string from the string table.
///
/// The string starts at `offset` and runs to the next null byte.
///
/// # Errors
///
/// Returns [`Error::InvalidStringOffset`] if `offset` is past the table.
pub fn read_v1_string(context: &StringTable<'_>, offset: usize) -> Result<NsisString, Error> {
    let table = context.bytes();
    if offset >= table.len() {
        return Err(Error::InvalidStringOffset {
            offset: offset as u32,
        });
    }

    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut pos = offset;

    while let Some(&b) = table.get(pos) {
        if b == 0 {
            break;
        }
        pos = pos.saturating_add(1);

        if b < VAR_CODES_START {
            // 1.x is ANSI only, and every byte below the code range is one
            // character of the host code page.
            literal.push(b as char);
            continue;
        }

        if b == VAR_CODE_ESCAPE {
            // The escaped byte is text whatever its value.
            if let Some(&escaped) = table.get(pos) {
                literal.push(escaped as char);
                pos = pos.saturating_add(1);
            }
            continue;
        }

        if !literal.is_empty() {
            segments.push(StringSegment::Literal(core::mem::take(&mut literal)));
        }
        let index = u16::from(b.saturating_sub(VAR_CODES_START));
        segments.push(StringSegment::Variable {
            index,
            name: variable_name_v1(index),
        });
    }

    if !literal.is_empty() {
        segments.push(StringSegment::Literal(literal));
    }
    Ok(NsisString { segments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::testing::v1_table;

    /// Reads the string starting at the front of `bytes`.
    fn read(bytes: &[u8]) -> NsisString {
        let table = v1_table(bytes);
        read_v1_string(&table, 0).expect("a string at offset 0")
    }

    #[test]
    fn a_code_byte_is_the_whole_reference() {
        // 0xF6 is $PROGRAMFILES: one byte, no index following it.
        let s = read(b"\xF6\\Nsis1xTest\x00");
        assert_eq!(s.to_string(), "$PROGRAMFILES\\Nsis1xTest");
        assert_eq!(
            s.segments.first(),
            Some(&StringSegment::Variable {
                index: 25,
                name: Cow::Borrowed("$PROGRAMFILES"),
            })
        );
    }

    #[test]
    fn instdir_is_a_single_byte() {
        // The whole of tests/fixtures/nsis1x.exe's SetOutPath operand.
        assert_eq!(read(b"\xF3\x00").to_string(), "$INSTDIR");
        assert_eq!(V1_VARIABLES[V1_VAR_INSTDIR as usize], "$INSTDIR");
    }

    #[test]
    fn every_variable_has_its_own_code() {
        for (index, name) in V1_VARIABLES.iter().enumerate() {
            let byte = VAR_CODES_START.saturating_add(index as u8);
            assert_eq!(read(&[byte, 0]).to_string(), *name, "code {byte:#04X}");
        }
        // The list stops one short of the escape byte.
        assert_eq!(
            VAR_CODES_START as usize + V1_VARIABLES.len(),
            VAR_CODE_ESCAPE as usize
        );
    }

    #[test]
    fn the_escape_byte_makes_the_next_one_text() {
        // Without the escape, 0xF3 would read as $INSTDIR.
        assert_eq!(read(b"a\xFF\xF3b\x00").to_string(), "a\u{F3}b");
    }

    #[test]
    fn a_dangling_escape_ends_the_string() {
        assert_eq!(read(b"ab\xFF").to_string(), "ab");
    }

    #[test]
    fn text_and_variables_alternate() {
        let s = read(b"go \xF3 now\x00");
        assert_eq!(s.to_string(), "go $INSTDIR now");
        assert_eq!(s.segments.len(), 3);
    }

    #[test]
    fn an_offset_past_the_table_is_an_error() {
        let table = v1_table(b"hi\x00");
        assert!(matches!(
            read_v1_string(&table, 99),
            Err(Error::InvalidStringOffset { .. })
        ));
    }
}
