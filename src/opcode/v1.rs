//! The NSIS 1.x opcode table.
//!
//! NSIS 1.x numbers its instructions differently from every later version: it
//! has instructions 2.x dropped (`EW_SETSFCONTEXT`, `EW_IFREBOOTFLAG`), lacks
//! ones 2.x added, and orders the parameters of several others differently -
//! `FileOpen` takes its output handle last where 2.x takes it first,
//! `GetFileTime` takes its file first where 2.x takes it last. Reading a 1.x
//! entry through the modern table therefore does not merely rename operands,
//! it reads the wrong ones.
//!
//! # Provenance
//!
//! The numbering is the enum in `Source/exehead/fileform.h` of the NSIS 1.98
//! source, compiled with the defines a released makensis 1.98 reports under
//! `/HDRINFO`. Each instruction's parameters are what `Source/exehead/exec.c`
//! does with them, which is more reliable than the enum's comments: those
//! disagree with the code for `FindFirst`, `GetFullPathName` and
//! `SendMessage`.
//!
//! # Builds that renumber this table
//!
//! Nearly every instruction sits inside an `#ifdef`, so a makensis built with
//! features compiled out numbers the rest differently. This table is the
//! released configuration, which is what installers in the wild were built
//! with.

use crate::opcode::{
    info::{
        OpcodeInfo, ParamLayout, ParamType,
        ParamType::{DataOffset, Int, Jump, Number, String, Unused, Variable},
        pushpop_form,
    },
    semantics::{Access, Effects, ExecFlag, FlagSet, HiddenVariables, StackEffect, Termination},
};

/// A variable the instruction always writes.
const OUT: ParamType = Variable(Access::Write);
/// A variable the instruction writes only on some runs.
const MAY_OUT: ParamType = Variable(Access::MayWrite);
/// A variable the instruction reads.
const IN: ParamType = Variable(Access::Read);

/// 1.x keeps the error flag in a global of its own, `exec_errorflag`.
const ERROR: FlagSet = FlagSet::of(&[ExecFlag::ExecError]);
/// And the reboot flag in `exec_rebootflag`.
const REBOOT: FlagSet = FlagSet::of(&[ExecFlag::ExecReboot]);

/// No effect beyond the slots.
const NONE: Effects = Effects::NONE;
/// Sets the error flag when it fails.
const FAILS: Effects = Effects::NONE.failing();
/// Sets the error flag when it fails, and reports a status line: 1.x keeps
/// `SetDetailsPrint`'s choice in `ui_st_updateflag`.
const FAILS_REPORTING: Effects = Effects::NONE.failing().reporting();
/// An opcode the runtime has no case for: it reports a corrupted installer and
/// stops.
const CORRUPT: Effects = Effects::NONE.with_termination(Termination::Always);
/// `File`: relative to `$OUTDIR`, and on a write error it expands the
/// header's file-error text - a string the script chose, which may embed any
/// variable - with `$0` holding the path.
const EXTRACT: Effects = Effects {
    hidden: HiddenVariables::ReadsAny,
    ..FAILS_REPORTING
        .with_outdir(Access::Read)
        .with_termination(Termination::May)
};
/// `WriteUninstaller`: a relative name is placed in `$INSTDIR`.
const UNINSTALLER: Effects = Effects {
    instdir: Some(Access::Read),
    ..Effects::NONE.reporting()
};

