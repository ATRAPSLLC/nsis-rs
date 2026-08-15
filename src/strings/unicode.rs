//! Unicode (UTF-16LE) string decoding for NSIS 3.x.
//!
//! In Unicode mode, strings are UTF-16LE encoded. Special codes are
//! the u16 values `0x0001` through `0x0004`. After each special code
//! (except SKIP), a single u16 argument follows, encoding a 14-bit value
//! in its two bytes: `index = (w & 0x7F) | (((w >> 8) & 0x7F) << 7)`.
//!
//! Source: 7-Zip `NsisIn.cpp` `CONVERT_NUMBER_NS_3_UNICODE` macro.

use crate::{
    error::Error,
    strings::{NsisString, StringSegment, StringTable},
};

/// NSIS 3.x Unicode special codes (as UTF-16LE code units).
const NS_LANG_CODE_W: u16 = 0x0001;
const NS_SHELL_CODE_W: u16 = 0x0002;
const NS_VAR_CODE_W: u16 = 0x0003;
const NS_SKIP_CODE_W: u16 = 0x0004;

/// Reads a UTF-16LE code unit from the table at the given byte offset.
fn read_u16(table: &[u8], offset: usize) -> Option<u16> {
    table
        .get(offset..)
        .and_then(|s| s.first_chunk::<2>())
        .copied()
        .map(u16::from_le_bytes)
}

/// Decodes a 14-bit value from a single NSIS 3 Unicode argument u16.
///
/// The encoding stores a 14-bit value across the two bytes of a u16,
/// with bit 7 of each byte reserved (set to 1 as a marker):
/// `index = (w & 0x7F) | (((w >> 8) & 0x7F) << 7)`
///
/// Source: 7-Zip `NsisIn.cpp` `CONVERT_NUMBER_NS_3_UNICODE(n)`.
fn decode_nsis3_arg(w: u16) -> u16 {
    (w & 0x7F) | (((w >> 8) & 0x7F) << 7)
}

/// Reads a Unicode (UTF-16LE) encoded NSIS 3.x string from the string table.
///
/// The string starts at byte `offset` and continues until a null code unit
/// (`0x0000`).
pub fn read_unicode_string(context: &StringTable<'_>, offset: usize) -> Result<NsisString, Error> {
    let table = context.bytes();
    let mut segments = Vec::new();
    let mut literal_chars: Vec<u16> = Vec::new();
    let mut pos = offset;

    while let Some(ch) = read_u16(table, pos) {
        if ch == 0 {
            break;
        }

        if (NS_LANG_CODE_W..=NS_SKIP_CODE_W).contains(&ch) {
            if ch == NS_SKIP_CODE_W {
                // Next code unit is literal.
                pos = pos.saturating_add(2);
                if let Some(next) = read_u16(table, pos) {
                    literal_chars.push(next);
                }
                pos = pos.saturating_add(2);
                continue;
            }

            // Flush accumulated literal before emitting a special segment.
            if !literal_chars.is_empty() {
                let s = String::from_utf16_lossy(&literal_chars);
                segments.push(StringSegment::Literal(s));
                literal_chars.clear();
            }

            // Read ONE argument u16.
            let Some(arg_pos) = pos.checked_add(2) else {
                break;
            };
            let Some(arg) = read_u16(table, arg_pos) else {
                break;
            };
            pos = pos.saturating_add(4); // special code (2) + argument (2)

            match ch {
                NS_VAR_CODE_W => {
                    // Variables use the 14-bit coded conversion.
                    let index = decode_nsis3_arg(arg);
                    segments.push(StringSegment::Variable {
                        index,
                        name: context.variable_name(index),
                    });
                }
                NS_SHELL_CODE_W => {
                    // Two folder ids packed into the argument, low byte first.
                    // Not the 14-bit conversion, which is for numbers.
                    // Source: 7-Zip NsisIn.cpp:1022
                    //   GetShellString(Raw_AString, n & 0xFF, n >> 8)
                    let (primary, fallback) = ((arg & 0xFF) as u8, (arg >> 8) as u8);
                    segments.push(StringSegment::ShellFolder {
                        primary,
                        fallback,
                        target: context.shell_target(primary, fallback),
                    });
                }
                NS_LANG_CODE_W => {
                    // Language codes use the 14-bit coded conversion.
                    segments.push(StringSegment::LangString(decode_nsis3_arg(arg)));
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
        DEFAULT_INTERNAL_VARS, StringEncoding, ansi::AnsiCodeRange, testing::variable_name_for,
    };

    /// Wraps raw table bytes in the context the decoder needs.
    fn context(table: &[u8]) -> StringTable<'_> {
        StringTable::new(
            table,
            0,
            StringEncoding::Unicode,
            AnsiCodeRange::Nsis3,
            DEFAULT_INTERNAL_VARS,
        )
    }

    /// Encodes a string as UTF-16LE bytes with a null terminator.
    fn encode_utf16(s: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        for ch in s.encode_utf16() {
            buf.extend_from_slice(&ch.to_le_bytes());
        }
        buf.extend_from_slice(&[0x00, 0x00]);
        buf
    }

    /// Encodes a 14-bit value into the NSIS 3 Unicode argument format.
    fn encode_nsis3_arg(val: u16) -> u16 {
        let lo = (val & 0x7F) | 0x80;
        let hi = ((val >> 7) & 0x7F) | 0x80;
        lo | (hi << 8)
    }

    #[test]
    fn plain_string() {
        let table = encode_utf16("Hello");
        let s = read_unicode_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("Hello".into()));
    }

    #[test]
    fn string_with_variable() {
        // "Dir: " + NS_VAR_CODE + encoded(21) for $INSTDIR
        let mut table = Vec::new();
        for ch in "Dir: ".encode_utf16() {
            table.extend_from_slice(&ch.to_le_bytes());
        }
        table.extend_from_slice(&NS_VAR_CODE_W.to_le_bytes());
        table.extend_from_slice(&encode_nsis3_arg(21).to_le_bytes());
        table.extend_from_slice(&[0x00, 0x00]);

        let s = read_unicode_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[0], StringSegment::Literal("Dir: ".into()));
        assert_eq!(
            s.segments[1],
            StringSegment::Variable {
                index: 21,
                name: variable_name_for(21)
            }
        );
    }

    #[test]
    fn decode_roundtrip() {
        for val in [0u16, 1, 21, 26, 30, 127, 128, 0x3FFF] {
            let encoded = encode_nsis3_arg(val);
            assert_eq!(decode_nsis3_arg(encoded), val, "roundtrip failed for {val}");
        }
    }

    #[test]
    fn empty_string() {
        let table = [0x00, 0x00];
        let s = read_unicode_string(&context(&table), 0).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn skip_code() {
        let mut table = Vec::new();
        table.extend_from_slice(&NS_SKIP_CODE_W.to_le_bytes());
        table.extend_from_slice(&0x0003u16.to_le_bytes());
        table.extend_from_slice(&[0x00, 0x00]);

        let s = read_unicode_string(&context(&table), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert!(matches!(&s.segments[0], StringSegment::Literal(_)));
    }
}
