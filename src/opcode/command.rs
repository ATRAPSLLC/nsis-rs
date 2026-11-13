//! The script command an entry compiles from.
//!
//! An installer's entries carry opcodes ([`EW_*`](crate::opcode)), not the
//! commands a `.nsi` script writes: `StrCpy` compiles to `EW_ASSIGNVAR`,
//! `SetOutPath` and `CreateDirectory` both to `EW_CREATEDIR`, and a reader of
//! the recovered script wants the command rather than the opcode. This maps
//! one to the other, reading the same operands the compiler wrote.
//!
//! The mapping is NSIS's own, taken from its compiler sources rather than from
//! a description of what each opcode does: `Source/tokens.cpp` lists every
//! command with its `TOK_*`, and `CEXEBuild::doCommand` in `Source/script.cpp`
//! is the switch that turns a `TOK_*` into an entry, setting `which` and the
//! operands this module reads back. Where several commands compile to entries
//! an installer cannot tell apart - the compiler decided it and kept no record
//! - the answer says so rather than choosing silently.

use self::ScriptCommand::{Either, Internal, One};
use crate::opcode::{info::OpcodeInfo, semantics::ExecFlag, version::NsisVersion};

/// What command an entry's opcode and operands came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptCommand {
    /// The entry can only have come from this command.
    One(&'static str),
    /// Several commands compile to an identical entry. The first is the one a
    /// script usually writes; the rest are the alternatives, so a reader is
    /// told what the installer does not record.
    Either(&'static str, &'static [&'static str]),
    /// The opcode is not one a script command writes directly: a compiler
    /// fixup, a reserved slot, or the second entry of a two-entry command.
    Internal,
}

impl ScriptCommand {
    /// The command to show a reader.
    #[must_use]
    pub const fn shown(self) -> Option<&'static str> {
        match self {
            Self::One(name) | Self::Either(name, _) => Some(name),
            Self::Internal => None,
        }
    }

    /// The commands this entry could equally have come from; empty when the
    /// entry names its command exactly.
    #[must_use]
    pub const fn alternatives(self) -> &'static [&'static str] {
        match self {
            Self::Either(_, others) => others,
            Self::One(_) | Self::Internal => &[],
        }
    }
}

/// Returns the script command the entry `values` of the opcode `info`
/// describes compiles from, under `version`.
///
/// Keyed on the opcode's resolved identity (`info.mnemonic`), never on the
/// number stored in the entry. The number means different instructions in
/// different installers and only
/// [`resolve_opcode`](crate::NsisInstaller::resolve_opcode) knows which: 1.x
/// numbers from a table of its own, the Park fork inserts opcodes and shifts
/// the rest, a makensis built with logging shifts everything from
/// `EW_SECTIONSET` up by one, and number 37 is `EW_SETBRANDINGIMAGE` in NSIS 2
/// where it is `EW_LOADANDSETIMAGE` in NSIS 3. Reading the number here would
/// name another opcode's command in every one of those installers, so this
/// takes what the resolver already decided.
///
/// `version` is still needed for the forms an operand chooses between: the
/// rules below are the modern compiler's (`script.cpp`), and 1.x sets its
/// operands differently (see
/// [`param_layout_v1`](crate::opcode::param_layout_v1)), so a 1.x entry whose
/// command turns on an operand is left unnamed rather than guessed.
#[must_use]
pub fn script_command(info: &OpcodeInfo, version: NsisVersion, values: &[i32; 6]) -> ScriptCommand {
    match by_operands(info.mnemonic, values) {
        Some(_) if version == NsisVersion::V1 => ScriptCommand::Internal,
        Some(found) => found,
        None => fixed(info.mnemonic),
    }
}

