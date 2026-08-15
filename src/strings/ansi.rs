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
    strings::{NsisString, StringSegment, decode_short},
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
    /// `0xFC-0xFF`, used by NSIS 1 and 2.
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
            // Park is always Unicode and never reaches this decoder; NSIS 1
            // shares the NSIS 2 range.
            NsisVersion::V1 | NsisVersion::V2 | NsisVersion::Park => AnsiCodeRange::Nsis2,
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

fn classify_byte(b: u8, codes: AnsiCodeRange) -> AnsiCode {
    match codes {
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
pub fn read_ansi_string(
    table: &[u8],
    offset: usize,
    codes: AnsiCodeRange,
) -> Result<NsisString, Error> {
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

            // Read the 2-byte coded short.
            let (Some(p1), Some(p2)) = (pos.checked_add(1), pos.checked_add(2)) else {
                break;
            };
            let (Some(&hi), Some(&lo)) = (table.get(p1), table.get(p2)) else {
                break;
            };
            let val = decode_short(hi, lo);
            pos = pos.saturating_add(3);

            match code {
                AnsiCode::Var => segments.push(StringSegment::Variable(val)),
                AnsiCode::Shell => segments.push(StringSegment::ShellFolder(val)),
                AnsiCode::Lang => segments.push(StringSegment::LangString(val)),
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
    use crate::strings::encode_short;

    #[test]
    fn plain_string() {
        let table = b"Hello World\0rest";
        let s = read_ansi_string(table, 0, AnsiCodeRange::Nsis3).unwrap();
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

        let s = read_ansi_string(&table, 0, AnsiCodeRange::Nsis3).unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[0], StringSegment::Literal("Install to ".into()));
        assert_eq!(s.segments[1], StringSegment::Variable(21));
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

        let s = read_ansi_string(&table, 0, AnsiCodeRange::Nsis2).unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[0], StringSegment::Literal("Dir: ".into()));
        assert_eq!(s.segments[1], StringSegment::Variable(21));
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

        let first = read_ansi_string(&table, 0, AnsiCodeRange::Nsis3).unwrap();
        assert_eq!(
            first.segments,
            vec![StringSegment::Literal("gr\u{FC}\u{DF}e.txt".into())]
        );

        let second = read_ansi_string(&table, 10, AnsiCodeRange::Nsis3).unwrap();
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

        let as_nsis2 = read_ansi_string(&table, 0, AnsiCodeRange::Nsis2).unwrap();
        assert_eq!(as_nsis2.segments, vec![StringSegment::Variable(21)]);

        let as_nsis3 = read_ansi_string(&table, 0, AnsiCodeRange::Nsis3).unwrap();
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
        let decoded = read_ansi_string(&table, 0, AnsiCodeRange::Nsis2).unwrap();
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
        assert_eq!(
            AnsiCodeRange::for_version(NsisVersion::V1),
            AnsiCodeRange::Nsis2
        );
    }

    #[test]
    fn nsis2_shell_folder() {
        let (b0, b1) = encode_short(0x001A); // CSIDL_APPDATA
        let mut table = Vec::new();
        table.push(NS2_SHELL);
        table.push(b0);
        table.push(b1);
        table.extend_from_slice(b"\\MyApp\0");

        let s = read_ansi_string(&table, 0, AnsiCodeRange::Nsis2).unwrap();
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[0], StringSegment::ShellFolder(0x001A));
        assert_eq!(s.segments[1], StringSegment::Literal("\\MyApp".into()));
    }

    #[test]
    fn nsis3_skip_code() {
        let mut table = Vec::new();
        table.extend_from_slice(b"A");
        table.push(NS3_SKIP);
        table.push(0x03); // literal 0x03
        table.extend_from_slice(b"B\0");

        let s = read_ansi_string(&table, 0, AnsiCodeRange::Nsis3).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("A\x03B".into()));
    }

    #[test]
    fn nsis2_skip_code() {
        let table = vec![NS2_SKIP, NS2_VAR, 0]; // SKIP makes 0xFD literal

        let s = read_ansi_string(&table, 0, AnsiCodeRange::Nsis2).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(
            s.segments[0],
            StringSegment::Literal(String::from(NS2_VAR as char))
        );
    }

    #[test]
    fn string_at_offset() {
        let table = b"\0\0\0Hello\0";
        let s = read_ansi_string(table, 3, AnsiCodeRange::Nsis3).unwrap();
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0], StringSegment::Literal("Hello".into()));
    }

    #[test]
    fn empty_string() {
        let table = b"\0";
        let s = read_ansi_string(table, 0, AnsiCodeRange::Nsis3).unwrap();
        assert!(s.is_empty());
    }
}
