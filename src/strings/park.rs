//! Park variant string decoding.
//!
//! The Park variant (Jim Park's Unicode fork of NSIS) stores strings as
//! UTF-16LE, like NSIS 3.x Unicode, but uses different special code values
//! in the Unicode Private Use Area:
//!
//! | Code | Value | Purpose |
//! |------|-------|---------|
//! | `PARK_CODE_SKIP` | `0xE000` | Next code unit is a literal character |
//! | `PARK_CODE_VAR` | `0xE001` | Variable reference |
//! | `PARK_CODE_SHELL` | `0xE002` | Shell folder constant |
//! | `PARK_CODE_LANG` | `0xE003` | Language string reference |
//!
//! After each special code (except SKIP), **one** UTF-16LE code unit follows
//! as the argument. For VAR and LANG, the value is `n & 0x7FFF`. For SHELL,
//! `low = n & 0xFF`, `high = n >> 8`.
//!
//! Characters below `0x80` are always literal ASCII. Characters `>= 0x80`
//! that are not in `0xE000..0xE003` are also literal (normal Unicode).
//!
//! Sources: 7-Zip `NsisIn.cpp` (lines 638-996), Binary Refinery `xtnsis.py`.

use crate::error::Error;
use crate::strings::{NsisString, StringSegment, StringTable};

/// Park special code: next code unit is a literal character.
const PARK_CODE_SKIP: u16 = 0xE000;
/// Park special code: variable reference.
const PARK_CODE_VAR: u16 = 0xE001;
/// Park special code: shell folder constant.
const PARK_CODE_SHELL: u16 = 0xE002;
/// Park special code: language string reference.
const PARK_CODE_LANG: u16 = 0xE003;

/// Returns `true` if this UTF-16LE code unit is a Park special code.
pub fn is_park_special(ch: u16) -> bool {
    (PARK_CODE_SKIP..=PARK_CODE_LANG).contains(&ch)
}

/// Reads a UTF-16LE code unit from the table at the given byte offset.
fn read_u16(table: &[u8], offset: usize) -> Option<u16> {
    table
        .get(offset..)
        .and_then(|s| s.first_chunk::<2>())
        .copied()
        .map(u16::from_le_bytes)
}

/// Reads a Park-encoded NSIS string from the string table.
///
/// Park strings are UTF-16LE. The byte `offset` must be 2-byte aligned
/// (or at least point to the start of a valid UTF-16LE code unit).
pub fn read_park_string(context: &StringTable<'_>, offset: usize) -> Result<NsisString, Error> {
    let table = context.bytes();
    let mut segments = Vec::new();
    let mut literal_chars: Vec<u16> = Vec::new();
    let mut pos = offset;

    while let Some(ch) = read_u16(table, pos) {
        if ch == 0 {
            break;
        }

        if is_park_special(ch) {
            // Read the argument code unit.
            let Some(arg_pos) = pos.checked_add(2) else {
                break;
            };
            let Some(n) = read_u16(table, arg_pos) else {
                break;
            };
            pos = pos.saturating_add(4); // skip the special code + argument

            if n == 0 {
                break;
            }

            if ch == PARK_CODE_SKIP {
                // The argument is a literal character.
                literal_chars.push(n);
                continue;
            }

            // Flush accumulated literal before emitting a special segment.
            if !literal_chars.is_empty() {
                let s = String::from_utf16_lossy(&literal_chars);
                segments.push(StringSegment::Literal(s));
                literal_chars.clear();
            }

            match ch {
                PARK_CODE_VAR => {
                    let index = n & 0x7FFF;
                    segments.push(StringSegment::Variable {
                        index,
                        name: context.variable_name(index),
                    });
                }
                PARK_CODE_SHELL => {
                    // Two folder ids, low byte first — the same packing NSIS 3
                    // Unicode uses. Keeping only the low byte would discard the
                    // fallback and mis-resolve folders whose primary id is not
                    // in the table. Source: 7-Zip NsisIn.cpp:1056.
                    let (primary, fallback) = ((n & 0xFF) as u8, (n >> 8) as u8);
                    segments.push(StringSegment::ShellFolder {
                        primary,
                        fallback,
                        target: context.shell_target(primary, fallback),
                    });
                }
                PARK_CODE_LANG => {
                    let index = n & 0x7FFF;
                    segments.push(StringSegment::LangString(index));
                }
                _ => {}
            }
        } else {
            literal_chars.push(ch);
            pos = pos.saturating_add(2);
        }
    }

    if !literal_chars.is_empty() {
        let s = String::from_utf16_lossy(&literal_chars);
        segments.push(StringSegment::Literal(s));
    }

    Ok(NsisString { segments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::{
        DEFAULT_INTERNAL_VARS, StringEncoding,
        ansi::AnsiCodeRange,
        testing::{shell_segment, variable_name_for},
    };

    /// Wraps raw table bytes in the context the decoder needs.
    fn context(table: &[u8]) -> StringTable<'_> {
        StringTable::new(
            table,
            0,
            StringEncoding::Park,
            AnsiCodeRange::Nsis3,
            DEFAULT_INTERNAL_VARS,
        )
    }

    /// Builds a UTF-16LE byte sequence from u16 code units.
    fn encode_u16s(units: &[u16]) -> Vec<u8> {
        let mut buf = Vec::new();
        for &u in units {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        buf
    }

    #[test]
    fn plain_ascii_string() {
        // "Hello" in UTF-16LE + null terminator
        let table = encode_u16s(&[0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x0000]);
        let s = read_park_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("Hello".into()));
    }

    #[test]
    fn string_with_variable() {
        // PARK_CODE_VAR + 21 ($INSTDIR) + "\App" + null
        let table = encode_u16s(&[
            PARK_CODE_VAR,
            21, // $INSTDIR
            0x5C,
            0x41,
            0x70,
            0x70, // \App
            0x0000,
        ]);
        let s = read_park_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(
            s.segments[0],
            StringSegment::Variable {
                index: 21,
                name: variable_name_for(21)
            }
        );
        assert_eq!(s.segments[1], StringSegment::Literal("\\App".into()));
    }

    #[test]
    fn string_with_shell_folder() {
        // PARK_CODE_SHELL + 0x001A (CSIDL_APPDATA in low byte) + null
        let table = encode_u16s(&[PARK_CODE_SHELL, 0x001A, 0x0000]);
        let s = read_park_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], shell_segment(0x1A));
    }

    #[test]
    fn skip_code() {
        // PARK_CODE_SKIP + 0xE001 (should be literal, not treated as VAR) + null
        let table = encode_u16s(&[PARK_CODE_SKIP, PARK_CODE_VAR, 0x0000]);
        let s = read_park_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        // The PARK_CODE_VAR value should appear as a literal character.
        let lit = &s.segments[0];
        assert!(matches!(lit, StringSegment::Literal(_)));
    }

    #[test]
    fn empty_string() {
        let table = encode_u16s(&[0x0000]);
        let s = read_park_string(&context(&table), 0).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn non_ascii_literal() {
        // Characters >= 0x80 that are NOT park specials should be literal.
        // e.g. 'ä' = 0x00E4
        let table = encode_u16s(&[0x00E4, 0x0000]);
        let s = read_park_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("ä".into()));
    }
}