/// The command of an opcode that carries several, chosen by the operand the
/// compiler set; `None` for an opcode whose command its operands do not
/// decide.
///
/// Every arm of this match ends in an unguarded case for its own mnemonic, so
/// the answer is `Some` for exactly the opcodes whose form an operand picks,
/// whatever the values are - which is what lets a caller tell the two kinds
/// apart without a second list to keep in step.
#[must_use]
#[allow(clippy::too_many_lines)]
fn by_operands(mnemonic: &str, values: &[i32; 6]) -> Option<ScriptCommand> {
    let [v0, v1, v2, v3, v4, v5] = *values;
    Some(match mnemonic {
        // A jump keeps its target in the first operand; a true no-op has none.
        "EW_NOP" if v0 != 0 => One("Goto"),
        "EW_NOP" => One("Nop"),
        // The directory becomes `$OUTDIR` only for `SetOutPath`.
        "EW_CREATEDIR" if v1 != 0 => One("SetOutPath"),
        "EW_CREATEDIR" => One("CreateDirectory"),
        // A flag instruction names its flag by index.
        "EW_SETFLAG" => match flag_of(v0) {
            Some(ExecFlag::Silent) => One("SetSilent"),
            Some(ExecFlag::AlterRegView) => One("SetRegView"),
            Some(ExecFlag::AllUserVar) => One("SetShellVarContext"),
            Some(ExecFlag::StatusUpdate) => One("SetDetailsPrint"),
            Some(ExecFlag::AutoClose) => One("SetAutoClose"),
            Some(ExecFlag::ErrorLevel) => One("SetErrorLevel"),
            Some(ExecFlag::ExecReboot) => One("SetRebootFlag"),
            // The error flag is cleared or set by the operand's value.
            Some(ExecFlag::ExecError) if v1 == 0 => One("ClearErrors"),
            Some(ExecFlag::ExecError) => One("SetErrors"),
            _ => Internal,
        },
        "EW_IFFLAG" => match flag_of(v0) {
            Some(ExecFlag::Silent) => One("IfSilent"),
            Some(ExecFlag::Rtl) => One("IfRtlLanguage"),
            Some(ExecFlag::AlterRegView) => One("IfAltRegView"),
            Some(ExecFlag::AllUserVar) => One("IfShellVarContextAll"),
            Some(ExecFlag::ExecError) => One("IfErrors"),
            Some(ExecFlag::Abort) => One("IfAbort"),
            Some(ExecFlag::ExecReboot) => One("IfRebootFlag"),
            _ => Internal,
        },
        "EW_GETFLAG" => match flag_of(v0) {
            Some(ExecFlag::InstdirError) => One("GetInstDirError"),
            Some(ExecFlag::AlterRegView) => One("GetRegView"),
            Some(ExecFlag::AllUserVar) => One("GetShellVarContext"),
            Some(ExecFlag::ErrorLevel) => One("GetErrorLevel"),
            _ => Internal,
        },
        "EW_STRCMP" if v4 != 0 => One("StrCmpS"),
        "EW_STRCMP" => One("StrCmp"),
        "EW_READENVSTR" if v2 != 0 => One("ReadEnvStr"),
        "EW_READENVSTR" => One("ExpandEnvStrings"),
        // The comparison's width and signedness are bits of the last operand.
        "EW_INTCMP" => match (v5 & 0x8000 != 0, v5 & 0x1 != 0) {
            (true, true) => Either("Int64CmpU", &["IntPtrCmpU"]),
            (true, false) => Either("Int64Cmp", &["IntPtrCmp"]),
            (false, true) => One("IntCmpU"),
            (false, false) => One("IntCmp"),
        },
        "EW_INTFMT" if v3 != 0 => One("Int64Fmt"),
        "EW_INTFMT" => One("IntFmt"),
        // Push, Pop and Exch are one opcode: the second operand pops and the
        // third exchanges at a depth.
        "EW_PUSHPOP" if v2 != 0 => One("Exch"),
        "EW_PUSHPOP" if v1 != 0 => One("Pop"),
        "EW_PUSHPOP" => One("Push"),
        // The branding image has no output variable; the general form does.
        "EW_LOADANDSETIMAGE" if v0 == -1 => One("SetBrandingImage"),
        "EW_LOADANDSETIMAGE" => One("LoadAndSetImage"),
        "EW_SHOWWINDOW" if v3 != 0 => One("EnableWindow"),
        "EW_SHOWWINDOW" => Either("ShowWindow", &["HideWindow", "BringToFront"]),
        // Waiting is a mask bit the compiler added.
        "EW_SHELLEXEC" if v4 & 0x40 != 0 => One("ExecShellWait"),
        "EW_SHELLEXEC" => One("ExecShell"),
        "EW_EXECUTE" if v2 != 0 => One("ExecWait"),
        "EW_EXECUTE" => One("Exec"),
        // One opcode writes an INI value, deletes one, deletes a section and
        // flushes the file, told apart by which operands are set.
        "EW_WRITEINI" if v4 != 0 => One("WriteINIStr"),
        "EW_WRITEINI" if v1 == 0 && v3 != 0 => One("DeleteINISec"),
        "EW_WRITEINI" if v1 != 0 => One("DeleteINIStr"),
        "EW_WRITEINI" => One("FlushINI"),
        // The last operand's low bit says key rather than value.
        "EW_DELREG" if v4 & 0x1 != 0 => One("DeleteRegKey"),
        "EW_DELREG" => One("DeleteRegValue"),
        // The registry type the compiler wrote decides the command.
        "EW_WRITEREG" => match v5 {
            1 => One("WriteRegStr"),
            2 => One("WriteRegExpandStr"),
            3 => One("WriteRegBin"),
            4 => One("WriteRegDWORD"),
            0 => One("WriteRegNone"),
            7 => One("WriteRegMultiStr"),
            _ => One("WriteReg"),
        },
        "EW_READREGSTR" if v4 != 0 => One("ReadRegDWORD"),
        "EW_READREGSTR" => One("ReadRegStr"),
        "EW_REGENUM" if v4 != 0 => One("EnumRegKey"),
        "EW_REGENUM" => One("EnumRegValue"),
        "EW_FPUTS" if v2 != 0 => One("FileWriteByte"),
        "EW_FPUTS" => One("FileWrite"),
        "EW_FGETS" if v3 != 0 => One("FileReadByte"),
        "EW_FGETS" => One("FileRead"),
        "EW_LOG" if v0 != 0 => One("LogSet"),
        "EW_LOG" => One("LogText"),
        // A section field is set when the encoding is negative and read when
        // it is not; which field is the encoded value itself.
        "EW_SECTIONSET" if v1 != 0 => One("SectionGetText"),
        "EW_SECTIONSET" if v3 != 0 => One("SectionSetFlags"),
        "EW_SECTIONSET" if v2 < 0 => {
            Either("SectionSetText", &["SectionSetInstTypes", "SectionSetSize"])
        }
        "EW_SECTIONSET" => Either(
            "SectionGetFlags",
            &["SectionGetInstTypes", "SectionGetSize"],
        ),
        "EW_INSTTYPESET" if v3 != 0 && v2 != 0 => One("SetCurInstType"),
        "EW_INSTTYPESET" if v3 != 0 => One("GetCurInstType"),
        "EW_INSTTYPESET" if v2 != 0 => One("InstTypeSetText"),
        "EW_INSTTYPESET" => One("InstTypeGetText"),
        "EW_GETOSINFO" if v3 == 0 => One("GetKnownFolderPath"),
        "EW_GETOSINFO" => Either("ReadMemory", &["GetWinVer"]),
        // The Unicode builds' file words, told apart as their byte forms are.
        "EW_FPUTWS" if v2 != 0 => One("FileWriteWord"),
        "EW_FPUTWS" => One("FileWriteUTF16LE"),
        "EW_FGETWS" if v3 != 0 => One("FileReadWord"),
        "EW_FGETWS" => One("FileReadUTF16LE"),
        _ => return None,
    })
}

