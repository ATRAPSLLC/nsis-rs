//! ANSI string decoding for NSIS.
//!
//! In ANSI mode, strings are single-byte characters with embedded special
//! codes followed by 2 coded bytes containing a 14-bit value.
//!
//! Two code ranges exist depending on the NSIS version:
//!
//! | Version | SKIP | VAR | SHELL | LANG |
//! |---------|------|-----|-------|------|
//! | NSIS 3.x | 0x04 | 0x03 | 0x02 | 0x01 |
//! | NSIS 2.x | 0xFC (252) | 0xFD (253) | 0xFE (254) | 0xFF (255) |
//!
//! Only one range is live in any given table, and which one must be known
//! before decoding: honouring both at once corrupts text, because each range
//! is ordinary character data under the other convention. `0xFC-0xFF` are the
//! Latin-1 characters `ü ý þ ÿ`, and `0x01-0x04` are control characters.
//!
//! Sources: `fileform.h`, NRS `nsis2.py` / `nsis3.py`.

use crate::{
    error::Error,
    opcode::NsisVersion,
    strings::{NsisString, StringSegment, StringTable, decode_short},
};

/// NSIS 3.x ANSI special codes.
const NS3_LANG: u8 = 0x01;
const NS3_SHELL: u8 = 0x02;
const NS3_VAR: u8 = 0x03;
const NS3_SKIP: u8 = 0x04;

/// NSIS 2.x ANSI special codes.
const NS2_SKIP: u8 = 0xFC;
const NS2_VAR: u8 = 0xFD;
const NS2_SHELL: u8 = 0xFE;
const NS2_LANG: u8 = 0xFF;

/// Which of the two ANSI special-code ranges a string table uses.
///
/// NSIS 3 moved the codes from the top of the byte range to the bottom. The
/// ranges overlap with real text in both directions, so a decoder has to be
/// told which one applies rather than accepting either — see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiCodeRange {
    /// `0xDD-0xFF`, used by NSIS 1. Each code byte is a whole variable
    /// reference and there are no shell or language codes — a different
    /// encoding rather than a different range, decoded by
    /// [`strings::v1`](crate::strings::v1).
    Nsis1,
    /// `0xFC-0xFF`, used by NSIS 2.
    Nsis2,
    /// `0x01-0x04`, used by NSIS 3 builds that target ANSI.
    Nsis3,
}

impl AnsiCodeRange {
    /// Returns the range a given NSIS version writes.
    #[inline]
    pub fn for_version(version: NsisVersion) -> Self {
        match version {
            NsisVersion::V3 => AnsiCodeRange::Nsis3,
            NsisVersion::V1 => AnsiCodeRange::Nsis1,
            // Park is always Unicode and never reaches this decoder.
            NsisVersion::V2 | NsisVersion::Park => AnsiCodeRange::Nsis2,
        }
    }
}

/// Classifies a byte as an NSIS special code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiCode {
    Literal,
    Skip,
    Var,
    Shell,
    Lang,
}

/// Returns `true` if `b` introduces a special code in the given range.
pub(crate) fn is_special_code(b: u8, codes: AnsiCodeRange) -> bool {
    classify_byte(b, codes) != AnsiCode::Literal
}

fn classify_byte(b: u8, codes: AnsiCodeRange) -> AnsiCode {
    match codes {
        // Every byte at or above the 1.x code start is a variable in its own
        // right, so nothing here introduces a multi-byte code.
        AnsiCodeRange::Nsis1 => {
            if b >= crate::strings::v1::VAR_CODES_START {
                AnsiCode::Var
            } else {
                AnsiCode::Literal
            }
        }
        AnsiCodeRange::Nsis3 => match b {
            NS3_LANG => AnsiCode::Lang,
            NS3_SHELL => AnsiCode::Shell,
            NS3_VAR => AnsiCode::Var,
            NS3_SKIP => AnsiCode::Skip,
            _ => AnsiCode::Literal,
        },
        AnsiCodeRange::Nsis2 => match b {
            NS2_LANG => AnsiCode::Lang,
            NS2_SHELL => AnsiCode::Shell,
            NS2_VAR => AnsiCode::Var,
            NS2_SKIP => AnsiCode::Skip,
            _ => AnsiCode::Literal,
        },
    }
}

