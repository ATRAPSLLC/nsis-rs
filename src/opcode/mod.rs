//! NSIS opcode definitions and version-aware resolution.
//!
//! NSIS uses approximately 71 opcodes (`EW_INVALID_OPCODE` through `EW_FGETWS`),
//! but the exact numbering shifts between NSIS versions due to conditional
//! compilation with `#ifdef`. This module provides:
//!
//! - [`OpcodeInfo`]: Static metadata for each opcode.
//! - [`NsisVersion`]: Version enum for version-aware opcode resolution.
//! - [`lookup`]: Resolves an opcode index to its info for a given version.
//!
//! Source: `fileform.h` and `exec.c` from the NSIS source code.

pub mod info;
pub mod version;

use crate::{nsis::entry::Entry, util::read_i32_le};

pub use info::OpcodeInfo;
pub use version::{NsisVersion, ParkSubVersion};

// Opcode indices from `fileform.h`.
// These are the `which` values stored in entry structures.

/// Invalid/error opcode.
pub const EW_INVALID_OPCODE: i32 = 0;
/// Return from function.
pub const EW_RET: i32 = 1;
/// No-op / jump.
pub const EW_NOP: i32 = 2;
/// Abort installation.
pub const EW_ABORT: i32 = 3;
/// Quit installer.
pub const EW_QUIT: i32 = 4;
/// Call subroutine.
pub const EW_CALL: i32 = 5;
/// Update status text.
pub const EW_UPDATETEXT: i32 = 6;
/// Sleep.
pub const EW_SLEEP: i32 = 7;
/// Bring window to front.
pub const EW_BRINGTOFRONT: i32 = 8;
/// Set details view.
pub const EW_CHDETAILSVIEW: i32 = 9;
/// Set file attributes.
pub const EW_SETFILEATTRIBUTES: i32 = 10;
/// Create directory.
pub const EW_CREATEDIR: i32 = 11;
/// If file exists.
pub const EW_IFFILEEXISTS: i32 = 12;
/// Set exec flag.
pub const EW_SETFLAG: i32 = 13;
/// If flag set.
pub const EW_IFFLAG: i32 = 14;
/// Get exec flag.
pub const EW_GETFLAG: i32 = 15;
/// Rename/move file.
pub const EW_RENAME: i32 = 16;
/// Get full path name.
pub const EW_GETFULLPATHNAME: i32 = 17;
/// Search PATH.
pub const EW_SEARCHPATH: i32 = 18;
/// Get temp filename.
pub const EW_GETTEMPFILENAME: i32 = 19;
/// Extract file from archive.
pub const EW_EXTRACTFILE: i32 = 20;
/// Delete file.
pub const EW_DELETEFILE: i32 = 21;
/// Message box.
pub const EW_MESSAGEBOX: i32 = 22;
/// Remove directory.
pub const EW_RMDIR: i32 = 23;
/// String length.
pub const EW_STRLEN: i32 = 24;
/// StrCpy.
pub const EW_ASSIGNVAR: i32 = 25;
/// String compare.
pub const EW_STRCMP: i32 = 26;
/// ReadEnvStr / ExpandEnvStrings.
pub const EW_READENVSTR: i32 = 27;
/// Integer compare.
pub const EW_INTCMP: i32 = 28;
/// Integer operation.
pub const EW_INTOP: i32 = 29;
/// IntFmt / Int64Fmt.
pub const EW_INTFMT: i32 = 30;
/// Push / Pop / Exch.
pub const EW_PUSHPOP: i32 = 31;
/// FindWindow.
pub const EW_FINDWINDOW: i32 = 32;
/// SendMessage.
pub const EW_SENDMESSAGE: i32 = 33;
/// IsWindow.
pub const EW_ISWINDOW: i32 = 34;
/// GetDlgItem.
pub const EW_GETDLGITEM: i32 = 35;
/// Set control colors.
pub const EW_SETCTLCOLORS: i32 = 36;
/// Load and set image.
pub const EW_LOADANDSETIMAGE: i32 = 37;
/// CreateFont.
pub const EW_CREATEFONT: i32 = 38;
/// ShowWindow.
pub const EW_SHOWWINDOW: i32 = 39;
/// ShellExecute.
pub const EW_SHELLEXEC: i32 = 40;
/// Exec / ExecWait.
pub const EW_EXECUTE: i32 = 41;
/// GetFileTime.
pub const EW_GETFILETIME: i32 = 42;
/// GetDLLVersion.
pub const EW_GETDLLVERSION: i32 = 43;
/// RegisterDLL / plugin call.
pub const EW_REGISTERDLL: i32 = 44;
/// CreateShortcut.
pub const EW_CREATESHORTCUT: i32 = 45;
/// CopyFiles.
pub const EW_COPYFILES: i32 = 46;
/// Reboot.
pub const EW_REBOOT: i32 = 47;
/// WriteINIStr.
pub const EW_WRITEINI: i32 = 48;
/// ReadINIStr.
pub const EW_READINISTR: i32 = 49;
/// DeleteRegValue / Key.
pub const EW_DELREG: i32 = 50;
/// WriteRegStr / DWORD / Bin.
pub const EW_WRITEREG: i32 = 51;
/// ReadRegStr / DWORD.
pub const EW_READREGSTR: i32 = 52;
/// RegEnumKey / Value.
pub const EW_REGENUM: i32 = 53;
/// FileClose.
pub const EW_FCLOSE: i32 = 54;
/// FileOpen.
pub const EW_FOPEN: i32 = 55;
/// FileWrite.
pub const EW_FPUTS: i32 = 56;
/// FileRead.
pub const EW_FGETS: i32 = 57;
/// FileSeek.
pub const EW_FSEEK: i32 = 58;
/// FindClose.
pub const EW_FINDCLOSE: i32 = 59;
/// FindNext.
pub const EW_FINDNEXT: i32 = 60;
/// FindFirst.
pub const EW_FINDFIRST: i32 = 61;
/// WriteUninstaller.
pub const EW_WRITEUNINSTALLER: i32 = 62;
/// LogText / LogSet.
pub const EW_LOG: i32 = 63;
/// SectionSet / GetText / Flags.
pub const EW_SECTIONSET: i32 = 64;
/// InstTypeSet / GetFlags.
pub const EW_INSTTYPESET: i32 = 65;
/// GetOSInfo / GetKnownFolderPath.
pub const EW_GETOSINFO: i32 = 66;
/// Reserved / free slot.
pub const EW_RESERVEDOPCODE: i32 = 67;
/// Lock / unlock window updates.
pub const EW_LOCKWINDOW: i32 = 68;
/// FileWriteUTF16LE.
pub const EW_FPUTWS: i32 = 69;
/// FileReadUTF16LE.
pub const EW_FGETWS: i32 = 70;