/// The command of an opcode that carries one whatever its operands say, and
/// [`ScriptCommand::Internal`] for an opcode no command writes.
#[must_use]
fn fixed(mnemonic: &str) -> ScriptCommand {
    match mnemonic {
        "EW_INVALID_OPCODE" | "EW_RESERVEDOPCODE" => Internal,
        // `Return` is also what a section's and a function's end emit.
        "EW_RET" => Either("Return", &["SectionEnd", "FunctionEnd"]),
        "EW_ABORT" => One("Abort"),
        // `Reboot` writes `EW_REBOOT` and then this; a script's own `Quit` is
        // the same entry with no reboot before it, which an entry alone
        // cannot distinguish.
        "EW_QUIT" => Either("Quit", &["Reboot"]),
        "EW_CALL" => One("Call"),
        "EW_UPDATETEXT" => One("DetailPrint"),
        "EW_SLEEP" => One("Sleep"),
        "EW_BRINGTOFRONT" => One("BringToFront"),
        "EW_CHDETAILSVIEW" => One("SetDetailsView"),
        "EW_SETFILEATTRIBUTES" => One("SetFileAttributes"),
        "EW_IFFILEEXISTS" => One("IfFileExists"),
        "EW_RENAME" => One("Rename"),
        "EW_GETFULLPATHNAME" => One("GetFullPathName"),
        "EW_SEARCHPATH" => One("SearchPath"),
        "EW_GETTEMPFILENAME" => One("GetTempFileName"),
        // `ReserveFile` extracts at the same entry; only the compiler knew.
        "EW_EXTRACTFILE" => Either("File", &["ReserveFile"]),
        "EW_DELETEFILE" => One("Delete"),
        "EW_MESSAGEBOX" => One("MessageBox"),
        "EW_RMDIR" => One("RMDir"),
        "EW_STRLEN" => One("StrLen"),
        // Several commands compile to an assignment: the compiler resolved an
        // address or a version into the operand and left the entry identical.
        "EW_ASSIGNVAR" => Either(
            "StrCpy",
            &[
                "UnsafeStrCpy",
                "GetCurrentAddress",
                "GetFunctionAddress",
                "GetLabelAddress",
                "GetDLLVersionLocal",
                "GetFileTimeLocal",
            ],
        ),
        "EW_INTOP" => Either("IntOp", &["IntPtrOp"]),
        "EW_FINDWINDOW" => One("FindWindow"),
        "EW_SENDMESSAGE" => One("SendMessage"),
        "EW_ISWINDOW" => One("IsWindow"),
        "EW_GETDLGITEM" => One("GetDlgItem"),
        "EW_SETCTLCOLORS" => One("SetCtlColors"),
        // NSIS 2 and the Park fork store this where NSIS 3 stores
        // `EW_LOADANDSETIMAGE`: one command, with no general form to choose.
        "EW_SETBRANDINGIMAGE" => One("SetBrandingImage"),
        "EW_CREATEFONT" => One("CreateFont"),
        "EW_GETFILETIME" => One("GetFileTime"),
        "EW_GETDLLVERSION" => One("GetDLLVersion"),
        // Which of the three wrote it is the function name in the operand,
        // which is a string the entry only points at.
        "EW_REGISTERDLL" => Either("RegDLL", &["UnRegDLL", "CallInstDLL"]),
        "EW_CREATESHORTCUT" => One("CreateShortcut"),
        "EW_COPYFILES" => One("CopyFiles"),
        "EW_REBOOT" => One("Reboot"),
        "EW_READINISTR" => One("ReadINIStr"),
        "EW_FCLOSE" => One("FileClose"),
        "EW_FOPEN" => One("FileOpen"),
        "EW_FSEEK" => One("FileSeek"),
        "EW_FINDCLOSE" => One("FindClose"),
        "EW_FINDNEXT" => One("FindNext"),
        "EW_FINDFIRST" => One("FindFirst"),
        "EW_WRITEUNINSTALLER" => One("WriteUninstaller"),
        "EW_LOCKWINDOW" => One("LockWindow"),
        _ => Internal,
    }
}