/// Reads an ANSI-encoded NSIS string from the string table.
///
/// The string starts at `offset` and continues until a null byte (`0x00`).
/// `codes` selects the special-code range; bytes outside it are literal text.
pub fn read_ansi_string(context: &StringTable<'_>, offset: usize) -> Result<NsisString, Error> {
    let table = context.bytes();
    let codes = context.ansi_code_range();
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut pos = offset;

    while let Some(&b) = table.get(pos) {
        if b == 0 {
            break;
        }

        let code = classify_byte(b, codes);

        if code != AnsiCode::Literal {
            if code == AnsiCode::Skip {
                // Next byte is a literal character (no flush needed).
                pos = pos.saturating_add(1);
                if let Some(&next) = table.get(pos) {
                    literal.push(next as char);
                }
                pos = pos.saturating_add(1);
                continue;
            }

            // Flush accumulated literal before emitting a special segment.
            if !literal.is_empty() {
                segments.push(StringSegment::Literal(literal.clone()));
                literal.clear();
            }

            // Both argument bytes follow the code.
            let (Some(p1), Some(p2)) = (pos.checked_add(1), pos.checked_add(2)) else {
                break;
            };
            let (Some(&first), Some(&second)) = (table.get(p1), table.get(p2)) else {
                break;
            };
            pos = pos.saturating_add(3);

            match code {
                // Shell folder ids are two independent bytes. The 14-bit
                // transform applies to numbers, and a folder pair is not one —
                // running it through the transform mixes the fallback id into
                // the primary. 7-Zip and Binary Refinery both pass the raw pair.
                AnsiCode::Shell => segments.push(StringSegment::ShellFolder {
                    primary: first,
                    fallback: second,
                    target: context.shell_target(first, second),
                }),
                AnsiCode::Var => {
                    let index = decode_short(first, second);
                    segments.push(StringSegment::Variable {
                        index,
                        name: context.variable_name(index),
                    });
                }
                AnsiCode::Lang => {
                    segments.push(StringSegment::LangString(decode_short(first, second)));
                }
                _ => {}
            }
        } else {
            literal.push(b as char);
            pos = pos.saturating_add(1);
        }
    }

    if !literal.is_empty() {
        segments.push(StringSegment::Literal(literal));
    }

    Ok(NsisString { segments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::testing::variable_name_for;
    use crate::strings::{DEFAULT_INTERNAL_VARS, ShellTarget, StringEncoding, encode_short};

    /// Wraps raw table bytes in the context the decoder needs.
    fn context(table: &[u8], codes: AnsiCodeRange) -> StringTable<'_> {
        StringTable::new(table, 0, StringEncoding::Ansi, codes, DEFAULT_INTERNAL_VARS)
    }

    #[test]
    fn plain_string() {
        let table = b"Hello World\0rest";
        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("Hello World".into()));
    }

    #[test]
    fn nsis3_variable() {
        // NSIS 3.x: NS_VAR_CODE = 0x03
        let (b0, b1) = encode_short(21);
        let mut table = Vec::new();
        table.extend_from_slice(b"Install to ");
        table.push(NS3_VAR);
        table.push(b0);
        table.push(b1);
        table.push(0);

        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 0).unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[0], StringSegment::Literal("Install to ".into()));
        assert_eq!(
            s.segments[1],
            StringSegment::Variable {
                index: 21,
                name: variable_name_for(21)
            }
        );
        assert_eq!(s.to_string(), "Install to $INSTDIR");
    }

    #[test]
    fn nsis2_variable() {
        // NSIS 2.x: NS_VAR_CODE = 0xFD
        let (b0, b1) = encode_short(21);
        let mut table = Vec::new();
        table.extend_from_slice(b"Dir: ");
        table.push(NS2_VAR);
        table.push(b0);
        table.push(b1);
        table.push(0);

        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis2), 0).unwrap();
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
    fn latin1_text_survives_the_nsis3_range() {
        // Regression: accepting both ranges at once made `0xFC` a SKIP code,
        // which swallowed the following byte, and `0xFE` a shell-folder code.
        // In an NSIS 3 ANSI table these are the characters `ü` and `þ`.
        // "grüße.txt" and "þýÿ.ini" in Windows-1252.
        let table = [
            b'g', b'r', 0xFC, 0xDF, b'e', b'.', b't', b'x', b't', 0x00, 0xFE, 0xFD, 0xFF, b'.',
            b'i', b'n', b'i', 0x00,
        ];

        let first = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 0).unwrap();
        assert_eq!(
            first.segments,
            vec![StringSegment::Literal("gr\u{FC}\u{DF}e.txt".into())]
        );

        let second = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 10).unwrap();
        assert_eq!(
            second.segments,
            vec![StringSegment::Literal("\u{FE}\u{FD}\u{FF}.ini".into())]
        );
    }

    #[test]
    fn the_same_bytes_decode_differently_per_range() {
        // The two ranges are not distinguishable byte by byte, which is why the
        // decoder has to be told which applies. Here 0xFD is either a variable
        // reference or the character `ý`.
        let (b0, b1) = encode_short(21);
        let table = [0xFD, b0, b1, 0x00];

        let as_nsis2 = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis2), 0).unwrap();
        assert_eq!(
            as_nsis2.segments,
            vec![StringSegment::Variable {
                index: 21,
                name: variable_name_for(21)
            }]
        );

        let as_nsis3 = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 0).unwrap();
        assert_eq!(
            as_nsis3.segments,
            vec![StringSegment::Literal(format!(
                "\u{FD}{}{}",
                b0 as char, b1 as char
            ))]
        );
    }

    #[test]
    fn control_characters_survive_the_nsis2_range() {
        // The mirror image: 0x01-0x04 are ordinary control characters in an
        // NSIS 2 table, and an NSIS 2 script can legitimately contain them.
        let table = [b'a', 0x03, 0x95, 0x80, b'b', 0x00];
        let decoded = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis2), 0).unwrap();
        assert_eq!(
            decoded.segments,
            vec![StringSegment::Literal("a\u{3}\u{95}\u{80}b".into())]
        );
    }

    #[test]
    fn code_range_follows_the_version() {
        assert_eq!(
            AnsiCodeRange::for_version(NsisVersion::V3),
            AnsiCodeRange::Nsis3
        );
        assert_eq!(
            AnsiCodeRange::for_version(NsisVersion::V2),
            AnsiCodeRange::Nsis2
        );
        // NSIS 1 does not share NSIS 2's range: its references are one byte
        // each, in a range NSIS 2 reads as ordinary text.
        assert_eq!(
            AnsiCodeRange::for_version(NsisVersion::V1),
            AnsiCodeRange::Nsis1
        );
    }

    #[test]
    fn nsis2_shell_folder() {
        // The two folder ids follow the code as raw bytes, not as a coded
        // number: 0x1A is CSIDL_APPDATA, 0x23 the common-profile equivalent
        // used when the primary is unavailable.
        let mut table = vec![NS2_SHELL, 0x1A, 0x23];
        table.extend_from_slice(b"\\MyApp\0");

        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis2), 0).unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(
            s.segments[0],
            StringSegment::ShellFolder {
                primary: 0x1A,
                fallback: 0x23,
                target: ShellTarget::Csidl("APPDATA"),
            }
        );
        assert_eq!(s.segments[1], StringSegment::Literal("\\MyApp".into()));
    }

    #[test]
    fn shell_ids_are_not_run_through_the_number_transform() {
        // Regression: decoding the pair as a 14-bit number folds the fallback
        // id into the primary, so `81 20` (a registry lookup) came out as
        // CSIDL 1. Real data: the InstallDir of the ansi3_deflate_nonsolid
        // fixture.
        let table = vec![NS3_SHELL, 0x81, 0x20, 0x00];
        let s = read_ansi_string(&context(&table, AnsiCodeRange::Nsis3), 0).unwrap();
        match &s.segments[..] {
            [
                StringSegment::ShellFolder {
                    primary, fallback, ..
                },
            ] => {
                assert_eq!((*primary, *fallback), (0x81, 0x20));
            }
            other => panic!("expected one shell segment, got {other:?}"),
        }
    }

    #[test]
    fn nsis3_skip_code() {
        let mut table = Vec::new();
        table.extend_from_slice(b"A");
        table.push(NS3_SKIP);
        table.push(0x03); // literal 0x03
        table.extend_from_slice(b"B\0");

        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("A\x03B".into()));
    }

    #[test]
    fn nsis2_skip_code() {
        let table = vec![NS2_SKIP, NS2_VAR, 0]; // SKIP makes 0xFD literal

        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis2), 0).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(
            s.segments[0],
            StringSegment::Literal(String::from(NS2_VAR as char))
        );
    }

    #[test]
    fn string_at_offset() {
        let table = b"\0\0\0Hello\0";
        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 3).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("Hello".into()));
    }

    #[test]
    fn empty_string() {
        let table = b"\0";
        let s = read_ansi_string(&context(table.as_ref(), AnsiCodeRange::Nsis3), 0).unwrap();
        assert!(s.is_empty());
    }
}