/// Normalizes a Park raw opcode to its V2-equivalent opcode number.
///
/// Park builds insert extra opcodes into the table, shifting subsequent
/// opcode numbers upward. This function reverses that shift so the raw
/// opcode can be looked up in the V2 table.
///
/// Implements the same logic as 7-Zip `NsisIn.cpp` `GetCmd()`.
pub fn normalize_park_opcode(raw: u32, sub: ParkSubVersion) -> u32 {
    let mut a = raw;

    // Opcodes below EW_REGISTERDLL (44) are the same in all versions.
    if a < EW_REGISTERDLL as u32 {
        return a;
    }

    // Park2+: GetFontVersion inserted at position 44.
    if matches!(sub, ParkSubVersion::Park2 | ParkSubVersion::Park3) {
        if a == EW_REGISTERDLL as u32 {
            // This raw opcode is the inserted GetFontVersion — not a V2
            // opcode. Return it as-is so lookup() returns None (or
            // the caller can handle it).
            return raw;
        }
        a = a.saturating_sub(1);
    }

    // Park3+: GetFontName inserted at position 44 (after the Park2 shift).
    if sub == ParkSubVersion::Park3 {
        if a == EW_REGISTERDLL as u32 {
            return raw; // inserted GetFontName
        }
        a = a.saturating_sub(1);
    }

    // Unicode Park: EW_FPUTWS and EW_FGETWS inserted before EW_FSEEK.
    // Park is always Unicode.
    if a >= EW_FSEEK as u32 {
        if a == EW_FSEEK as u32 {
            return EW_FPUTWS as u32;
        }
        if a == (EW_FSEEK as u32).saturating_add(1) {
            return EW_FGETWS as u32;
        }
        a = a.saturating_sub(2);
    }

    a
}

