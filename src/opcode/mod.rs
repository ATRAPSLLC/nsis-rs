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

use crate::{
    nsis::entry::{Entry, MAX_ENTRY_OFFSETS},
    util::read_i32_le,
};

pub use info::OpcodeInfo;
pub use version::{Nsis2SubVersion, NsisVersion, ParkSubVersion};

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
/// SectionSet / GetText / Flags.
pub const EW_SECTIONSET: i32 = 63;
/// InstTypeSet / GetFlags.
pub const EW_INSTTYPESET: i32 = 64;
/// GetOSInfo / GetKnownFolderPath (NSIS 3.06+).
pub const EW_GETOSINFO: i32 = 65;
/// Reserved / free slot.
pub const EW_RESERVEDOPCODE: i32 = 66;
/// Lock / unlock window updates.
pub const EW_LOCKWINDOW: i32 = 67;
/// FileWriteUTF16LE.
pub const EW_FPUTWS: i32 = 68;
/// FileReadUTF16LE.
pub const EW_FGETWS: i32 = 69;

// The opcodes below are never stored in an installer. NSIS compiles some
// instructions conditionally, which shifts the numbering of everything after
// them, so this crate translates those layouts into the one above and uses
// these slots for the instructions that only exist in a conditional build.

/// LogText / LogSet, present only in a build compiled with logging.
pub const EW_LOG: i32 = 70;
/// FindProc, present only in the Park fork.
pub const EW_FINDPROC: i32 = 71;

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

/// Translates a raw opcode from a log-enabled build into the main layout.
///
/// NSIS compiles the log instructions conditionally. A build with logging has
/// an extra opcode between `EW_WRITEUNINSTALLER` and `EW_SECTIONSET`, so every
/// opcode from that point on is stored one higher than in a standard build.
/// This maps that raw number back: the inserted slot becomes [`EW_LOG`], and
/// everything above it shifts down by one.
///
/// Applies to non-Park installers only; Park has its own insertions, handled by
/// [`normalize_park_opcode`].
///
/// # Source
///
/// 7-Zip `NsisIn.cpp` `GetCmd`, the `LogCmdIsEnabled` branch.
pub fn normalize_log_opcode(raw: u32) -> u32 {
    let boundary = EW_SECTIONSET as u32;
    if raw < boundary {
        raw
    } else if raw == boundary {
        EW_LOG as u32
    } else {
        raw.saturating_sub(1)
    }
}

/// Reports whether an entry block can be read under the given opcode layout.
///
/// Returns the lowest opcode that contradicts the layout, or `None` if the
/// whole block is consistent with it. An opcode contradicts the layout when it
/// is not in the table, when it is one that no installer stores, or — the
/// telling case — when the entry passes **more parameters than that opcode
/// takes**. Conditional compilation shifts opcode numbers, so reading a block
/// under the wrong layout tends to land instructions on opcodes whose
/// parameter counts do not fit.
///
/// `resolve` maps a raw opcode to its table entry under the layout being
/// tested.
///
/// # Source
///
/// 7-Zip `NsisIn.cpp` `FindBadCmd`.
pub fn find_bad_opcode(
    header_data: &[u8],
    entry_block_offset: usize,
    entry_count: usize,
    resolve: impl Fn(u32) -> Option<&'static OpcodeInfo>,
) -> Option<u32> {
    let mut worst: Option<u32> = None;

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

        let raw = read_i32_le(header_data, offset);
        let Ok(raw) = u32::try_from(raw) else {
            continue;
        };
        if worst.is_some_and(|bad| raw >= bad) {
            continue;
        }

        let Some(info) = resolve(raw) else {
            worst = Some(raw);
            continue;
        };
        // Slots the crate uses for translation are never stored in a file.
        if info.mnemonic == "EW_LOG" || info.mnemonic == "EW_FINDPROC" {
            worst = Some(raw);
            continue;
        }

        // Highest parameter slot the entry actually uses.
        let used = (0..MAX_ENTRY_OFFSETS)
            .rev()
            .find(|n| {
                let slot = 4_usize.saturating_add(4_usize.saturating_mul(*n));
                read_i32_le(header_data, offset.saturating_add(slot)) != 0
            })
            .map_or(0, |n| n.saturating_add(1));

        if used > info.param_count as usize {
            worst = Some(raw);
        }
    }

    worst
}