/// The NSIS 1.x opcode table, indexed by an entry's `which` field.
pub static OPCODES_V1: [OpcodeInfo; 66] = [
    OpcodeInfo {
        mnemonic: "EW_INVALID_OPCODE",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: CORRUPT,
        description: "Invalid; a zeroed instruction",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_RET",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Return from a function call",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_NOP",
        param_count: 1,
        param_names: ["jump_addr", "", "", "", "", ""],
        param_types: [Jump, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Goto/Nop",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_ABORT",
        param_count: 1,
        param_names: ["status_text", "", "", "", "", ""],
        param_types: [String, Unused, Unused, Unused, Unused, Unused],
        effects: NONE
            .reporting()
            .with_termination(Termination::Always)
            .acting_outside(),
        description: "Abort",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_QUIT",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.with_termination(Termination::Always),
        description: "Quit",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_CALL",
        param_count: 2,
        param_names: ["address", "label_call", "", "", "", ""],
        param_types: [Jump, Int, Unused, Unused, Unused, Unused],
        effects: NONE.with_termination(Termination::May),
        description: "Call",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_UPDATETEXT",
        param_count: 2,
        param_names: ["text", "set_status", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: NONE.reporting().acting_outside(),
        description: "DetailPrint/status text",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SLEEP",
        param_count: 1,
        param_names: ["milliseconds", "", "", "", "", ""],
        param_types: [Number, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "Sleep",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_SETSFCONTEXT",
        param_count: 1,
        param_names: ["all_users", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.writing(FlagSet::of(&[ExecFlag::AllUserVar])),
        description: "SetShellVarContext",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_HIDEWINDOW",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "HideWindow",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_BRINGTOFRONT",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "BringToFront",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SETWINDOWCLOSE",
        param_count: 1,
        param_names: ["close_on_end", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.writing(FlagSet::of(&[ExecFlag::AutoClose])),
        description: "SetAutoClose",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_CHDETAILSVIEW",
        param_count: 2,
        param_names: ["list_action", "button_action", "", "", "", ""],
        param_types: [Int, Int, Unused, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "SetDetailsView",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SETFILEATTRIBUTES",
        param_count: 2,
        param_names: ["file", "attributes", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "SetFileAttributes",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATEDIR",
        param_count: 2,
        param_names: ["path", "set_outdir", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: NONE.reporting().acting_outside(),
        description: "CreateDirectory/SetOutPath",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_IFFILEEXISTS",
        param_count: 3,
        param_names: ["file", "jump_yes", "jump_no", "", "", ""],
        param_types: [String, Jump, Jump, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "IfFileExists",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_IFERRORS",
        param_count: 3,
        param_names: ["jump_error", "jump_no_error", "new_error_flag", "", "", ""],
        param_types: [Jump, Jump, Int, Unused, Unused, Unused],
        effects: NONE.reading(ERROR).writing(ERROR),
        description: "IfErrors/ClearErrors",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_RENAME",
        param_count: 3,
        param_names: ["old", "new", "rebootok", "", "", ""],
        param_types: [String, String, Int, Unused, Unused, Unused],
        effects: FAILS_REPORTING.acting_outside(),
        description: "Rename",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFULLPATHNAME",
        param_count: 3,
        param_names: ["output", "input", "long_name", "", "", ""],
        param_types: [OUT, String, Int, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "GetFullPathName",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_SEARCHPATH",
        param_count: 2,
        param_names: ["output", "filename", "", "", "", ""],
        param_types: [OUT, String, Unused, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "SearchPath",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETTEMPFILENAME",
        param_count: 1,
        param_names: ["output", "", "", "", "", ""],
        param_types: [OUT, Unused, Unused, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "GetTempFileName",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_EXTRACTFILE",
        param_count: 5,
        param_names: ["overwrite", "name", "data_offset", "date_lo", "date_hi", ""],
        param_types: [Int, String, DataOffset, Int, Int, Unused],
        effects: EXTRACT.acting_outside(),
        description: "File",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_DELETEFILE",
        param_count: 2,
        param_names: ["filename", "rebootok", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: FAILS_REPORTING.acting_outside(),
        description: "Delete",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_MESSAGEBOX",
        param_count: 5,
        param_names: ["mb_flags", "text", "buttons", "jump1", "jump2", ""],
        param_types: [Int, String, Int, Jump, Jump, Unused],
        effects: FAILS.acting_outside(),
        description: "MessageBox",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_RMDIR",
        param_count: 2,
        param_names: ["path", "recursive", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: FAILS_REPORTING.acting_outside(),
        description: "RMDir",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_STRLEN",
        param_count: 2,
        param_names: ["output", "input", "", "", "", ""],
        param_types: [OUT, String, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "StrLen",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_ASSIGNVAR",
        param_count: 4,
        param_names: ["var", "string", "max_len", "start", "", ""],
        param_types: [OUT, String, Number, Number, Unused, Unused],
        effects: NONE,
        description: "StrCpy",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_STRCMP",
        param_count: 4,
        param_names: ["s1", "s2", "jump_eq", "jump_neq", "", ""],
        param_types: [String, String, Jump, Jump, Unused, Unused],
        effects: NONE,
        description: "StrCmp",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_READENVSTR",
        param_count: 3,
        param_names: ["output", "string", "is_read", "", "", ""],
        param_types: [OUT, String, Int, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "ReadEnvStr/ExpandEnvStrings",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_INTCMP",
        param_count: 5,
        param_names: ["v1", "v2", "jump_eq", "jump_lt", "jump_gt", ""],
        param_types: [Number, Number, Jump, Jump, Jump, Unused],
        effects: NONE,
        description: "IntCmp",
        category: "int",
    },
    OpcodeInfo {
        mnemonic: "EW_INTCMPU",
        param_count: 5,
        param_names: ["v1", "v2", "jump_eq", "jump_lt", "jump_gt", ""],
        param_types: [Number, Number, Jump, Jump, Jump, Unused],
        effects: NONE,
        description: "IntCmpU",
        category: "int",
    },
    OpcodeInfo {
        mnemonic: "EW_INTOP",
        param_count: 4,
        param_names: ["output", "input1", "input2", "op", "", ""],
        param_types: [OUT, Number, Number, Int, Unused, Unused],
        effects: NONE,
        description: "IntOp",
        category: "int",
    },
    OpcodeInfo {
        mnemonic: "EW_INTFMT",
        param_count: 3,
        param_names: ["output", "format", "input", "", "", ""],
        param_types: [OUT, String, Number, Unused, Unused, Unused],
        effects: NONE,
        description: "IntFmt",
        category: "int",
    },
    OpcodeInfo {
        mnemonic: "EW_PUSHPOP",
        param_count: 3,
        param_names: ["value", "pop", "exch", "", "", ""],
        param_types: [String, Int, Int, Unused, Unused, Unused],
        effects: NONE.with_stack(StackEffect::Push),
        description: "Push/Pop/Exch",
        category: "stack",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDWINDOW",
        param_count: 5,
        param_names: ["output", "class", "title", "parent", "after", ""],
        param_types: [OUT, String, String, Number, Number, Unused],
        effects: NONE.acting_outside(),
        description: "FindWindow",
        category: "window",
    },
    OpcodeInfo {
        mnemonic: "EW_SENDMESSAGE",
        param_count: 5,
        param_names: ["output", "hwnd", "msg", "wparam", "lparam", ""],
        param_types: [OUT, Number, Number, Number, Number, Unused],
        effects: NONE.acting_outside(),
        description: "SendMessage",
        category: "window",
    },
    OpcodeInfo {
        mnemonic: "EW_ISWINDOW",
        param_count: 3,
        param_names: ["hwnd", "jump_yes", "jump_no", "", "", ""],
        param_types: [Number, Jump, Jump, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "IsWindow",
        category: "window",
    },
    OpcodeInfo {
        mnemonic: "EW_SHELLEXEC",
        param_count: 4,
        param_names: ["verb", "file", "params", "showwindow", "", ""],
        param_types: [String, String, String, Int, Unused, Unused],
        effects: FAILS_REPORTING.with_outdir(Access::Read).acting_outside(),
        description: "ExecShell",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_EXECUTE",
        param_count: 3,
        param_names: ["command", "wait", "exit_code", "", "", ""],
        param_types: [String, Int, MAY_OUT, Unused, Unused, Unused],
        effects: FAILS_REPORTING.with_outdir(Access::Read).acting_outside(),
        description: "Exec/ExecWait",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFILETIME",
        param_count: 3,
        param_names: ["file", "high_out", "low_out", "", "", ""],
        param_types: [String, OUT, OUT, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "GetFileTime",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETDLLVERSION",
        param_count: 3,
        param_names: ["file", "high_out", "low_out", "", "", ""],
        param_types: [String, OUT, OUT, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "GetDLLVersion",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_REGISTERDLL",
        param_count: 3,
        param_names: ["dll", "function", "status_text", "", "", ""],
        param_types: [String, String, String, Unused, Unused, Unused],
        effects: FAILS_REPORTING.acting_outside(),
        description: "RegDLL/UnRegDLL",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATESHORTCUT",
        param_count: 5,
        param_names: ["link", "target", "params", "icon", "packed_cs", ""],
        param_types: [String, String, String, String, Int, Unused],
        effects: FAILS_REPORTING.with_outdir(Access::Read).acting_outside(),
        description: "CreateShortCut",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_COPYFILES",
        param_count: 3,
        param_names: ["source", "dest", "flags", "", "", ""],
        param_types: [String, String, Int, Unused, Unused, Unused],
        effects: FAILS_REPORTING.acting_outside(),
        description: "CopyFiles",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_REBOOT",
        param_count: 1,
        param_names: ["magic", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        effects: FAILS.with_termination(Termination::May).acting_outside(),
        description: "Reboot",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_IFREBOOTFLAG",
        param_count: 2,
        param_names: ["jump_set", "jump_unset", "", "", "", ""],
        param_types: [Jump, Jump, Unused, Unused, Unused, Unused],
        effects: NONE.reading(REBOOT),
        description: "IfRebootFlag",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_SETREBOOTFLAG",
        param_count: 1,
        param_names: ["value", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.writing(REBOOT),
        description: "SetRebootFlag",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEINI",
        param_count: 4,
        param_names: ["section", "name", "value", "ini_file", "", ""],
        param_types: [String, String, String, String, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "WriteINIStr",
        category: "ini",
    },
    OpcodeInfo {
        mnemonic: "EW_READINISTR",
        param_count: 4,
        param_names: ["output", "section", "name", "ini_file", "", ""],
        param_types: [OUT, String, String, String, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "ReadINIStr",
        category: "ini",
    },
    OpcodeInfo {
        mnemonic: "EW_DELREG",
        param_count: 4,
        param_names: ["root", "keyname", "valuename", "only_if_empty", "", ""],
        param_types: [Int, String, String, Int, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "DeleteRegValue/DeleteRegKey",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEREG",
        param_count: 5,
        param_names: ["root", "keyname", "itemname", "data", "kind", ""],
        param_types: [Int, String, String, String, Int, Unused],
        effects: FAILS.acting_outside(),
        description: "WriteReg*",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_READREGSTR",
        param_count: 5,
        param_names: ["output", "root", "keyname", "itemname", "want_dword", ""],
        param_types: [OUT, Int, String, String, Int, Unused],
        effects: FAILS.acting_outside(),
        description: "ReadRegStr/ReadRegDWORD",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_REGENUM",
        param_count: 5,
        param_names: ["output", "root", "keyname", "index", "enum_keys", ""],
        param_types: [OUT, Int, String, Number, Int, Unused],
        effects: FAILS.acting_outside(),
        description: "EnumRegKey/EnumRegValue",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_FCLOSE",
        param_count: 1,
        param_names: ["handle", "", "", "", "", ""],
        param_types: [IN, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "FileClose",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_FOPEN",
        param_count: 4,
        param_names: ["name", "access", "disposition", "handle_out", "", ""],
        param_types: [String, Int, Int, OUT, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "FileOpen",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_FPUTS",
        param_count: 3,
        param_names: ["handle", "string", "write_char", "", "", ""],
        param_types: [IN, String, Int, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "FileWrite/FileWriteByte",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_FGETS",
        param_count: 4,
        param_names: ["handle", "output", "max_len", "read_char", "", ""],
        param_types: [IN, MAY_OUT, Number, Int, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "FileRead/FileReadByte",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_FSEEK",
        param_count: 4,
        param_names: ["handle", "offset", "method", "position", "", ""],
        param_types: [IN, Number, Int, MAY_OUT, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "FileSeek",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDCLOSE",
        param_count: 1,
        param_names: ["handle", "", "", "", "", ""],
        param_types: [IN, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "FindClose",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDNEXT",
        param_count: 2,
        param_names: ["output", "handle", "", "", "", ""],
        param_types: [OUT, IN, Unused, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "FindNext",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDFIRST",
        param_count: 3,
        param_names: ["filespec", "output", "handle_out", "", "", ""],
        param_types: [String, OUT, OUT, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "FindFirst",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEUNINSTALLER",
        param_count: 1,
        param_names: ["name", "", "", "", "", ""],
        param_types: [String, Unused, Unused, Unused, Unused, Unused],
        effects: UNINSTALLER.acting_outside(),
        description: "WriteUninstaller",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_LOG",
        param_count: 2,
        param_names: ["type", "text", "", "", "", ""],
        param_types: [Int, String, Unused, Unused, Unused, Unused],
        effects: NONE.acting_outside(),
        description: "LogText/LogSet",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_SECTIONSET",
        param_count: 3,
        param_names: ["section", "op", "value", "", "", ""],
        param_types: [Number, Int, String, Unused, Unused, Unused],
        effects: FAILS.acting_outside(),
        description: "SectionSet/GetText, SectionSet/GetFlags",
        category: "section",
    },
    OpcodeInfo {
        mnemonic: "EW_GETLABELADDR",
        param_count: 2,
        param_names: ["output", "address", "", "", "", ""],
        param_types: [OUT, Jump, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "GetLabelAddress (compiled to StrCpy)",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFUNCTIONADDR",
        param_count: 2,
        param_names: ["output", "address", "", "", "", ""],
        param_types: [OUT, Jump, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "GetFunctionAddress (compiled to StrCpy)",
        category: "flow",
    },
];

/// Looks up an NSIS 1.x opcode.
///
/// Returns `None` for an opcode outside the table, and for the two slots the
/// 1.x compiler resolves before writing the file - `EW_GETLABELADDR` and
/// `EW_GETFUNCTIONADDR` both become `EW_ASSIGNVAR`, so an entry claiming to be
/// one means the block is not a 1.x entry block.
pub fn lookup_v1(which: u32) -> Option<&'static OpcodeInfo> {
    if which >= EW_V1_GETLABELADDR {
        return None;
    }
    OPCODES_V1.get(which as usize)
}

/// The first opcode slot no NSIS 1.x installer stores.
pub const EW_V1_GETLABELADDR: u32 = 64;

/// Maps an NSIS 1.x opcode onto the canonical numbering this crate reports.
///
/// `-1` marks an instruction NSIS 2.0 dropped. Those were folded into the
/// general flag instructions - `IfErrors` and `IfRebootFlag` both became
/// `EW_IFFLAG`, `SetShellVarContext` and `SetRebootFlag` became `EW_SETFLAG`,
/// and `IntCmpU` became `EW_INTCMP` with a flag - so there is no one opcode to
/// map them to.
///
/// Only the identity is canonicalised. Several instructions kept the name and
/// changed the parameter order, so an entry's operands must still be read
/// through [`lookup_v1`].
static V1_TO_CANONICAL: [i32; 66] = [
    0,  //  0 EW_INVALID_OPCODE
    1,  //  1 EW_RET
    2,  //  2 EW_NOP
    3,  //  3 EW_ABORT
    4,  //  4 EW_QUIT
    5,  //  5 EW_CALL
    6,  //  6 EW_UPDATETEXT
    7,  //  7 EW_SLEEP
    -1, //  8 EW_SETSFCONTEXT
    -1, //  9 EW_HIDEWINDOW
    8,  // 10 EW_BRINGTOFRONT
    -1, // 11 EW_SETWINDOWCLOSE
    9,  // 12 EW_CHDETAILSVIEW
    10, // 13 EW_SETFILEATTRIBUTES
    11, // 14 EW_CREATEDIR
    12, // 15 EW_IFFILEEXISTS
    -1, // 16 EW_IFERRORS
    16, // 17 EW_RENAME
    17, // 18 EW_GETFULLPATHNAME
    18, // 19 EW_SEARCHPATH
    19, // 20 EW_GETTEMPFILENAME
    20, // 21 EW_EXTRACTFILE
    21, // 22 EW_DELETEFILE
    22, // 23 EW_MESSAGEBOX
    23, // 24 EW_RMDIR
    24, // 25 EW_STRLEN
    25, // 26 EW_ASSIGNVAR
    26, // 27 EW_STRCMP
    27, // 28 EW_READENVSTR
    28, // 29 EW_INTCMP
    -1, // 30 EW_INTCMPU
    29, // 31 EW_INTOP
    30, // 32 EW_INTFMT
    31, // 33 EW_PUSHPOP
    32, // 34 EW_FINDWINDOW
    33, // 35 EW_SENDMESSAGE
    34, // 36 EW_ISWINDOW
    40, // 37 EW_SHELLEXEC
    41, // 38 EW_EXECUTE
    42, // 39 EW_GETFILETIME
    43, // 40 EW_GETDLLVERSION
    44, // 41 EW_REGISTERDLL
    45, // 42 EW_CREATESHORTCUT
    46, // 43 EW_COPYFILES
    47, // 44 EW_REBOOT
    -1, // 45 EW_IFREBOOTFLAG
    -1, // 46 EW_SETREBOOTFLAG
    48, // 47 EW_WRITEINI
    49, // 48 EW_READINISTR
    50, // 49 EW_DELREG
    51, // 50 EW_WRITEREG
    52, // 51 EW_READREGSTR
    53, // 52 EW_REGENUM
    54, // 53 EW_FCLOSE
    55, // 54 EW_FOPEN
    56, // 55 EW_FPUTS
    57, // 56 EW_FGETS
    58, // 57 EW_FSEEK
    59, // 58 EW_FINDCLOSE
    60, // 59 EW_FINDNEXT
    61, // 60 EW_FINDFIRST
    62, // 61 EW_WRITEUNINSTALLER
    70, // 62 EW_LOG
    63, // 63 EW_SECTIONSET
    -1, // 64 EW_GETLABELADDR
    -1, // 65 EW_GETFUNCTIONADDR
];

/// Returns the canonical opcode an NSIS 1.x opcode corresponds to, or the raw
/// value when 2.0 dropped the instruction and there is nothing to map it to.
pub fn canonical_opcode(which: u32) -> i32 {
    match V1_TO_CANONICAL.get(which as usize) {
        Some(&canonical) if canonical >= 0 => canonical,
        _ => which as i32,
    }
}

/// Returns the operand layout of an NSIS 1.x instruction.
///
/// As with later versions, one opcode can carry several script commands and
/// pick between them with an operand - see
/// [`opcode::param_layout`](crate::opcode::param_layout). 1.x selects them
/// differently, so its rules live here.
pub fn param_layout_v1(which: u32, info: &OpcodeInfo, values: &[i32; 6]) -> ParamLayout {
    let mut layout = ParamLayout::fixed(info);
    let [v0, v1, v2, v3, v4, _] = *values;
    match which {
        // 6: a second operand sets `SetDetailsPrint`'s mode instead of
        // printing.
        6 if v1 != 0 => {
            layout.clear(0);
            layout.effects = NONE.writing(FlagSet::of(&[ExecFlag::StatusUpdate]));
        }
        // 14: SetOutPath sets `$OUTDIR`.
        14 if v1 != 0 => layout.effects = layout.effects.with_outdir(Access::Write),
        // 17 Rename, 22 Delete: a move deferred to the next reboot raises the
        // reboot flag.
        17 if v2 != 0 => layout.effects = layout.effects.writing(REBOOT),
        22 if v1 != 0 => layout.effects = layout.effects.writing(REBOOT),
        // 31: 1.x numbers IntOp's operations its own way: 3 divides and 11
        // takes a remainder, and either sets the error flag on zero.
        31 if v3 == 3 || v3 == 11 => layout.effects = layout.effects.failing(),
        33 => pushpop_form(&mut layout, v1, v2, true),
        // 34 FindWindow, 35 SendMessage: no output variable is -1.
        34 | 35 if v0 < 0 => layout.clear(0),
        // 38: only a wait with a variable records the exit code.
        38 if v1 == 0 || v2 < 0 => layout.clear(2),
        // 41: a negative status marks a plugin call, which gets the variables
        // and the stack - 1.x hands a plugin no flags.
        41 if v2 < 0 => {
            layout.clear(2);
            layout.effects = layout.effects.of_plugin(FlagSet::EMPTY);
        }
        // 44: anything but the magic is a corrupted installer.
        44 if v0 != 0x0BAD_F00D => {
            layout.effects = layout.effects.with_termination(Termination::Always);
        }
        // 47: a negative key or value is `NULL`, which deletes.
        47 => {
            if v1 < 0 {
                layout.clear(1);
            }
            if v2 < 0 {
                layout.clear(2);
            }
        }
        // 49: -1 in place of a value name deletes the key, and the fourth
        // operand then says whether only an empty one.
        49 => {
            if v2 == -1 {
                layout.clear(2);
            } else {
                layout.clear(3);
            }
        }
        // 50: the kind decides the data slot.
        50 => match v4 {
            0 | 1 => {}
            2 => layout.retype(3, Number),
            3 => layout.retype(3, DataOffset),
            _ => layout.clear(3),
        },
        // 55: FileWriteByte writes a number.
        55 if v2 != 0 => layout.retype(1, Number),
        // 57: no variable for the new position.
        57 if v3 < 0 => layout.clear(3),
        // 62: LogSet toggles logging where LogText writes a string.
        62 if v0 != 0 => layout.set(1, "on_off", Int),
        // 63: the operation says both which section property is meant and
        // whether the third operand is read or written. Getting the text
        // expands the name the section holds, which the script may have set to
        // any string.
        63 => match v1 {
            0 if v2 < 0 => layout.set(2, "text", Unused),
            0 => layout.set(2, "text", String),
            1 => {
                layout.set(2, "text", MAY_OUT);
                layout.effects.hidden = HiddenVariables::ReadsAny;
            }
            2 => layout.set(2, "flags", Number),
            _ => layout.set(2, "flags", MAY_OUT),
        },
        _ => {}
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_opcode_is_described() {
        for (i, op) in OPCODES_V1.iter().enumerate() {
            assert!(!op.mnemonic.is_empty(), "opcode {i} has no mnemonic");
            assert!(
                !op.description.is_empty(),
                "{} has no description",
                op.mnemonic
            );
            assert!(!op.category.is_empty(), "{} has no category", op.mnemonic);
            let named = op.param_names.iter().filter(|n| !n.is_empty()).count();
            assert_eq!(
                named, op.param_count as usize,
                "{} names {named} parameters but declares {}",
                op.mnemonic, op.param_count
            );
            let used = op.param_types.iter().filter(|t| **t != Unused).count();
            assert_eq!(
                used, op.param_count as usize,
                "{} types {used} parameters but declares {}",
                op.mnemonic, op.param_count
            );
        }
    }

    #[test]
    fn the_numbering_is_the_one_1x_installers_store() {
        // Verified against tests/fixtures/nsis1x.exe, whose three instructions
        // are CreateDirectory, File and Ret.
        for (which, mnemonic) in [
            (1, "EW_RET"),
            (14, "EW_CREATEDIR"),
            (21, "EW_EXTRACTFILE"),
            (63, "EW_SECTIONSET"),
        ] {
            assert_eq!(lookup_v1(which).map(|o| o.mnemonic), Some(mnemonic));
        }
    }

    #[test]
    fn compiler_only_slots_do_not_resolve() {
        // A 1.x file storing one of these is not a 1.x entry block.
        assert!(lookup_v1(64).is_none());
        assert!(lookup_v1(65).is_none());
        assert!(lookup_v1(66).is_none());
    }

    #[test]
    fn parameters_that_moved_between_generations_follow_the_1x_runtime() {
        // exec.c reads FileOpen's handle out of the last slot and GetFileTime's
        // file out of the first; 2.x swapped both.
        let fopen = lookup_v1(54).expect("FileOpen");
        assert_eq!(fopen.param_names[0], "name");
        assert_eq!(fopen.param_names[3], "handle_out");
        assert_eq!(fopen.param_types[3], OUT);

        let get_file_time = lookup_v1(39).expect("GetFileTime");
        assert_eq!(get_file_time.param_names[0], "file");
        assert_eq!(get_file_time.param_types[0], String);

        let find_first = lookup_v1(60).expect("FindFirst");
        assert_eq!(find_first.param_names[0], "filespec");
        assert_eq!(find_first.param_types[1], OUT);
    }

    /// Resolves a 1.x layout.
    fn layout_v1(which: u32, values: [i32; 6]) -> ParamLayout {
        param_layout_v1(which, lookup_v1(which).expect("known"), &values)
    }

    /// **1.x `ExecWait` keeps its wait flag in slot 1 and its exit code in
    /// slot 2** (`exec.c` 1.98, `EW_EXECUTE`), and writes the code only when
    /// it waits with a variable.
    #[test]
    fn a_1x_exec_wait_writes_its_exit_code_from_slot_two() {
        let exec_wait = layout_v1(38, [7, 1, 3, 0, 0, 0]);
        assert_eq!(exec_wait.types[1], Int);
        assert_eq!(exec_wait.types[2], MAY_OUT);
        assert_eq!(exec_wait.effects.outdir, Some(Access::Read));
        assert_eq!(layout_v1(38, [7, 1, -1, 0, 0, 0]).types[2], Unused);
        assert_eq!(layout_v1(38, [7, 0, 3, 0, 0, 0]).types[2], Unused);
    }

    /// **1.x Push, Pop and Exch are the same three forms**, and a Push's
    /// operand is a string, not the variable the fixed table once said.
    #[test]
    fn a_1x_pushpop_states_its_command() {
        assert_eq!(layout_v1(33, [42, 0, 0, 0, 0, 0]).types[0], String);
        assert_eq!(layout_v1(33, [4, 1, 0, 0, 0, 0]).types[0], MAY_OUT);
        let exch = layout_v1(33, [0, 0, 1, 0, 0, 0]);
        assert_eq!(exch.effects.stack, StackEffect::Exch);
        // 1.x shows no silent-mode box on a short stack.
        assert!(exch.effects.flags_read.is_empty());
    }

    /// **1.x flags live in globals, and the instructions that use them say
    /// so**: `IfErrors` reads and resets the error flag, `SetDetailsPrint` is
    /// `EW_UPDATETEXT` with a mode, a plugin gets no flags.
    #[test]
    fn a_1x_instruction_states_the_flags_it_uses() {
        let if_errors = layout_v1(16, [3, 0, 0, 0, 0, 0]);
        assert!(if_errors.effects.flags_read.contains(ExecFlag::ExecError));
        assert!(
            if_errors
                .effects
                .flags_written
                .contains(ExecFlag::ExecError)
        );

        let details_print = layout_v1(6, [-1, 2, 0, 0, 0, 0]);
        assert_eq!(details_print.types[0], Unused);
        assert!(
            details_print
                .effects
                .flags_written
                .contains(ExecFlag::StatusUpdate)
        );
        let detail_print = layout_v1(6, [9, 0, 0, 0, 0, 0]);
        assert!(
            detail_print
                .effects
                .flags_read
                .contains(ExecFlag::StatusUpdate)
        );

        let plugin = layout_v1(41, [1, 2, -1, 0, 0, 0]);
        assert!(plugin.effects.plugin);
        // Only the load can fail; the plugin itself cannot reach a flag.
        assert_eq!(plugin.effects.flags_written, ERROR);
        assert_eq!(plugin.types[2], Unused);
    }

    /// **1.x deletes with -1**: a missing INI key or value, or a registry
    /// value name, is -1 rather than a string.
    #[test]
    fn a_1x_deletion_names_nothing_it_deletes() {
        let delete_section = layout_v1(47, [1, -1, -1, 4, 0, 0]);
        assert_eq!(
            (delete_section.types[1], delete_section.types[2]),
            (Unused, Unused)
        );
        let delete_key = layout_v1(49, [-2147483646, 5, -1, 1, 0, 0]);
        assert_eq!((delete_key.types[2], delete_key.types[3]), (Unused, Int));
        let delete_value = layout_v1(49, [-2147483646, 5, 6, 0, 0, 0]);
        assert_eq!(
            (delete_value.types[2], delete_value.types[3]),
            (String, Unused)
        );
    }
}