/// Detects the Park sub-version by scanning entry opcodes.
///
/// Replicates 7-Zip's `DetectNsisType()` logic: find entries whose raw
/// opcode falls in `[EW_WRITEUNINSTALLER .. EW_WRITEUNINSTALLER + 4]` and
/// whose parameters match the WriteUninstaller signature (param\[0\] > 1,
/// param\[3\] > 1, param\[4\] == 0, param\[5\] == 0).
///
/// The offset from `EW_WRITEUNINSTALLER` reveals how many extra opcodes
/// were inserted, which identifies the sub-version.
pub fn detect_park_sub_version(
    header_data: &[u8],
    entry_block_offset: usize,
    entry_count: usize,
) -> ParkSubVersion {
    // The maximum number of extra inserts for Unicode Park is 4.
    let base = EW_WRITEUNINSTALLER;
    let max_raw = base + 4;

    let mut mask: u32 = 0;

    for i in 0..entry_count {
        let Some(offset) = i
            .checked_mul(Entry::SIZE)
            .and_then(|n| n.checked_add(entry_block_offset))
        else {
            break;
        };
        let Some(end) = offset.checked_add(Entry::SIZE) else {
            break;
        };
        if end > header_data.len() {
            break;
        }
        let raw_cmd = read_i32_le(header_data, offset);
        if raw_cmd < base || raw_cmd > max_raw {
            continue;
        }

        // Read params.
        let p0 = read_i32_le(header_data, offset.saturating_add(4));
        let p3 = read_i32_le(header_data, offset.saturating_add(16));
        let p4 = read_i32_le(header_data, offset.saturating_add(20));
        let p5 = read_i32_le(header_data, offset.saturating_add(24));

        // Filter: must have valid path strings and zero in params[4..5].
        if p4 != 0 || p5 != 0 || p0 <= 1 || p3 <= 1 {
            continue;
        }

        let num_inserts = raw_cmd.saturating_sub(base) as u32;
        mask |= 1_u32.checked_shl(num_inserts).unwrap_or(0);
    }

    // Park sub-version from mask (Unicode mode).
    // Source: 7-Zip NsisIn.cpp lines 2656-2661.
    match mask {
        m if m & (1 << 4) != 0 => ParkSubVersion::Park3,
        m if m & (1 << 3) != 0 => ParkSubVersion::Park2,
        _ => ParkSubVersion::Park1,
    }
}

/// Looks up opcode metadata for the given opcode index and NSIS version.
///
/// Returns `None` if the opcode index is out of range for the given version.
pub fn lookup(version: NsisVersion, which: u32) -> Option<&'static OpcodeInfo> {
    let table: &[OpcodeInfo] = match version {
        NsisVersion::V2 => &info::OPCODES_NSIS2,
        NsisVersion::V3 => info::OPCODES_NSIS3,
        NsisVersion::V1 | NsisVersion::Park => &info::OPCODES_NSIS2,
    };

    table.get(which as usize)
}

/// Looks up opcode metadata with Park-aware normalization.
///
/// For Park version, the raw opcode is first normalized to its V2 equivalent
/// before table lookup.
pub fn lookup_normalized(
    version: NsisVersion,
    which: u32,
    park_sub: Option<ParkSubVersion>,
) -> Option<&'static OpcodeInfo> {
    let normalized = match (version, park_sub) {
        (NsisVersion::Park, Some(sub)) => normalize_park_opcode(which, sub),
        _ => which,
    };
    lookup(version, normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_valid_opcode() {
        let info = lookup(NsisVersion::V2, 0);
        assert!(info.is_some());
        assert_eq!(info.unwrap().mnemonic, "EW_INVALID_OPCODE");
    }

    #[test]
    fn lookup_ret() {
        let info = lookup(NsisVersion::V2, 1).unwrap();
        assert_eq!(info.mnemonic, "EW_RET");
    }

    #[test]
    fn lookup_out_of_range() {
        assert!(lookup(NsisVersion::V2, 999).is_none());
    }

    #[test]
    fn lookup_v3() {
        let info = lookup(NsisVersion::V3, 0).unwrap();
        assert_eq!(info.mnemonic, "EW_INVALID_OPCODE");
    }
}