/// Detects which NSIS 2.x variable layout an installer uses.
///
/// The layouts are indistinguishable from the header alone, so this replicates
/// 7-Zip's approach and looks for instructions that name a variable whose index
/// moved between releases:
///
/// - `EW_GETDLGITEM` whose second parameter is exactly `$HWNDPARENT` proves the
///   installer predates 2.26, where that variable sat at index 27 rather than
///   29. If its first parameter is also 29 — `$_OUTDIR` in that layout — the
///   installer predates 2.04.
/// - `EW_ASSIGNVAR` writing variable 29 from exactly `$OUTDIR`, with no
///   substring parameters, is the `StrCpy $_OUTDIR $OUTDIR` that only the
///   pre-2.26 layout produces.
///
/// Returns [`Nsis2SubVersion::From226`] when nothing matches.
///
/// # Limits
///
/// Both markers are instructions a script has to actually use. An installer
/// that never touches a dialog item and never copies `$OUTDIR` — a plain
/// `SetOutPath` plus `File` script, for instance — carries no evidence of its
/// layout at all, and is reported as [`Nsis2SubVersion::From226`] whatever
/// version built it. 7-Zip has the same blind spot and makes the same
/// assumption. It only matters for installers that also reference a variable
/// whose index moved, which such a minimal script does not.
///
/// `read_var_index` resolves a string-table offset to the variable index of a
/// string consisting of exactly one variable reference, or `None` for anything
/// else — the equivalent of 7-Zip's `IsVarStr`.
///
/// # Source
///
/// 7-Zip `NsisIn.cpp`, the `IsNsis225` / `IsNsis200` block of `DetectNsisType`.
pub fn detect_nsis2_sub_version(
    header_data: &[u8],
    entry_block_offset: usize,
    entry_count: usize,
    read_var_index: impl Fn(i32) -> Option<u16>,
) -> Nsis2SubVersion {
    /// `$HWNDPARENT` in the pre-2.26 layout.
    const VAR_HWNDPARENT_225: u16 = 27;
    /// `$OUTDIR`, at the same index in every layout.
    const VAR_OUTDIR: u16 = 22;
    /// `$_OUTDIR` in the pre-2.26 layout.
    const VAR_SPEC_OUTDIR_225: i32 = 29;

    let mut sub = Nsis2SubVersion::From226;

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

        let which = read_i32_le(header_data, offset);
        let param = |n: usize| {
            let slot = 4_usize.saturating_add(4_usize.saturating_mul(n));
            read_i32_le(header_data, offset.saturating_add(slot))
        };

        match which {
            EW_GETDLGITEM => {
                if read_var_index(param(1)) == Some(VAR_HWNDPARENT_225) {
                    sub = Nsis2SubVersion::UpTo225;
                    if param(0) == VAR_SPEC_OUTDIR_225 {
                        // Only 2.03 and earlier put `$_OUTDIR` in a dialog-item
                        // handle; this is the strongest signal available, so
                        // stop looking.
                        return Nsis2SubVersion::UpTo203;
                    }
                }
            }
            EW_ASSIGNVAR
                if param(0) == VAR_SPEC_OUTDIR_225
                    && param(2) == 0
                    && param(3) == 0
                    && read_var_index(param(1)) == Some(VAR_OUTDIR) =>
            {
                sub = Nsis2SubVersion::UpTo225;
            }
            _ => {}
        }
    }

    sub
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

    /// Builds an entry block from `(opcode, params)` pairs.
    fn entry_block(entries: &[(i32, [i32; 6])]) -> Vec<u8> {
        let mut data = Vec::new();
        for (which, params) in entries {
            data.extend_from_slice(&which.to_le_bytes());
            for param in params {
                data.extend_from_slice(&param.to_le_bytes());
            }
        }
        data
    }

    /// Resolves the string offsets used by the tests below to variable indices,
    /// standing in for a real string table: offset 100 is `$HWNDPARENT` in the
    /// pre-2.26 layout, offset 200 is `$OUTDIR`, and anything else is not a
    /// bare variable reference.
    fn var_index(offset: i32) -> Option<u16> {
        match offset {
            100 => Some(27),
            200 => Some(22),
            _ => None,
        }
    }

    #[test]
    fn log_builds_shift_every_opcode_above_the_boundary() {
        // Below the boundary the two layouts agree.
        assert_eq!(normalize_log_opcode(EW_WRITEUNINSTALLER as u32), 62);
        // The inserted instruction itself.
        assert_eq!(normalize_log_opcode(63), EW_LOG as u32);
        // Everything above it is stored one higher than the main layout.
        assert_eq!(normalize_log_opcode(64), EW_SECTIONSET as u32);
        assert_eq!(normalize_log_opcode(66), EW_GETOSINFO as u32);
        assert_eq!(normalize_log_opcode(70), EW_FGETWS as u32);
    }

    #[test]
    fn a_consistent_block_contradicts_no_layout() {
        let data = entry_block(&[
            (EW_CREATEDIR, [1, 1, 0, 0, 0, 0]),
            (EW_EXTRACTFILE, [0, 2, 3, 0, 0, 0]),
            (EW_RET, [0; 6]),
        ]);
        assert_eq!(
            find_bad_opcode(&data, 0, 3, |raw| lookup(NsisVersion::V3, raw)),
            None
        );
    }

    #[test]
    fn too_many_parameters_contradicts_the_layout() {
        // `EW_RET` takes none, so an entry passing one cannot be a return.
        let data = entry_block(&[(EW_RET, [0, 0, 0, 0, 0, 7])]);
        assert_eq!(
            find_bad_opcode(&data, 0, 1, |raw| lookup(NsisVersion::V3, raw)),
            Some(EW_RET as u32)
        );
    }

    #[test]
    fn translation_only_opcodes_contradict_the_layout() {
        // No installer stores these; seeing one means the layout is wrong.
        for slot in [EW_LOG, EW_FINDPROC] {
            let data = entry_block(&[(slot, [0; 6])]);
            assert_eq!(
                find_bad_opcode(&data, 0, 1, |raw| lookup(NsisVersion::V3, raw)),
                Some(slot as u32),
                "{slot} should not appear in a file"
            );
        }
    }

    #[test]
    fn the_log_layout_resolves_a_block_the_standard_one_cannot() {
        // A log-enabled build stores SectionSet at 64 and LockWindow at 68.
        // Read as a standard build, 64 is InstTypeSet (4 parameters) and 68 is
        // FileWriteUTF16LE (4) — both fit, so this block alone is ambiguous.
        // Raw 70 is the giveaway: standard reads it as a translation-only slot.
        let data = entry_block(&[
            (64, [1, 2, 3, 0, 0, 0]),
            (70, [1, 2, 0, 0, 0, 0]), // FileReadUTF16LE in a log build
        ]);

        assert!(
            find_bad_opcode(&data, 0, 2, |raw| lookup(NsisVersion::V3, raw)).is_some(),
            "the standard layout should not explain this block"
        );
        assert_eq!(
            find_bad_opcode(&data, 0, 2, |raw| lookup(
                NsisVersion::V3,
                normalize_log_opcode(raw)
            )),
            None,
            "the log layout should explain it"
        );
    }

    #[test]
    fn nsis2_sub_version_defaults_to_the_modern_layout() {
        // A script that touches no dialog item and copies no `$OUTDIR` leaves
        // no evidence of its layout, whatever version built it.
        let data = entry_block(&[
            (EW_CREATEDIR, [1, 1, 0, 0, 0, 0]),
            (EW_EXTRACTFILE, [0, 2, 0, 0, 0, 0]),
            (EW_RET, [0; 6]),
        ]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 3, var_index),
            Nsis2SubVersion::From226
        );
    }

    #[test]
    fn getdlgitem_on_hwndparent_means_pre_226() {
        // `$HWNDPARENT` sat at index 27 before 2.26.
        let data = entry_block(&[(EW_GETDLGITEM, [0, 100, 0, 0, 0, 0])]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 1, var_index),
            Nsis2SubVersion::UpTo225
        );
    }

    #[test]
    fn getdlgitem_writing_spec_outdir_means_pre_204() {
        // Storing the handle in variable 29 — `$_OUTDIR` in that layout — only
        // happens on 2.03 and earlier.
        let data = entry_block(&[(EW_GETDLGITEM, [29, 100, 0, 0, 0, 0])]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 1, var_index),
            Nsis2SubVersion::UpTo203
        );
    }

    #[test]
    fn strcpy_spec_outdir_from_outdir_means_pre_226() {
        // `StrCpy $_OUTDIR $OUTDIR` with no substring parameters.
        let data = entry_block(&[(EW_ASSIGNVAR, [29, 200, 0, 0, 0, 0])]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 1, var_index),
            Nsis2SubVersion::UpTo225
        );
    }

    #[test]
    fn strcpy_with_substring_parameters_is_not_the_marker() {
        // `StrCpy $_OUTDIR $OUTDIR 3 1` copies a slice, not the whole path, so
        // it is not the compiler-generated save.
        let data = entry_block(&[(EW_ASSIGNVAR, [29, 200, 3, 1, 0, 0])]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 1, var_index),
            Nsis2SubVersion::From226
        );
    }

    #[test]
    fn markers_naming_other_variables_are_ignored() {
        // A dialog item fetched into some other variable says nothing: offset
        // 300 is not a bare variable reference at all.
        let data = entry_block(&[
            (EW_GETDLGITEM, [0, 300, 0, 0, 0, 0]),
            (EW_ASSIGNVAR, [5, 200, 0, 0, 0, 0]),
        ]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 2, var_index),
            Nsis2SubVersion::From226
        );
    }

    #[test]
    fn the_strongest_marker_wins_over_a_later_weaker_one() {
        // The 2.03 signal short-circuits, so a later `StrCpy` cannot pull the
        // verdict back up to 2.25.
        let data = entry_block(&[
            (EW_GETDLGITEM, [29, 100, 0, 0, 0, 0]),
            (EW_ASSIGNVAR, [29, 200, 0, 0, 0, 0]),
        ]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 2, var_index),
            Nsis2SubVersion::UpTo203
        );
    }

    #[test]
    fn entry_count_beyond_the_block_stops_the_scan() {
        // A truncated header must not read past the end of the block.
        let data = entry_block(&[(EW_GETDLGITEM, [29, 100, 0, 0, 0, 0])]);
        assert_eq!(
            detect_nsis2_sub_version(&data, 0, 999, var_index),
            Nsis2SubVersion::UpTo203
        );
        assert_eq!(
            detect_nsis2_sub_version(&[], 0, 999, var_index),
            Nsis2SubVersion::From226
        );
    }

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
