//! Opcode metadata definitions.
//!
//! Each NSIS opcode has static metadata describing its mnemonic, parameter
//! count, parameter names, description, and semantic category.

use ParamType::{Int, Jump, String, Unused, Variable};

/// The semantic type of an opcode parameter.
///
/// NSIS entry parameters are raw i32 values. Their interpretation depends
/// on the opcode and parameter position. This enum classifies each parameter
/// so consumers can resolve them correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// Unused parameter slot.
    Unused,
    /// Offset into the string table (resolve via `NsisInstaller::read_string`).
    String,
    /// Variable index (resolve via `strings::variable_name`).
    Variable,
    /// Entry index used as a jump/call target.
    Jump,
    /// Literal integer (flags, sizes, modes, operation codes, etc.).
    Int,
}

/// The operand layout of one instruction.
///
/// Most opcodes have a fixed layout, the one held in [`OpcodeInfo`]. A few
/// pack several script commands into a single opcode and pick between them
/// with an operand: `SectionGetText` and `SectionSetText` are both
/// [`EW_SECTIONSET`](crate::opcode::EW_SECTIONSET), and they disagree about
/// which slots hold string offsets. [`param_layout`] resolves that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamLayout {
    /// Name for each parameter slot; empty where the form has no operand.
    pub names: [&'static str; 6],
    /// Semantic type of each parameter slot.
    pub types: [ParamType; 6],
    /// Number of slots to consider, from the start.
    pub count: u8,
}

impl ParamLayout {
    /// The fixed layout an opcode declares.
    fn fixed(info: &OpcodeInfo) -> Self {
        Self {
            names: info.param_names,
            types: info.param_types,
            count: info.param_count,
        }
    }
}

/// Property names for the operations [`EW_SECTIONSET`](crate::opcode::EW_SECTIONSET)
/// reads and writes, in the order NSIS numbers them.
const SECTION_PROPERTIES: [&str; 6] = [
    "text",
    "inst_types",
    "flags",
    "code",
    "code_size",
    "size_kb",
];

/// Returns the operand layout for one instruction.
///
/// `which` must already be normalized (see
/// [`NsisInstaller::resolve_opcode`](crate::NsisInstaller::resolve_opcode)),
/// and `values` are the entry's six raw operands.
///
/// Rendering a form-selecting opcode from its fixed layout does not merely
/// mislabel operands, it loses them: `SectionSetText` keeps its text in the
/// fifth slot, which the fixed layout marks unused, and `LogSet` keeps an
/// on/off flag where `LogText` keeps a string offset — read as a string, that
/// flag resolves to whatever text happens to sit at offset 1. The forms and
/// slot assignments below follow 7-Zip's `NsisIn.cpp`.
pub fn param_layout(which: u32, info: &OpcodeInfo, values: &[i32; 6]) -> ParamLayout {
    let mut layout = ParamLayout::fixed(info);
    match which as i32 {
        crate::opcode::EW_LOG => {
            // LogSet toggles logging; only LogText carries a string.
            if values[0] != 0 {
                layout.names[1] = "on_off";
                layout.types[1] = Int;
            }
        }
        crate::opcode::EW_SECTIONSET => {
            // The operation doubles as the direction: negative sets, and the
            // property it names decides which slot the value lives in.
            // Widened, because -1 is the first set operation and i32::MIN has
            // no positive counterpart to negate towards.
            let op = i64::from(values[2]);
            let (property, sets) = if op >= 0 {
                (op, false)
            } else {
                (op.saturating_add(1).saturating_neg(), true)
            };
            let name = usize::try_from(property)
                .ok()
                .and_then(|i| SECTION_PROPERTIES.get(i))
                .copied()
                .unwrap_or("value");

            layout.names = ["section", "", "op", "", "", ""];
            layout.types = [String, Unused, Int, Unused, Unused, Unused];
            layout.count = 3;
            match (sets, property) {
                // SectionSetText stores its text in the fifth slot; every other
                // set operation stores its value in the second.
                (true, 0) => {
                    layout.names[4] = name;
                    layout.types[4] = String;
                    layout.names[3] = "flags_changed";
                    layout.types[3] = Int;
                    layout.count = 5;
                }
                (true, _) => {
                    layout.names[1] = name;
                    layout.types[1] = String;
                    layout.names[3] = "flags_changed";
                    layout.types[3] = Int;
                    layout.count = 4;
                }
                (false, _) => {
                    layout.names[1] = name;
                    layout.types[1] = Variable;
                }
            }
        }
        crate::opcode::EW_INSTTYPESET => {
            // Two independent flags select between four script commands.
            let current = values[3] != 0;
            let writes = values[2] != 0;
            layout.names = ["inst_type", "", "op", "cur", "", ""];
            layout.types = [String, Unused, Int, Int, Unused, Unused];
            layout.count = 4;
            match (current, writes) {
                // InstTypeGetText / InstTypeSetText
                (false, false) => {
                    layout.names[1] = "text";
                    layout.types[1] = Variable;
                }
                (false, true) => {
                    layout.names[1] = "text";
                    layout.types[1] = String;
                }
                // GetCurInstType reads into a variable and takes no index.
                (true, false) => {
                    layout.names[0] = "";
                    layout.types[0] = Unused;
                    layout.names[1] = "output";
                    layout.types[1] = Variable;
                }
                // SetCurInstType
                (true, true) => {}
            }
        }
        _ => {}
    }
    layout
}