/// The flag an instruction's first operand names, when it names one.
fn flag_of(value: i32) -> Option<ExecFlag> {
    u8::try_from(value)
        .ok()
        .and_then(|index| ExecFlag::ALL.get(usize::from(index)).copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcode::{
        info::{OPCODES, SET_BRANDING_IMAGE},
        lookup_for,
    };

    /// Every operand pattern the forms below turn on: all zero, each slot set
    /// on its own, each slot negative, and the two mask bits
    /// (`EW_INTCMP`'s width, `EW_SHELLEXEC`'s wait) that no single bit reaches.
    fn probes() -> Vec<[i32; 6]> {
        let mut out = vec![[0; 6], [1; 6], [-1; 6]];
        for slot in 0..6 {
            for value in [1, -1, 0x8000, 0x40, 3, 7] {
                let mut values = [0; 6];
                values[slot] = value;
                out.push(values);
            }
        }
        out
    }

    /// The opcode description of `mnemonic`, from the table the resolver reads.
    fn info_of(mnemonic: &str) -> &'static OpcodeInfo {
        if mnemonic == SET_BRANDING_IMAGE.mnemonic {
            return &SET_BRANDING_IMAGE;
        }
        OPCODES
            .iter()
            .find(|info| info.mnemonic == mnemonic)
            .unwrap_or_else(|| panic!("{mnemonic} is in the opcode table"))
    }

    fn command(mnemonic: &str, values: [i32; 6]) -> ScriptCommand {
        script_command(info_of(mnemonic), NsisVersion::V3, &values)
    }

    /// **A form-selecting opcode names the command its operands chose.** The
    /// same opcode writes `SetOutPath` or `CreateDirectory`, reads a string or
    /// a DWORD from the registry, and pushes, pops or exchanges - each decided
    /// by an operand the compiler set, which is what `exec.c` branches on.
    #[test]
    fn a_form_is_named_by_the_operands_the_compiler_set() {
        assert_eq!(
            command("EW_CREATEDIR", [0, 1, 0, 0, 0, 0]).shown(),
            Some("SetOutPath")
        );
        assert_eq!(
            command("EW_CREATEDIR", [0, 0, 0, 0, 0, 0]).shown(),
            Some("CreateDirectory")
        );
        assert_eq!(
            command("EW_READREGSTR", [0, 0, 0, 0, 1, 0]).shown(),
            Some("ReadRegDWORD")
        );
        assert_eq!(
            command("EW_READREGSTR", [0, 0, 0, 0, 0, 0]).shown(),
            Some("ReadRegStr")
        );
        for (values, expected) in [
            ([0, 0, 0, 0, 0, 0], "Push"),
            ([0, 1, 0, 0, 0, 0], "Pop"),
            ([0, 0, 3, 0, 0, 0], "Exch"),
        ] {
            assert_eq!(command("EW_PUSHPOP", values).shown(), Some(expected));
        }
        assert_eq!(
            command("EW_EXECUTE", [0, 0, 1, 0, 0, 0]).shown(),
            Some("ExecWait")
        );
        assert_eq!(command("EW_EXECUTE", [0; 6]).shown(), Some("Exec"));
    }

    /// **An entry that cannot name its command says so.** The compiler
    /// resolved `GetFunctionAddress` into an assignment and kept no record, so
    /// the answer shows `StrCpy` and lists what else it could have been; a
    /// reserved opcode is no command at all.
    #[test]
    fn an_entry_that_names_no_single_command_lists_the_alternatives() {
        let assign = command("EW_ASSIGNVAR", [0; 6]);
        assert_eq!(assign.shown(), Some("StrCpy"));
        assert!(assign.alternatives().contains(&"GetFunctionAddress"));
        assert!(assign.alternatives().contains(&"GetDLLVersionLocal"));
        let reserved = command("EW_RESERVEDOPCODE", [0; 6]);
        assert_eq!(reserved.shown(), None);
        assert!(reserved.alternatives().is_empty());
    }

    /// **A flag instruction names the command of the flag it carries.** One
    /// opcode sets silence, the registry view, the shell context and the error
    /// flag, and the error flag's own two commands differ by the value set.
    #[test]
    fn a_flag_instruction_names_its_flags_command() {
        let set = |flag: ExecFlag, value: i32| {
            command("EW_SETFLAG", [flag as i32, value, 0, 0, 0, 0]).shown()
        };
        assert_eq!(set(ExecFlag::Silent, 1), Some("SetSilent"));
        assert_eq!(set(ExecFlag::AlterRegView, 0), Some("SetRegView"));
        assert_eq!(set(ExecFlag::ExecError, 0), Some("ClearErrors"));
        assert_eq!(set(ExecFlag::ExecError, 1), Some("SetErrors"));
        assert_eq!(
            command("EW_IFFLAG", [ExecFlag::Abort as i32, 0, 0, 0, 0, 0]).shown(),
            Some("IfAbort")
        );
    }

    /// **The command follows the opcode the resolver named, not the number in
    /// the entry.** Number 37 is `EW_SETBRANDINGIMAGE` in an NSIS 2 or Park
    /// installer and `EW_LOADANDSETIMAGE` in an NSIS 3 one, and the two carry
    /// different commands - `LoadAndSetImage` does not exist in NSIS 2. The
    /// same number reaching two answers is the whole point: a lookup keyed on
    /// the number could only ever give one of them, and would give it to 1.x,
    /// to the Park fork (whose inserted opcodes shift the rest) and to a
    /// logging build (which shifts everything from `EW_SECTIONSET` up) as
    /// well. This function cannot be handed a number at all, which is how
    /// that whole family is closed; the test pins the one case the tables can
    /// show on their own.
    #[test]
    fn a_command_follows_the_resolved_opcode_not_its_number() {
        let number = 37;
        let two = lookup_for(number, NsisVersion::V2).expect("NSIS 2 resolves 37");
        let three = lookup_for(number, NsisVersion::V3).expect("NSIS 3 resolves 37");
        assert_eq!(two.mnemonic, "EW_SETBRANDINGIMAGE");
        assert_eq!(three.mnemonic, "EW_LOADANDSETIMAGE");
        assert_eq!(
            script_command(two, NsisVersion::V2, &[0; 6]).shown(),
            Some("SetBrandingImage")
        );
        assert_eq!(
            script_command(three, NsisVersion::V3, &[0; 6]).shown(),
            Some("LoadAndSetImage"),
            "the control: the same number under NSIS 3 is another command"
        );
    }

    /// **1.x is never guessed at, and never mis-named.** NSIS 1.x sets the
    /// operands that choose between an opcode's forms by its own rules
    /// (`param_layout_v1`), so a form-selecting opcode is left unnamed for it
    /// rather than read under the modern compiler's rules - for every opcode
    /// of that kind, and whatever its operands say. An opcode that carries
    /// one command whatever its operands is named as it is everywhere else:
    /// that is the control, so "unnamed" cannot be passing by refusing
    /// everything.
    #[test]
    fn a_1_x_entry_is_named_only_where_no_operand_decides() {
        let mut unnamed = 0;
        let mut named = 0;
        for info in OPCODES.iter().chain(std::iter::once(&SET_BRANDING_IMAGE)) {
            let decided = by_operands(info.mnemonic, &[0; 6]).is_some();
            for values in probes() {
                let legacy = script_command(info, NsisVersion::V1, &values);
                if decided {
                    assert_eq!(
                        legacy.shown(),
                        None,
                        "{} turns on an operand, so 1.x cannot name it",
                        info.mnemonic
                    );
                } else {
                    assert_eq!(
                        legacy,
                        fixed(info.mnemonic),
                        "{} carries one command, which 1.x writes too",
                        info.mnemonic
                    );
                }
            }
            if decided {
                unnamed += 1;
            } else {
                named += 1;
            }
        }
        assert!(unnamed >= 25, "the form-selecting opcodes: {unnamed}");
        assert!(named >= 40, "the single-command opcodes: {named}");
    }
}