/// Static metadata for a single NSIS opcode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpcodeInfo {
    /// The opcode mnemonic (e.g., `"EW_EXTRACTFILE"`).
    pub mnemonic: &'static str,
    /// Number of meaningful parameters (0..6).
    pub param_count: u8,
    /// Human-readable names for each parameter.
    pub param_names: [&'static str; 6],
    /// Semantic type of each parameter.
    pub param_types: [ParamType; 6],
    /// Brief description of what the opcode does.
    pub description: &'static str,
    /// Semantic category (e.g., `"file"`, `"registry"`, `"string"`, `"flow"`).
    pub category: &'static str,
}

/// The opcode table.
///
/// Indices are the `which` field of an entry, in the layout a standard
/// makensis produces — NSIS 2 and NSIS 3 number their instructions the same
/// way. What varies is which instructions a build *has*: a build compiled with
/// logging adds one, the Park fork adds three, and the two UTF-16 file
/// commands exist only in a Unicode build. Those shift the stored numbering,
/// which [`normalize_log_opcode`](super::normalize_log_opcode) and
/// [`normalize_park_opcode`](super::normalize_park_opcode) undo before lookup,
/// so a caller sees one numbering whatever produced the file.
///
/// Parameter counts are the maximum an opcode has taken across NSIS versions,
/// matching 7-Zip and Binary Refinery: older releases passed operands that
/// newer ones dropped, and a count that is too low makes a valid entry look
/// malformed.
///
/// Parameter types are derived from the NSIS source (`exec.c`) and the
/// 7-Zip NSIS handler (`NsisIn.cpp`).
pub static OPCODES: [OpcodeInfo; 72] = [
    OpcodeInfo {
        mnemonic: "EW_INVALID_OPCODE",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        description: "Invalid/error opcode",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_RET",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        description: "Return from function",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_NOP",
        param_count: 1,
        param_names: ["jump_addr", "", "", "", "", ""],
        param_types: [Jump, Unused, Unused, Unused, Unused, Unused],
        description: "No-op / Jump",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_ABORT",
        param_count: 1,
        param_names: ["status_text", "", "", "", "", ""],
        param_types: [String, Unused, Unused, Unused, Unused, Unused],
        description: "Abort installation",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_QUIT",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        description: "Quit installer",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_CALL",
        param_count: 2,
        param_names: ["address", "", "", "", "", ""],
        param_types: [Jump, Unused, Unused, Unused, Unused, Unused],
        description: "Call subroutine",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_UPDATETEXT",
        param_count: 6,
        param_names: ["text", "flag", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        description: "Update status text",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SLEEP",
        param_count: 1,
        param_names: ["milliseconds", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        description: "Sleep",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_BRINGTOFRONT",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        description: "Bring window to front",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_CHDETAILSVIEW",
        param_count: 2,
        param_names: ["list_hwnd", "button_hwnd", "", "", "", ""],
        param_types: [Int, Int, Unused, Unused, Unused, Unused],
        description: "Set details view",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SETFILEATTRIBUTES",
        param_count: 2,
        param_names: ["file", "attributes", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        description: "Set file attributes",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATEDIR",
        param_count: 3,
        param_names: ["path", "update_instdir", "acl", "", "", ""],
        param_types: [String, Int, Int, Unused, Unused, Unused],
        description: "Create directory",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_IFFILEEXISTS",
        param_count: 3,
        param_names: ["file", "jump_yes", "jump_no", "", "", ""],
        param_types: [String, Jump, Jump, Unused, Unused, Unused],
        description: "If file exists",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_SETFLAG",
        param_count: 3,
        param_names: ["id", "data", "lastused", "", "", ""],
        param_types: [Int, String, Int, Unused, Unused, Unused],
        description: "Set exec flag",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_IFFLAG",
        param_count: 4,
        param_names: ["jump_on", "jump_off", "id", "mask", "", ""],
        param_types: [Jump, Jump, Int, Int, Unused, Unused],
        description: "If flag set",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFLAG",
        param_count: 2,
        param_names: ["output", "id", "", "", "", ""],
        param_types: [Variable, Int, Unused, Unused, Unused, Unused],
        description: "Get exec flag",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_RENAME",
        param_count: 4,
        param_names: ["old", "new", "rebootok", "log_text", "", ""],
        param_types: [String, String, Int, String, Unused, Unused],
        description: "Rename/move file",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFULLPATHNAME",
        param_count: 3,
        param_names: ["input", "output", "lfn_sfn", "", "", ""],
        param_types: [String, Variable, Int, Unused, Unused, Unused],
        description: "Get full path name",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_SEARCHPATH",
        param_count: 2,
        param_names: ["output", "filename", "", "", "", ""],
        param_types: [Variable, String, Unused, Unused, Unused, Unused],
        description: "Search PATH",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETTEMPFILENAME",
        param_count: 2,
        param_names: ["output", "basedir", "", "", "", ""],
        param_types: [Variable, String, Unused, Unused, Unused, Unused],
        description: "Get temp filename",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_EXTRACTFILE",
        param_count: 6,
        param_names: [
            "overwrite",
            "name",
            "data_offset",
            "date_lo",
            "date_hi",
            "allow_ignore",
        ],
        param_types: [Int, String, Int, Int, Int, Int],
        description: "Extract file from archive",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_DELETEFILE",
        param_count: 2,
        param_names: ["filename", "rebootok", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        description: "Delete file",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_MESSAGEBOX",
        param_count: 6,
        param_names: ["mb_flags", "text", "button1", "jump1", "button2", "jump2"],
        param_types: [Int, String, Int, Jump, Int, Jump],
        description: "Message box",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_RMDIR",
        param_count: 2,
        param_names: ["path", "flags", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        description: "Remove directory",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_STRLEN",
        param_count: 2,
        param_names: ["output", "input", "", "", "", ""],
        param_types: [Variable, String, Unused, Unused, Unused, Unused],
        description: "String length",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_ASSIGNVAR",
        param_count: 4,
        param_names: ["var", "string", "maxlen", "startpos", "", ""],
        param_types: [Variable, String, String, String, Unused, Unused],
        description: "StrCpy",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_STRCMP",
        param_count: 5,
        param_names: ["s1", "s2", "jump_eq", "jump_neq", "case", ""],
        param_types: [String, String, Jump, Jump, Int, Unused],
        description: "String compare",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_READENVSTR",
        param_count: 3,
        param_names: ["output", "string", "is_read", "", "", ""],
        param_types: [Variable, String, Int, Unused, Unused, Unused],
        description: "ReadEnvStr/ExpandEnvStrings",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_INTCMP",
        param_count: 6,
        param_names: ["v1", "v2", "jump_eq", "jump_lt", "jump_gt", "flags"],
        param_types: [String, String, Jump, Jump, Jump, Int],
        description: "Integer compare",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_INTOP",
        param_count: 4,
        param_names: ["output", "input1", "input2", "op", "", ""],
        param_types: [Variable, String, String, Int, Unused, Unused],
        description: "Integer operation",
        category: "math",
    },
    OpcodeInfo {
        mnemonic: "EW_INTFMT",
        param_count: 4,
        param_names: ["output", "format", "input", "is_64bit", "", ""],
        param_types: [Variable, String, String, Int, Unused, Unused],
        description: "IntFmt/Int64Fmt",
        category: "math",
    },
    OpcodeInfo {
        mnemonic: "EW_PUSHPOP",
        param_count: 6,
        param_names: ["var_or_str", "pop_or_push", "exch", "", "", ""],
        param_types: [String, Int, Int, Unused, Unused, Unused],
        description: "Push/Pop/Exch",
        category: "stack",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDWINDOW",
        param_count: 5,
        param_names: ["output", "class", "title", "parent", "after", ""],
        param_types: [Variable, String, String, String, String, Unused],
        description: "FindWindow",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SENDMESSAGE",
        param_count: 6,
        param_names: ["output", "hwnd", "msg", "wparam", "lparam", "flags"],
        param_types: [Variable, String, String, String, String, Int],
        description: "SendMessage",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_ISWINDOW",
        param_count: 3,
        param_names: ["hwnd", "jump_yes", "jump_no", "", "", ""],
        param_types: [String, Jump, Jump, Unused, Unused, Unused],
        description: "IsWindow",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_GETDLGITEM",
        param_count: 3,
        param_names: ["output", "dialog", "item_id", "", "", ""],
        param_types: [Variable, String, String, Unused, Unused, Unused],
        description: "GetDlgItem",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SETCTLCOLORS",
        param_count: 2,
        param_names: ["hwnd", "colors_ptr", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        description: "Set control colors",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_LOADANDSETIMAGE",
        param_count: 4,
        param_names: ["ctrl", "type_flags", "imageid", "output", "", ""],
        param_types: [String, Int, Int, Variable, Unused, Unused],
        description: "Load and set image",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATEFONT",
        param_count: 5,
        param_names: ["output", "face", "height", "weight", "flags", ""],
        param_types: [Variable, String, String, String, Int, Unused],
        description: "CreateFont",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SHOWWINDOW",
        param_count: 4,
        param_names: ["hwnd", "show_state", "hide", "enable", "", ""],
        param_types: [String, String, Int, Int, Unused, Unused],
        description: "ShowWindow",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SHELLEXEC",
        param_count: 6,
        param_names: ["verb", "file", "params", "showwindow", "", "status_text"],
        param_types: [String, String, String, Int, Unused, String],
        description: "ShellExecute",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_EXECUTE",
        param_count: 3,
        param_names: ["cmdline", "wait", "output", "", "", ""],
        param_types: [String, Variable, Int, Unused, Unused, Unused],
        description: "Exec/ExecWait",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFILETIME",
        param_count: 3,
        param_names: ["hi_out", "lo_out", "file", "", "", ""],
        param_types: [Variable, Variable, String, Unused, Unused, Unused],
        description: "GetFileTime",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETDLLVERSION",
        param_count: 4,
        param_names: ["hi_out", "lo_out", "file", "kind", "", ""],
        param_types: [Variable, Variable, String, Int, Unused, Unused],
        description: "GetDLLVersion",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_REGISTERDLL",
        param_count: 6,
        param_names: ["dll", "function", "register", "nounload", "", ""],
        param_types: [String, String, Int, Int, Unused, Unused],
        description: "RegisterDLL/plugin call",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATESHORTCUT",
        param_count: 6,
        param_names: ["link", "target", "params", "icon", "packed_cs", ""],
        param_types: [String, String, String, String, Int, Unused],
        description: "CreateShortcut",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_COPYFILES",
        param_count: 4,
        param_names: ["source", "dest", "flags", "status_text", "", ""],
        param_types: [String, String, Int, String, Unused, Unused],
        description: "CopyFiles",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_REBOOT",
        param_count: 1,
        param_names: ["type", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        description: "Reboot",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEINI",
        param_count: 5,
        param_names: ["section", "name", "value", "ini_file", "", ""],
        param_types: [String, String, String, String, Unused, Unused],
        description: "WriteINIStr",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_READINISTR",
        param_count: 4,
        param_names: ["output", "section", "name", "ini_file", "", ""],
        param_types: [Variable, String, String, String, Unused, Unused],
        description: "ReadINIStr",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_DELREG",
        param_count: 5,
        param_names: ["", "root", "keyname", "valuename", "flags", ""],
        param_types: [Unused, Int, String, String, Int, Unused],
        description: "DeleteRegValue/Key",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEREG",
        param_count: 6,
        param_names: ["root", "keyname", "itemname", "data", "typelen", "flags"],
        param_types: [Int, String, String, String, Int, Int],
        description: "WriteRegStr/DWORD/Bin",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_READREGSTR",
        param_count: 5,
        param_names: ["output", "root", "keyname", "itemname", "type", ""],
        param_types: [Variable, Int, String, String, Int, Unused],
        description: "ReadRegStr/DWORD",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_REGENUM",
        param_count: 5,
        param_names: ["output", "root", "keyname", "index", "key_or_value", ""],
        param_types: [Variable, Int, String, Int, Int, Unused],
        description: "RegEnumKey/Value",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_FCLOSE",
        param_count: 1,
        param_names: ["handle", "", "", "", "", ""],
        param_types: [Variable, Unused, Unused, Unused, Unused, Unused],
        description: "FileClose",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FOPEN",
        param_count: 4,
        param_names: ["handle_out", "openmode", "createmode", "name", "", ""],
        param_types: [Variable, Int, Int, String, Unused, Unused],
        description: "FileOpen",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FPUTS",
        param_count: 3,
        param_names: ["handle", "string", "int_or_str", "", "", ""],
        param_types: [Variable, String, Int, Unused, Unused, Unused],
        description: "FileWrite",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FGETS",
        param_count: 4,
        param_names: ["handle", "output", "maxlen", "getchar_gets", "", ""],
        param_types: [Variable, Variable, String, Int, Unused, Unused],
        description: "FileRead",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FSEEK",
        param_count: 4,
        param_names: ["handle", "pos_out", "offset", "mode", "", ""],
        param_types: [Variable, Variable, String, Int, Unused, Unused],
        description: "FileSeek",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDCLOSE",
        param_count: 1,
        param_names: ["handle", "", "", "", "", ""],
        param_types: [Variable, Unused, Unused, Unused, Unused, Unused],
        description: "FindClose",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDNEXT",
        param_count: 2,
        param_names: ["output", "handle", "", "", "", ""],
        param_types: [Variable, Variable, Unused, Unused, Unused, Unused],
        description: "FindNext",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDFIRST",
        param_count: 3,
        param_names: ["output", "handle_out", "filespec", "", "", ""],
        param_types: [Variable, Variable, String, Unused, Unused, Unused],
        description: "FindFirst",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEUNINSTALLER",
        param_count: 4,
        param_names: ["name", "offset", "icon_size", "", "", ""],
        param_types: [String, Int, Int, Unused, Unused, Unused],
        description: "WriteUninstaller",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_SECTIONSET",
        param_count: 5,
        param_names: ["idx", "op", "data", "", "", ""],
        param_types: [Int, Int, Int, Unused, Unused, Unused],
        description: "SectionSet/GetText/Flags",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_INSTTYPESET",
        param_count: 4,
        param_names: ["idx", "op", "flags", "", "", ""],
        param_types: [Int, Int, Int, Unused, Unused, Unused],
        description: "InstTypeSet/GetFlags",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_GETOSINFO",
        param_count: 6,
        param_names: ["operation", "varies", "", "", "", ""],
        param_types: [Int, Int, Unused, Unused, Unused, Unused],
        description: "GetOSInfo/GetKnownFolderPath",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_RESERVEDOPCODE",
        param_count: 2,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        description: "Reserved/free slot",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_LOCKWINDOW",
        param_count: 1,
        param_names: ["on_off", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        description: "Lock/unlock window updates",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_FPUTWS",
        param_count: 4,
        param_names: ["handle", "string", "int_or_str", "bom", "", ""],
        param_types: [Variable, String, Int, Int, Unused, Unused],
        description: "FileWriteUTF16LE",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FGETWS",
        param_count: 4,
        param_names: ["handle", "output", "maxlen", "getchar", "", ""],
        param_types: [Variable, Variable, String, Int, Unused, Unused],
        description: "FileReadUTF16LE",
        category: "file_io",
    },
    // The entries below are not opcodes an installer stores. They are slots
    // this crate translates conditional layouts into, so that a caller sees one
    // numbering whatever the installer was compiled with.
    OpcodeInfo {
        mnemonic: "EW_LOG",
        param_count: 2,
        param_names: ["type", "text", "", "", "", ""],
        param_types: [Int, String, Unused, Unused, Unused, Unused],
        description: "LogText/LogSet (log-enabled builds only)",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDPROC",
        param_count: 2,
        param_names: ["result", "process", "", "", "", ""],
        param_types: [Variable, String, Unused, Unused, Unused, Unused],
        description: "FindProc (Park fork)",
        category: "process",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcode::{EW_INSTTYPESET, EW_LOG, EW_SECTIONSET, lookup};

    /// Resolves the layout for an instruction with the given operands.
    fn layout_of(which: i32, values: [i32; 6]) -> ParamLayout {
        let info = lookup(which as u32).expect("opcode should be known");
        param_layout(which as u32, info, &values)
    }

    /// The name and type each operand slot renders under, up to `count`.
    fn slots(layout: &ParamLayout) -> Vec<(&'static str, ParamType)> {
        layout
            .names
            .iter()
            .zip(layout.types.iter())
            .take(layout.count as usize)
            .map(|(&n, &t)| (n, t))
            .collect()
    }

    #[test]
    fn log_set_does_not_read_its_flag_as_a_string() {
        // LogText carries a string; LogSet carries on/off in the same slot.
        // Reading that flag as a string offset resolves it to whatever text
        // sits at offset 1, which is how `LogSet on` rendered as
        // `text="ProgramFilesDir"`.
        let text = layout_of(EW_LOG, [0, 42, 0, 0, 0, 0]);
        assert_eq!(
            slots(&text),
            [("type", ParamType::Int), ("text", ParamType::String)]
        );

        let set = layout_of(EW_LOG, [1, 1, 0, 0, 0, 0]);
        assert_eq!(
            slots(&set),
            [("type", ParamType::Int), ("on_off", ParamType::Int)]
        );
    }

    #[test]
    fn section_get_reads_into_a_variable() {
        // Operation >= 0 reads; the second slot is the destination variable.
        let get = layout_of(EW_SECTIONSET, [10, 0, 0, 0, 0, 0]);
        assert_eq!(
            slots(&get),
            [
                ("section", ParamType::String),
                ("text", ParamType::Variable),
                ("op", ParamType::Int),
            ]
        );

        // The operation names the property being read.
        let flags = layout_of(EW_SECTIONSET, [10, 0, 2, 0, 0, 0]);
        assert_eq!(slots(&flags)[1], ("flags", ParamType::Variable));
    }

    #[test]
    fn section_set_text_keeps_its_text_in_the_fifth_slot() {
        // SectionSetText is the one set operation that does not use slot 1,
        // so a fixed layout renders it without the text it is setting.
        let set_text = layout_of(EW_SECTIONSET, [10, 0, -1, 0, 42, 0]);
        assert_eq!(
            slots(&set_text),
            [
                ("section", ParamType::String),
                ("", ParamType::Unused),
                ("op", ParamType::Int),
                ("flags_changed", ParamType::Int),
                ("text", ParamType::String),
            ]
        );

        // Every other set operation puts its value in slot 1.
        let set_flags = layout_of(EW_SECTIONSET, [10, 42, -3, 1, 0, 0]);
        assert_eq!(slots(&set_flags)[1], ("flags", ParamType::String));
    }

    #[test]
    fn section_op_out_of_range_still_renders() {
        // A property number this crate has no name for must not be dropped or
        // panic the renderer; NSIS gained operations over time.
        let unknown = layout_of(EW_SECTIONSET, [10, 42, -99, 0, 0, 0]);
        assert_eq!(slots(&unknown)[1], ("value", ParamType::String));

        // i32::MIN has no positive counterpart, so negating it would overflow.
        // It names no property, so it renders as a plain set.
        let extreme = layout_of(EW_SECTIONSET, [10, 42, i32::MIN, 0, 0, 0]);
        assert_eq!(slots(&extreme)[1], ("value", ParamType::String));
    }

    #[test]
    fn inst_type_set_selects_between_four_commands() {
        let cases = [
            // (cur, writes) -> the slot-1 rendering of each command
            ([0, 7, 0, 0, 0, 0], ("text", ParamType::Variable)), // InstTypeGetText
            ([0, 7, 1, 0, 0, 0], ("text", ParamType::String)),   // InstTypeSetText
            ([0, 7, 0, 1, 0, 0], ("output", ParamType::Variable)), // GetCurInstType
            ([0, 0, 1, 1, 0, 0], ("", ParamType::Unused)),       // SetCurInstType
        ];
        for (values, expected) in cases {
            let layout = layout_of(EW_INSTTYPESET, values);
            assert_eq!(slots(&layout)[1], expected, "operands {values:?}");
        }

        // GetCurInstType takes no install-type index, so slot 0 is not a string.
        let get_cur = layout_of(EW_INSTTYPESET, [0, 7, 0, 1, 0, 0]);
        assert_eq!(slots(&get_cur)[0], ("", ParamType::Unused));
    }

    #[test]
    fn opcodes_without_forms_keep_their_fixed_layout() {
        for (i, info) in OPCODES.iter().enumerate() {
            if matches!(i as i32, EW_LOG | EW_SECTIONSET | EW_INSTTYPESET) {
                continue;
            }
            let layout = param_layout(i as u32, info, &[1; 6]);
            assert_eq!(layout.names, info.param_names, "{}", info.mnemonic);
            assert_eq!(layout.types, info.param_types, "{}", info.mnemonic);
            assert_eq!(layout.count, info.param_count, "{}", info.mnemonic);
        }
    }

    #[test]
    fn all_opcodes_have_mnemonics() {
        for (i, op) in OPCODES.iter().enumerate() {
            assert!(!op.mnemonic.is_empty(), "opcode {i} has empty mnemonic");
        }
    }

    #[test]
    fn extract_file_opcode() {
        let op = &OPCODES[20];
        assert_eq!(op.mnemonic, "EW_EXTRACTFILE");
        assert_eq!(op.param_count, 6);
        assert_eq!(op.category, "file");
    }
}
