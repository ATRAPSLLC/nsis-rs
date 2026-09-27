//! Opcode metadata definitions.
//!
//! Each NSIS opcode has static metadata describing its mnemonic, what each of
//! its operand slots holds and how the instruction uses it, what it does
//! beyond its slots, and a description and category.
//!
//! # Provenance
//!
//! Every slot's role and every effect is what the runtime does with it:
//! `Source/exehead/exec.c` of NSIS 3.10, checked against NSIS 2.46's, with the
//! compiler (`script.cpp`, `build.cpp`) consulted for what it stores in each
//! slot. Where the two runtimes differ the difference is stated at the opcode.

use crate::opcode::{
    NsisVersion,
    semantics::{Access, Effects, ExecFlag, FlagSet, HiddenVariables, StackEffect, Termination},
};

use ParamType::{DataOffset, Flag, Int, Jump, Number, RawString, String, Unused, Variable};

/// The semantic type of an opcode parameter.
///
/// NSIS entry parameters are raw i32 values. Their interpretation depends on
/// the opcode, the parameter position and, for several opcodes, the other
/// parameters. This enum says what one slot holds and, for the slots that name
/// script state, how the instruction uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamType {
    /// Unused parameter slot.
    Unused,
    /// A string the instruction expands when it runs, reading every variable
    /// the string embeds.
    ///
    /// A value from zero up is an offset into the string table - zero is the
    /// empty string the table starts with. A negative value is a language
    /// string, `-(n + 1)` for string `n` of the running language's table.
    /// [`StringTable::read`](crate::strings::StringTable::read) resolves both.
    String,
    /// A [`String`](Self::String) the instruction reads as an integer once
    /// expanded (`myatoi`): a literal like `"5"`, or a variable holding one.
    Number,
    /// A string-table offset the instruction stores without expanding it. The
    /// variables it embeds are read later, wherever the stored string is.
    RawString,
    /// A variable index, and how the instruction uses the variable.
    ///
    /// Resolve the index with
    /// [`StringTable::variable_name`](crate::strings::StringTable::variable_name).
    Variable(Access),
    /// An execution-flag index ([`ExecFlag::from_index`]), and how the
    /// instruction uses the flag.
    Flag(Access),
    /// A jump target: `0` continues with the next entry, `n > 0` is entry
    /// `n - 1`, and a negative value is a variable, `-(v + 1)`, whose value is
    /// the one-based target
    /// ([`ControlFlowTarget::resolve`](crate::installer::script::ControlFlowTarget::resolve)).
    Jump,
    /// Literal integer (flags, sizes, modes, operation codes, etc.).
    Int,
    /// An offset into the data block, where the instruction's compressed data
    /// starts.
    DataOffset,
}

impl ParamType {
    /// Reports whether the slot names a variable.
    #[must_use]
    pub fn is_variable(self) -> bool {
        matches!(self, Variable(_))
    }

    /// Reports whether the slot holds a string reference, expanded or not.
    #[must_use]
    pub fn is_string(self) -> bool {
        matches!(self, String | Number | RawString)
    }
}

/// A variable the instruction always writes.
const OUT: ParamType = Variable(Access::Write);
/// A variable the instruction writes only on some runs.
const MAY_OUT: ParamType = Variable(Access::MayWrite);
/// A variable the instruction reads.
const IN: ParamType = Variable(Access::Read);

/// No effect beyond the slots.
const NONE: Effects = Effects::NONE;
/// Sets the error flag when it fails.
const FAILS: Effects = Effects::NONE.failing();
/// Sets the error flag when it fails, and reports a status line.
const FAILS_REPORTING: Effects = Effects::NONE.failing().reporting();
/// A registry command: it can fail, and opens keys in the view `SetRegView`
/// chose (`GetRegKeyAndSAM` in `util.c`).
const REGISTRY: Effects = Effects::NONE
    .failing()
    .reading(FlagSet::of(&[ExecFlag::AlterRegView]));

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
    /// What the instruction does beyond its slots.
    pub effects: Effects,
}

impl ParamLayout {
    /// The fixed layout an opcode declares.
    pub(crate) fn fixed(info: &OpcodeInfo) -> Self {
        Self {
            names: info.param_names,
            types: info.param_types,
            count: info.param_count,
            effects: info.effects,
        }
    }

    /// Names and types slot `index`, when it exists.
    pub(crate) fn set(&mut self, index: usize, name: &'static str, kind: ParamType) {
        if let (Some(slot_name), Some(slot_kind)) =
            (self.names.get_mut(index), self.types.get_mut(index))
        {
            *slot_name = name;
            *slot_kind = kind;
        }
    }

    /// Changes slot `index`'s type, keeping its name.
    pub(crate) fn retype(&mut self, index: usize, kind: ParamType) {
        if let Some(slot_kind) = self.types.get_mut(index) {
            *slot_kind = kind;
        }
    }

    /// Marks slot `index` as carrying no operand in this form.
    pub(crate) fn clear(&mut self, index: usize) {
        self.set(index, "", Unused);
    }

    /// Returns each slot's name, type and raw value, up to the layout's count.
    pub fn slots<'v>(
        &self,
        values: &'v [i32; 6],
    ) -> impl Iterator<Item = (usize, &'static str, ParamType, i32)> + 'v {
        let names = self.names;
        let types = self.types;
        (0..usize::from(self.count)).filter_map(move |index| {
            Some((
                index,
                *names.get(index)?,
                *types.get(index)?,
                *values.get(index)?,
            ))
        })
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

/// `EW_REBOOT`'s operand: anything else is a corrupted installer.
const REBOOT_MAGIC: i32 = 0x0BAD_F00D;
/// `DEL_REBOOT`: `Delete` and `RMDir` retry at the next reboot.
const DEL_REBOOT: i32 = 4;
/// `CS_NWD`: a shortcut with no working directory.
const CS_NWD: i32 = 0x8000;
/// `LASIF_HWND`: `LoadAndSetImage`'s control is a window handle.
const LASIF_HWND: i32 = 0x100;
/// `LASIF_STRID`: `LoadAndSetImage`'s image is named by a string.
const LASIF_STRID: i32 = 0x1_0000;
/// `REG_SZ`, `REG_BINARY` and `REG_DWORD`: what `EW_WRITEREG` writes.
const REG_SZ: i32 = 1;
/// See [`REG_SZ`].
const REG_BINARY: i32 = 3;
/// See [`REG_SZ`].
const REG_DWORD: i32 = 4;
/// `GETOSINFO_KNOWNFOLDER` and `GETOSINFO_READMEMORY`.
const GETOSINFO_KNOWNFOLDER: i32 = 0;
/// See [`GETOSINFO_KNOWNFOLDER`].
const GETOSINFO_READMEMORY: i32 = 1;

/// Returns the operand layout for one instruction.
///
/// `which` must already be normalized (see
/// [`NsisInstaller::resolve_opcode`](crate::NsisInstaller::resolve_opcode)),
/// `info` is its entry under `version` ([`lookup_for`](super::lookup_for)),
/// and `values` are the entry's six raw operands.
///
/// Rendering a form-selecting opcode from its fixed layout does not merely
/// mislabel operands, it loses them: `SectionSetText` keeps its text in the
/// fifth slot, which the fixed layout marks unused, and `LogSet` keeps an
/// on/off flag where `LogText` keeps a string offset - read as a string, that
/// flag resolves to whatever text happens to sit at offset 1. Every form below
/// is the branch `exec.c` takes on the same operands.
pub fn param_layout(
    which: u32,
    version: NsisVersion,
    info: &OpcodeInfo,
    values: &[i32; 6],
) -> ParamLayout {
    let mut layout = ParamLayout::fixed(info);
    let [v0, v1, v2, v3, v4, v5] = *values;
    let Ok(which) = i32::try_from(which) else {
        return layout;
    };
    match which {
        crate::opcode::EW_CREATEDIR if v1 != 0 => {
            // SetOutPath: the directory becomes `$OUTDIR`.
            layout.effects = layout.effects.with_outdir(Access::Write);
        }
        crate::opcode::EW_SETFLAG if v2 > 0 => {
            // Restores the value an earlier `v2 == 0` form saved, or with a
            // negative fourth operand the one a `v2 < 0` form saved: the
            // value is the runtime's, not an operand.
            layout.clear(1);
            layout.set(3, "saved_status", Int);
        }
        crate::opcode::EW_IFFLAG if v3 == -1 => {
            // Masking with every bit set leaves the flag as it was.
            layout.set(2, "flag", Flag(Access::Read));
        }
        crate::opcode::EW_RENAME | crate::opcode::EW_DELETEFILE | crate::opcode::EW_RMDIR => {
            // A move deferred to the next reboot raises the reboot flag
            // (`MoveFileOnReboot` in `util.c`).
            let reboot = if which == crate::opcode::EW_RENAME {
                v2 != 0
            } else {
                v1 & DEL_REBOOT != 0
            };
            if reboot {
                layout.effects = layout.effects.writing(FlagSet::of(&[ExecFlag::ExecReboot]));
            }
        }
        crate::opcode::EW_INTOP if v3 == 3 || v3 == 10 => {
            // Dividing by zero sets the error flag.
            layout.effects = layout.effects.failing();
        }
        crate::opcode::EW_PUSHPOP => pushpop_form(&mut layout, v1, v2, false),
        crate::opcode::EW_FINDWINDOW | crate::opcode::EW_SENDMESSAGE => {
            // No output variable is stored as -1.
            if v0 < 0 {
                layout.clear(0);
            }
            // The low bits say which message parameters are strings; both are
            // read as integers first either way.
            if v5 & 1 != 0 {
                layout.retype(3, String);
            }
            if v5 & 2 != 0 {
                layout.retype(4, String);
            }
            // The bits above are a `SendMessageTimeout` timeout, which can
            // fail.
            if which == crate::opcode::EW_SENDMESSAGE && v5 & !3 != 0 {
                layout.effects = layout.effects.failing();
            }
        }
        crate::opcode::EW_LOADANDSETIMAGE if version == NsisVersion::V3 => {
            if v0 < 0 {
                layout.clear(0);
            }
            if v3 & LASIF_STRID != 0 {
                layout.retype(1, String);
            }
            if v3 & LASIF_HWND != 0 {
                layout.retype(2, Number);
            }
        }
        crate::opcode::EW_EXECUTE if v2 == 0 || v1 < 0 => {
            // Only a wait with a variable records the exit code.
            layout.clear(1);
        }
        crate::opcode::EW_REGISTERDLL if v2 == 0 => {
            // No status text: the function is a plugin's, called with the
            // variables, the stack and the flags (`extra_parameters`).
            layout.clear(2);
            layout.effects = layout.effects.of_plugin(FlagSet::ALL);
        }
        crate::opcode::EW_CREATESHORTCUT if v4 & CS_NWD != 0 => {
            // No working directory, so `$OUTDIR` is not read.
            layout.effects.outdir = None;
        }
        crate::opcode::EW_REBOOT if v0 != REBOOT_MAGIC => {
            // Anything but the magic is a corrupted installer.
            layout.effects = NONE.asking().with_termination(Termination::Always);
        }
        crate::opcode::EW_WRITEINI => {
            // A zero section or key is `NULL`, which deletes; the value is
            // read only when the fifth operand says to write one.
            if v0 == 0 {
                layout.clear(0);
            }
            if v1 == 0 {
                layout.clear(1);
            }
            if v4 == 0 {
                layout.clear(2);
            }
        }
        crate::opcode::EW_DELREG => {
            // A key deletion names no value.
            if v4 != 0 {
                layout.clear(3);
            }
            layout.effects = layout.effects.reading(shell_context(v1));
        }
        crate::opcode::EW_WRITEREG => {
            // The data slot is what the value kind says it is.
            match v4 {
                REG_SZ => layout.retype(3, String),
                REG_DWORD => layout.retype(3, Number),
                REG_BINARY => layout.retype(3, DataOffset),
                _ => layout.clear(3),
            }
            layout.effects = layout.effects.reading(shell_context(v0));
        }
        crate::opcode::EW_READREGSTR | crate::opcode::EW_REGENUM => {
            layout.effects = layout.effects.reading(shell_context(v1));
        }
        crate::opcode::EW_FPUTS | crate::opcode::EW_FPUTWS if v2 != 0 => {
            // FileWriteByte and FileWriteWord write a number.
            layout.retype(1, Number);
        }
        crate::opcode::EW_FSEEK if v1 < 0 => {
            // No variable for the new position.
            layout.clear(1);
        }
        crate::opcode::EW_SECTIONSET => section_form(&mut layout, v2),
        crate::opcode::EW_INSTTYPESET => inst_type_form(&mut layout, v2, v3),
        crate::opcode::EW_GETOSINFO => match v3 {
            GETOSINFO_KNOWNFOLDER => {
                layout.retype(2, String);
                layout.effects = FAILS;
            }
            GETOSINFO_READMEMORY => {
                // Address zero is the runtime's own flags and OS block, so the
                // read may see any flag.
                layout.set(2, "address", Number);
                layout.set(4, "spec", Number);
                layout.effects = NONE.reading(FlagSet::ALL);
            }
            _ => {
                layout.clear(1);
            }
        },
        crate::opcode::EW_LOG if v0 != 0 => {
            // LogSet toggles logging; only LogText carries a string.
            layout.set(1, "on_off", Int);
        }
        _ => {}
    }
    layout
}

/// The flags a registry root reads: `SHCTX`, stored as a small non-negative
/// value, opens the machine or the user hive as `SetShellVarContext` says
/// (`GetRegRootKey` in `exec.c`). A predefined key is stored as its negative
/// handle and reads nothing.
fn shell_context(root: i32) -> FlagSet {
    if root >= 0 {
        FlagSet::of(&[ExecFlag::AllUserVar])
    } else {
        FlagSet::EMPTY
    }
}

/// Resolves `EW_PUSHPOP`'s three commands, chosen by which slot carries
/// something. NSIS 1.x reports an `Exch` past the stack's end as a corrupted
/// installer without the silent-mode check a later message box makes, which
/// `v1` selects.
pub(crate) fn pushpop_form(layout: &mut ParamLayout, pop: i32, depth: i32, v1: bool) {
    layout.names = ["value", "", "", "", "", ""];
    layout.types = [String, Unused, Unused, Unused, Unused, Unused];
    layout.count = 1;
    layout.effects = NONE.with_stack(StackEffect::Push);
    if depth != 0 {
        // Exch takes neither a value nor a variable: the depth is the whole
        // operand. Past the stack's end it ends the installer.
        layout.names = ["", "", "index", "", "", ""];
        layout.types = [Unused, Unused, Int, Unused, Unused, Unused];
        layout.count = 3;
        let effects = NONE
            .with_stack(StackEffect::Exch)
            .with_termination(Termination::May);
        layout.effects = if v1 { effects } else { effects.asking() };
    } else if pop != 0 {
        // Pop writes into a variable, when the stack has a string to pop.
        layout.names = ["var", "op", "", "", "", ""];
        layout.types = [MAY_OUT, Int, Unused, Unused, Unused, Unused];
        layout.count = 2;
        layout.effects = FAILS.with_stack(StackEffect::Pop);
    }
}

/// Resolves `EW_SECTIONSET`'s forms: the operation doubles as the direction,
/// negative for a set, and the property it names decides which slot the value
/// lives in.
fn section_form(layout: &mut ParamLayout, op: i32) {
    // Widened, because -1 is the first set operation and i32::MIN has no
    // positive counterpart to negate towards.
    let op = i64::from(op);
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
    layout.types = [Number, Unused, Int, Unused, Unused, Unused];
    layout.count = 3;
    match (sets, property) {
        // SectionSetText expands its text from the fifth slot into the
        // section, and stores the second as the section's name offset. The
        // compiler always leaves it zero, so it is unnamed: an operand only
        // when some other compiler put something there.
        (true, 0) => {
            layout.set(1, "", Int);
            layout.set(3, "flags_changed", Int);
            layout.set(4, name, String);
            layout.count = 5;
        }
        (true, _) => {
            layout.set(1, name, Number);
            layout.set(3, "flags_changed", Int);
            layout.count = 4;
        }
        // A get writes nothing for a section index out of range.
        (false, _) => layout.set(1, name, MAY_OUT),
    }
}

/// Resolves `EW_INSTTYPESET`'s four commands, selected by two flags.
fn inst_type_form(layout: &mut ParamLayout, writes: i32, current: i32) {
    layout.names = ["inst_type", "", "set", "current", "", ""];
    layout.types = [Number, Unused, Int, Int, Unused, Unused];
    layout.count = 4;
    match (current != 0, writes != 0) {
        // InstTypeGetText expands the text stored for the install type, so it
        // reads whatever variables that text embeds.
        (false, false) => {
            layout.set(1, "text", MAY_OUT);
            layout.effects.hidden = HiddenVariables::ReadsAny;
        }
        // InstTypeSetText stores the offset, unexpanded.
        (false, true) => layout.set(1, "text", RawString),
        // GetCurInstType
        (true, false) => layout.set(1, "output", MAY_OUT),
        // SetCurInstType
        (true, true) => {}
    }
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
    /// What the instruction does beyond its slots, in its fixed form.
    pub effects: Effects,
    /// Brief description of what the opcode does.
    pub description: &'static str,
    /// Semantic category (e.g., `"file"`, `"registry"`, `"string"`, `"flow"`).
    pub category: &'static str,
}

/// `EW_SETBRANDINGIMAGE`, which NSIS 2 stores where NSIS 3 stores
/// `EW_LOADANDSETIMAGE`.
///
/// NSIS 3 generalized the instruction and kept its number, so opcode 37 means
/// this in an NSIS 2 or Park installer: an image file loaded into a control of
/// the installer's window.
pub static SET_BRANDING_IMAGE: OpcodeInfo = OpcodeInfo {
    mnemonic: "EW_SETBRANDINGIMAGE",
    param_count: 3,
    param_names: ["image", "control", "resize", "", "", ""],
    param_types: [String, Int, Int, Unused, Unused, Unused],
    effects: NONE,
    description: "SetBrandingImage",
    category: "ui",
};

/// The opcode table.
///
/// Indices are the `which` field of an entry, in the layout a standard
/// makensis produces - NSIS 2 and NSIS 3 number their instructions the same
/// way, and differ in what one of them does ([`SET_BRANDING_IMAGE`]). What
/// varies is which instructions a build *has*: a build compiled with logging
/// adds one, the Park fork adds three, and the two UTF-16 file commands exist
/// only in a Unicode build. Those shift the stored numbering, which
/// [`normalize_log_opcode`](super::normalize_log_opcode) and
/// [`normalize_park_opcode`](super::normalize_park_opcode) undo before lookup,
/// so a caller sees one numbering whatever produced the file.
///
/// Parameter counts are the maximum an opcode has taken across NSIS versions,
/// matching 7-Zip and Binary Refinery: older releases passed operands that
/// newer ones dropped, and a count that is too low makes a valid entry look
/// malformed.
pub static OPCODES: [OpcodeInfo; 74] = [
    OpcodeInfo {
        mnemonic: "EW_INVALID_OPCODE",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Invalid opcode; the runtime skips it",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_RET",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Return from function",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_NOP",
        param_count: 1,
        param_names: ["jump_addr", "", "", "", "", ""],
        param_types: [Jump, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "No-op / Jump",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_ABORT",
        param_count: 1,
        param_names: ["status_text", "", "", "", "", ""],
        param_types: [String, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.reporting().with_termination(Termination::Always),
        description: "Abort installation",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_QUIT",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.with_termination(Termination::Always),
        description: "Quit installer",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_CALL",
        param_count: 2,
        // The compiler marks `Call :label` with a 1 the runtime never reads.
        param_names: ["address", "label_call", "", "", "", ""],
        param_types: [Jump, Int, Unused, Unused, Unused, Unused],
        // A function that aborts ends its caller too.
        effects: NONE.with_termination(Termination::May),
        description: "Call subroutine",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_UPDATETEXT",
        param_count: 6,
        // NSIS 2 and 3 read only the text; the compiler writes zero after it.
        param_names: ["text", "", "", "", "", ""],
        param_types: [String, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.reporting(),
        description: "Update status text",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SLEEP",
        param_count: 1,
        param_names: ["milliseconds", "", "", "", "", ""],
        param_types: [Number, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Sleep",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_BRINGTOFRONT",
        param_count: 0,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Bring window to front",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_CHDETAILSVIEW",
        param_count: 2,
        // `ShowWindow` commands for the details list and its button.
        param_names: ["list_show", "button_show", "", "", "", ""],
        param_types: [Int, Int, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Set details view",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SETFILEATTRIBUTES",
        param_count: 2,
        param_names: ["file", "attributes", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: FAILS,
        description: "Set file attributes",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATEDIR",
        param_count: 3,
        param_names: ["path", "set_outdir", "restrict", "", "", ""],
        param_types: [String, Int, Int, Unused, Unused, Unused],
        effects: FAILS_REPORTING,
        description: "Create directory",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_IFFILEEXISTS",
        param_count: 3,
        param_names: ["file", "jump_yes", "jump_no", "", "", ""],
        param_types: [String, Jump, Jump, Unused, Unused, Unused],
        effects: NONE,
        description: "If file exists",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_SETFLAG",
        param_count: 4,
        // The mode: zero sets the flag and saves its old value, negative sets
        // it and saves the old value apart, positive restores a saved one.
        param_names: ["flag", "value", "mode", "", "", ""],
        param_types: [Flag(Access::Write), Number, Int, Unused, Unused, Unused],
        effects: NONE,
        description: "Set exec flag",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_IFFLAG",
        param_count: 4,
        // Tests the flag, then keeps only the mask's bits of it: `IfErrors`
        // clears the error flag as it tests it.
        param_names: ["jump_set", "jump_clear", "flag", "mask", "", ""],
        param_types: [Jump, Jump, Flag(Access::Update), Int, Unused, Unused],
        effects: NONE,
        description: "If flag set",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFLAG",
        param_count: 2,
        param_names: ["output", "flag", "", "", "", ""],
        param_types: [OUT, Flag(Access::Read), Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Get exec flag",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_RENAME",
        param_count: 4,
        param_names: ["old", "new", "rebootok", "status_text", "", ""],
        param_types: [String, String, Int, String, Unused, Unused],
        effects: FAILS_REPORTING,
        description: "Rename/move file",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFULLPATHNAME",
        param_count: 3,
        // Zero asks for the short name.
        param_names: ["input", "output", "long_name", "", "", ""],
        param_types: [String, OUT, Int, Unused, Unused, Unused],
        effects: FAILS,
        description: "Get full path name",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_SEARCHPATH",
        param_count: 2,
        param_names: ["output", "filename", "", "", "", ""],
        param_types: [OUT, String, Unused, Unused, Unused, Unused],
        effects: FAILS,
        description: "Search PATH",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETTEMPFILENAME",
        param_count: 2,
        param_names: ["output", "basedir", "", "", "", ""],
        param_types: [OUT, String, Unused, Unused, Unused, Unused],
        effects: FAILS,
        description: "Get temp filename",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_EXTRACTFILE",
        param_count: 6,
        // The flags pack the overwrite mode (low three bits) and the error
        // box's buttons (above). The error text is expanded with `$0` holding
        // the file's path, and `$0` restored after.
        param_names: [
            "flags",
            "name",
            "data_offset",
            "date_lo",
            "date_hi",
            "error_text",
        ],
        param_types: [Int, String, DataOffset, Int, Int, String],
        effects: FAILS_REPORTING
            .asking()
            .with_outdir(Access::Read)
            .with_termination(Termination::May),
        description: "Extract file from archive",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_DELETEFILE",
        param_count: 2,
        param_names: ["filename", "flags", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: FAILS_REPORTING,
        description: "Delete file",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_MESSAGEBOX",
        param_count: 6,
        param_names: ["mb_flags", "text", "button1", "jump1", "button2", "jump2"],
        param_types: [Int, String, Int, Jump, Int, Jump],
        effects: FAILS.asking(),
        description: "Message box",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_RMDIR",
        param_count: 2,
        param_names: ["path", "flags", "", "", "", ""],
        param_types: [String, Int, Unused, Unused, Unused, Unused],
        effects: FAILS_REPORTING,
        description: "Remove directory",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_STRLEN",
        param_count: 2,
        param_names: ["output", "input", "", "", "", ""],
        param_types: [OUT, String, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "String length",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_ASSIGNVAR",
        param_count: 4,
        // The source is expanded before the variable is cleared, so
        // `StrCpy $0 "$0x"` reads the old `$0`.
        param_names: ["var", "string", "max_len", "start", "", ""],
        param_types: [OUT, String, Number, Number, Unused, Unused],
        effects: NONE,
        description: "StrCpy",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_STRCMP",
        param_count: 5,
        param_names: ["s1", "s2", "jump_eq", "jump_neq", "case_sensitive", ""],
        param_types: [String, String, Jump, Jump, Int, Unused],
        effects: NONE,
        description: "String compare",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_READENVSTR",
        param_count: 3,
        param_names: ["output", "string", "is_read", "", "", ""],
        param_types: [OUT, String, Int, Unused, Unused, Unused],
        effects: FAILS,
        description: "ReadEnvStr/ExpandEnvStrings",
        category: "string",
    },
    OpcodeInfo {
        mnemonic: "EW_INTCMP",
        param_count: 6,
        param_names: ["v1", "v2", "jump_eq", "jump_lt", "jump_gt", "flags"],
        param_types: [Number, Number, Jump, Jump, Jump, Int],
        effects: NONE,
        description: "Integer compare",
        category: "flow",
    },
    OpcodeInfo {
        mnemonic: "EW_INTOP",
        param_count: 4,
        param_names: ["output", "input1", "input2", "op", "", ""],
        param_types: [OUT, Number, Number, Int, Unused, Unused],
        effects: NONE,
        description: "Integer operation",
        category: "math",
    },
    OpcodeInfo {
        mnemonic: "EW_INTFMT",
        param_count: 4,
        param_names: ["output", "format", "input", "is_64bit", "", ""],
        param_types: [OUT, String, Number, Int, Unused, Unused],
        effects: NONE,
        description: "IntFmt/Int64Fmt",
        category: "math",
    },
    OpcodeInfo {
        mnemonic: "EW_PUSHPOP",
        param_count: 6,
        // One opcode, three commands; `param_layout` says which.
        param_names: ["value", "pop", "exch", "", "", ""],
        param_types: [String, Int, Int, Unused, Unused, Unused],
        effects: NONE.with_stack(StackEffect::Push),
        description: "Push/Pop/Exch",
        category: "stack",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDWINDOW",
        param_count: 6,
        param_names: ["output", "class", "title", "parent", "after", "flags"],
        param_types: [OUT, String, String, Number, Number, Int],
        effects: NONE,
        description: "FindWindow",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SENDMESSAGE",
        param_count: 6,
        // The flags' low bits mark string message parameters; the rest is a
        // timeout.
        param_names: ["output", "hwnd", "msg", "wparam", "lparam", "flags"],
        param_types: [OUT, Number, Number, Number, Number, Int],
        effects: NONE,
        description: "SendMessage",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_ISWINDOW",
        param_count: 3,
        param_names: ["hwnd", "jump_yes", "jump_no", "", "", ""],
        param_types: [Number, Jump, Jump, Unused, Unused, Unused],
        effects: NONE,
        description: "IsWindow",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_GETDLGITEM",
        param_count: 3,
        param_names: ["output", "dialog", "item_id", "", "", ""],
        param_types: [OUT, Number, Number, Unused, Unused, Unused],
        effects: NONE,
        description: "GetDlgItem",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SETCTLCOLORS",
        param_count: 2,
        // A byte offset into the installer's control-colors block.
        param_names: ["hwnd", "colors", "", "", "", ""],
        param_types: [Number, Int, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Set control colors",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_LOADANDSETIMAGE",
        param_count: 4,
        // The flags say whether the image is a string or a resource id, and
        // whether the control is a window handle or a dialog item id.
        param_names: ["output", "image", "control", "flags", "", ""],
        param_types: [OUT, Int, Int, Int, Unused, Unused],
        effects: NONE,
        description: "Load and set image",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATEFONT",
        param_count: 5,
        param_names: ["output", "face", "height", "weight", "flags", ""],
        param_types: [OUT, String, Number, Number, Int, Unused],
        effects: NONE,
        description: "CreateFont",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SHOWWINDOW",
        param_count: 4,
        param_names: ["hwnd", "show_state", "hide", "enable", "", ""],
        param_types: [Number, Number, Int, Int, Unused, Unused],
        effects: NONE,
        description: "ShowWindow",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_SHELLEXEC",
        param_count: 6,
        // The mask is `SEE_MASK_*`; `SEE_MASK_NOCLOSEPROCESS` waits.
        param_names: ["verb", "file", "params", "show", "mask", "status_text"],
        param_types: [String, String, String, Int, Int, String],
        effects: FAILS_REPORTING.with_outdir(Access::Read),
        description: "ShellExecute",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_EXECUTE",
        param_count: 3,
        // The exit code is written once the process ends, when it started.
        param_names: ["command", "exit_code", "wait", "", "", ""],
        param_types: [String, MAY_OUT, Int, Unused, Unused, Unused],
        effects: FAILS_REPORTING,
        description: "Exec/ExecWait",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFILETIME",
        param_count: 3,
        param_names: ["hi_out", "lo_out", "file", "", "", ""],
        param_types: [OUT, OUT, String, Unused, Unused, Unused],
        effects: FAILS,
        description: "GetFileTime",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETDLLVERSION",
        param_count: 4,
        param_names: ["hi_out", "lo_out", "file", "kind", "", ""],
        param_types: [OUT, OUT, String, Int, Unused, Unused],
        effects: FAILS,
        description: "GetDLLVersion",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_REGISTERDLL",
        param_count: 6,
        // A status text (usually a language string) marks `RegDLL`; zero, a
        // plugin call.
        param_names: [
            "dll",
            "function",
            "status_text",
            "no_unload",
            "use_loaded",
            "",
        ],
        param_types: [String, String, String, Int, Int, Unused],
        effects: FAILS_REPORTING,
        description: "RegisterDLL/plugin call",
        category: "exec",
    },
    OpcodeInfo {
        mnemonic: "EW_CREATESHORTCUT",
        param_count: 6,
        // The packed word holds the icon index, show command, hotkey and the
        // no-working-directory flag.
        param_names: ["link", "target", "params", "icon", "packed", "description"],
        param_types: [String, String, String, String, Int, String],
        effects: FAILS_REPORTING.with_outdir(Access::Read),
        description: "CreateShortcut",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_COPYFILES",
        param_count: 4,
        param_names: ["source", "dest", "flags", "status_text", "", ""],
        param_types: [String, String, Int, String, Unused, Unused],
        effects: FAILS_REPORTING,
        description: "CopyFiles",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_REBOOT",
        param_count: 1,
        param_names: ["magic", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        effects: NONE.writing(FlagSet::of(&[ExecFlag::RebootCalled])),
        description: "Reboot",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEINI",
        param_count: 5,
        param_names: ["section", "name", "value", "ini_file", "write", ""],
        param_types: [String, String, String, String, Int, Unused],
        effects: FAILS,
        description: "WriteINIStr",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_READINISTR",
        param_count: 4,
        param_names: ["output", "section", "name", "ini_file", "", ""],
        param_types: [OUT, String, String, String, Unused, Unused],
        effects: FAILS,
        description: "ReadINIStr",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_DELREG",
        param_count: 5,
        // Zero flags delete a value; otherwise a key, with the flags above
        // the lowest bit.
        param_names: ["", "root", "keyname", "valuename", "flags", ""],
        param_types: [Unused, Int, String, String, Int, Unused],
        effects: REGISTRY,
        description: "DeleteRegValue/Key",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEREG",
        param_count: 6,
        // The kind decides the data slot; the value type is the `REG_*` the
        // value gets.
        param_names: ["root", "keyname", "itemname", "data", "kind", "value_type"],
        param_types: [Int, String, String, String, Int, Int],
        effects: REGISTRY,
        description: "WriteRegStr/DWORD/Bin",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_READREGSTR",
        param_count: 5,
        param_names: ["output", "root", "keyname", "itemname", "want_dword", ""],
        param_types: [OUT, Int, String, String, Int, Unused],
        effects: REGISTRY,
        description: "ReadRegStr/DWORD",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_REGENUM",
        param_count: 5,
        param_names: ["output", "root", "keyname", "index", "enum_keys", ""],
        param_types: [OUT, Int, String, Number, Int, Unused],
        effects: REGISTRY,
        description: "RegEnumKey/Value",
        category: "registry",
    },
    OpcodeInfo {
        mnemonic: "EW_FCLOSE",
        param_count: 1,
        param_names: ["handle", "", "", "", "", ""],
        param_types: [IN, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "FileClose",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FOPEN",
        param_count: 4,
        param_names: ["handle_out", "openmode", "createmode", "name", "", ""],
        param_types: [OUT, Int, Int, String, Unused, Unused],
        effects: FAILS,
        description: "FileOpen",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FPUTS",
        param_count: 3,
        param_names: ["handle", "string", "write_char", "", "", ""],
        param_types: [IN, String, Int, Unused, Unused, Unused],
        effects: FAILS,
        description: "FileWrite",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FGETS",
        param_count: 4,
        // Nothing is written when asked for fewer than one character.
        param_names: ["handle", "output", "max_len", "read_char", "", ""],
        param_types: [IN, MAY_OUT, Number, Int, Unused, Unused],
        effects: FAILS,
        description: "FileRead",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FSEEK",
        param_count: 4,
        // The position is written only when the handle is not empty.
        param_names: ["handle", "position", "offset", "method", "", ""],
        param_types: [IN, MAY_OUT, Number, Int, Unused, Unused],
        effects: NONE,
        description: "FileSeek",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDCLOSE",
        param_count: 1,
        param_names: ["handle", "", "", "", "", ""],
        param_types: [IN, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "FindClose",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDNEXT",
        param_count: 2,
        param_names: ["output", "handle", "", "", "", ""],
        param_types: [OUT, IN, Unused, Unused, Unused, Unused],
        effects: FAILS,
        description: "FindNext",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FINDFIRST",
        param_count: 3,
        param_names: ["output", "handle_out", "filespec", "", "", ""],
        param_types: [OUT, OUT, String, Unused, Unused, Unused],
        effects: FAILS,
        description: "FindFirst",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_WRITEUNINSTALLER",
        param_count: 4,
        // The full path is the compiler's `$INSTDIR\<name>`, used when the
        // name alone is not a full path.
        param_names: ["name", "data_offset", "icon_size", "full_path", "", ""],
        param_types: [String, DataOffset, Int, String, Unused, Unused],
        effects: FAILS_REPORTING,
        description: "WriteUninstaller",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_SECTIONSET",
        param_count: 5,
        // One opcode, several commands; `param_layout` says which.
        param_names: ["section", "value", "op", "flags_changed", "text", ""],
        param_types: [Number, Number, Int, Int, String, Unused],
        effects: FAILS,
        description: "SectionSet/GetText/Flags",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_INSTTYPESET",
        param_count: 4,
        // One opcode, four commands; `param_layout` says which.
        param_names: ["inst_type", "value", "set", "current", "", ""],
        param_types: [Number, Unused, Int, Int, Unused, Unused],
        effects: FAILS,
        description: "InstTypeSet/GetFlags",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_GETOSINFO",
        param_count: 6,
        // The operation says what the source is: a known-folder id, or an
        // address and a size-and-offset spec to read memory with.
        param_names: ["", "output", "source", "op", "spec", ""],
        param_types: [Unused, OUT, String, Int, Unused, Unused],
        effects: FAILS,
        description: "GetOSInfo/GetKnownFolderPath",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_RESERVEDOPCODE",
        param_count: 2,
        param_names: ["", "", "", "", "", ""],
        param_types: [Unused, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Reserved/free slot",
        category: "misc",
    },
    OpcodeInfo {
        mnemonic: "EW_LOCKWINDOW",
        param_count: 1,
        // Zero is `LockWindow on`.
        param_names: ["unlock", "", "", "", "", ""],
        param_types: [Int, Unused, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "Lock/unlock window updates",
        category: "ui",
    },
    OpcodeInfo {
        mnemonic: "EW_FPUTWS",
        param_count: 4,
        param_names: ["handle", "string", "write_char", "bom", "", ""],
        param_types: [IN, String, Int, Int, Unused, Unused],
        effects: FAILS,
        description: "FileWriteUTF16LE",
        category: "file_io",
    },
    OpcodeInfo {
        mnemonic: "EW_FGETWS",
        param_count: 4,
        param_names: ["handle", "output", "max_len", "read_char", "", ""],
        param_types: [IN, MAY_OUT, Number, Int, Unused, Unused],
        effects: FAILS,
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
        effects: NONE,
        description: "LogText/LogSet (log-enabled builds only)",
        category: "misc",
    },
    // The Park fork's own instructions, from its `exec.c` (2.46.3).
    OpcodeInfo {
        mnemonic: "EW_FINDPROC",
        param_count: 2,
        param_names: ["result", "process", "", "", "", ""],
        param_types: [OUT, String, Unused, Unused, Unused, Unused],
        effects: NONE,
        description: "FindProc (Park fork)",
        category: "process",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFONTVERSION",
        param_count: 2,
        // The version is written by the font reader, which may leave it on
        // failure.
        param_names: ["output", "file", "", "", "", ""],
        param_types: [MAY_OUT, String, Unused, Unused, Unused, Unused],
        effects: FAILS,
        description: "GetFontVersion (Park fork)",
        category: "file",
    },
    OpcodeInfo {
        mnemonic: "EW_GETFONTNAME",
        param_count: 2,
        param_names: ["output", "file", "", "", "", ""],
        param_types: [MAY_OUT, String, Unused, Unused, Unused, Unused],
        effects: FAILS,
        description: "GetFontName (Park fork)",
        category: "file",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcode::{
        EW_CREATEDIR, EW_DELREG, EW_EXECUTE, EW_FINDWINDOW, EW_FSEEK, EW_GETOSINFO, EW_IFFLAG,
        EW_INSTTYPESET, EW_INTOP, EW_LOADANDSETIMAGE, EW_LOG, EW_PUSHPOP, EW_REBOOT,
        EW_REGISTERDLL, EW_SECTIONSET, EW_SENDMESSAGE, EW_SETFLAG, EW_WRITEINI, EW_WRITEREG,
        lookup, lookup_for,
    };

    /// Resolves the NSIS 3 layout for an instruction with the given operands.
    fn layout_of(which: i32, values: [i32; 6]) -> ParamLayout {
        layout_in(which, NsisVersion::V3, values)
    }

    /// Resolves the layout for an instruction as an installer of `version`
    /// means it.
    fn layout_in(which: i32, version: NsisVersion, values: [i32; 6]) -> ParamLayout {
        let info = lookup_for(which as u32, version).expect("opcode should be known");
        param_layout(which as u32, version, info, &values)
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

    /// The type of slot `index`.
    fn kind(layout: &ParamLayout, index: usize) -> ParamType {
        layout.types[index]
    }

    #[test]
    fn log_set_does_not_read_its_flag_as_a_string() {
        // LogText carries a string; LogSet carries on/off in the same slot.
        // Reading that flag as a string offset resolves it to whatever text
        // sits at offset 1, which is how `LogSet on` rendered as
        // `text="ProgramFilesDir"`.
        let text = layout_of(EW_LOG, [0, 42, 0, 0, 0, 0]);
        assert_eq!(slots(&text), [("type", Int), ("text", String)]);

        let set = layout_of(EW_LOG, [1, 1, 0, 0, 0, 0]);
        assert_eq!(slots(&set), [("type", Int), ("on_off", Int)]);
    }

    #[test]
    fn section_get_may_write_a_variable() {
        // Operation >= 0 reads; the second slot is the destination variable,
        // written only when the section index is in range.
        let get = layout_of(EW_SECTIONSET, [10, 0, 0, 0, 0, 0]);
        assert_eq!(
            slots(&get),
            [("section", Number), ("text", MAY_OUT), ("op", Int)]
        );

        // The operation names the property being read.
        let flags = layout_of(EW_SECTIONSET, [10, 0, 2, 0, 0, 0]);
        assert_eq!(slots(&flags)[1], ("flags", MAY_OUT));
    }

    #[test]
    fn section_set_text_keeps_its_text_in_the_fifth_slot() {
        // SectionSetText expands its text from slot 4, and stores slot 1 - the
        // compiler's zero, so unnamed - as the section's name offset.
        let set_text = layout_of(EW_SECTIONSET, [10, 0, -1, 0, 42, 0]);
        assert_eq!(
            slots(&set_text),
            [
                ("section", Number),
                ("", Int),
                ("op", Int),
                ("flags_changed", Int),
                ("text", String),
            ]
        );

        // Every other set operation reads its value, as a number, from slot 1.
        let set_flags = layout_of(EW_SECTIONSET, [10, 42, -3, 1, 0, 0]);
        assert_eq!(slots(&set_flags)[1], ("flags", Number));
    }

    #[test]
    fn section_op_out_of_range_still_renders() {
        // A property number this crate has no name for must not be dropped or
        // panic the renderer; NSIS gained operations over time.
        let unknown = layout_of(EW_SECTIONSET, [10, 42, -99, 0, 0, 0]);
        assert_eq!(slots(&unknown)[1], ("value", Number));

        // i32::MIN has no positive counterpart, so negating it would overflow.
        // It names no property, so it renders as a plain set.
        let extreme = layout_of(EW_SECTIONSET, [10, 42, i32::MIN, 0, 0, 0]);
        assert_eq!(slots(&extreme)[1], ("value", Number));
    }

    #[test]
    fn inst_type_set_selects_between_four_commands() {
        let cases = [
            // (current, set) -> the slot-1 rendering of each command
            ([0, 7, 0, 0, 0, 0], ("text", MAY_OUT)), // InstTypeGetText
            ([0, 7, 1, 0, 0, 0], ("text", RawString)), // InstTypeSetText
            ([0, 7, 0, 1, 0, 0], ("output", MAY_OUT)), // GetCurInstType
            ([0, 0, 1, 1, 0, 0], ("", Unused)),      // SetCurInstType
        ];
        for (values, expected) in cases {
            let layout = layout_of(EW_INSTTYPESET, values);
            assert_eq!(slots(&layout)[1], expected, "operands {values:?}");
            // The index is read and range-checked by every form.
            assert_eq!(slots(&layout)[0], ("inst_type", Number));
        }

        // InstTypeGetText expands the stored text, which may embed any
        // variable; InstTypeSetText stores it unexpanded.
        let get_text = layout_of(EW_INSTTYPESET, [0, 7, 0, 0, 0, 0]);
        assert_eq!(get_text.effects.hidden, HiddenVariables::ReadsAny);
        let set_text = layout_of(EW_INSTTYPESET, [0, 7, 1, 0, 0, 0]);
        assert_eq!(set_text.effects.hidden, HiddenVariables::None);
    }

    /// **`EW_PUSHPOP` is three commands, and the layout says which.**
    #[test]
    fn pushpop_states_which_of_its_three_commands_an_entry_is() {
        let push = layout_of(EW_PUSHPOP, [42, 0, 0, 0, 0, 0]);
        assert_eq!(slots(&push), [("value", String)]);
        assert_eq!(push.effects.stack, StackEffect::Push);

        // Pop may write: an empty stack sets the error flag and leaves the
        // variable alone.
        let pop = layout_of(EW_PUSHPOP, [3, 1, 0, 0, 0, 0]);
        assert_eq!(slots(&pop)[0], ("var", MAY_OUT));
        assert_eq!(pop.effects.stack, StackEffect::Pop);
        assert!(pop.effects.flags_written.contains(ExecFlag::ExecError));

        // Exch: the depth is the whole operand, and a short stack ends the
        // installer.
        let exch = layout_of(EW_PUSHPOP, [0, 0, 2, 0, 0, 0]);
        assert_eq!(slots(&exch)[0], ("", Unused));
        assert_eq!(slots(&exch)[2], ("index", Int));
        assert_eq!(exch.effects.stack, StackEffect::Exch);
        assert_eq!(exch.effects.terminates, Termination::May);
    }

    /// **`ExecWait`'s exit code is slot 1 and its wait flag slot 2** - the
    /// order NSIS 1.x used the other way round (`exec.c` 944-948).
    #[test]
    fn exec_wait_writes_its_exit_code_from_slot_one() {
        let exec_wait = layout_of(EW_EXECUTE, [7, 2, 1, 0, 0, 0]);
        assert_eq!(
            slots(&exec_wait),
            [("command", String), ("exit_code", MAY_OUT), ("wait", Int)]
        );
        // Without a variable (-1), or without waiting, nothing is written.
        for values in [[7, -1, 1, 0, 0, 0], [7, 0, 0, 0, 0, 0]] {
            assert_eq!(kind(&layout_of(EW_EXECUTE, values), 1), Unused);
        }
    }

    #[test]
    fn a_flag_instruction_names_the_flag_it_uses() {
        // SetErrors: flag 2 set from a number.
        let set = layout_of(EW_SETFLAG, [2, 5, 0, 0, 0, 0]);
        assert_eq!(
            slots(&set)[..3],
            [
                ("flag", Flag(Access::Write)),
                ("value", Number),
                ("mode", Int)
            ]
        );
        // The restore a plugin call ends with writes a saved value: four
        // operands, and none of them the value.
        let restore = layout_of(EW_SETFLAG, [13, 0, 1, -1, 0, 0]);
        assert_eq!(kind(&restore, 1), Unused);
        assert_eq!(slots(&restore)[3], ("saved_status", Int));

        // IfErrors masks with zero: it reads and clears. A full mask keeps it.
        assert_eq!(
            kind(&layout_of(EW_IFFLAG, [5, 0, 2, 0, 0, 0]), 2),
            Flag(Access::Update)
        );
        assert_eq!(
            kind(&layout_of(EW_IFFLAG, [5, 0, 8, -1, 0, 0]), 2),
            Flag(Access::Read)
        );
    }

    #[test]
    fn window_messages_write_only_a_variable_they_have() {
        // SendMessage with no output stores -1.
        let without = layout_of(EW_SENDMESSAGE, [-1, 1, 2, 3, 4, 0]);
        assert_eq!(kind(&without, 0), Unused);
        let with = layout_of(EW_SENDMESSAGE, [8, 1, 2, 3, 4, 0]);
        assert_eq!(kind(&with, 0), OUT);
        // `STR:` parameters are strings; the rest numbers.
        let strings = layout_of(EW_SENDMESSAGE, [8, 1, 2, 3, 4, 3]);
        assert_eq!((kind(&strings, 3), kind(&strings, 4)), (String, String));
        assert_eq!((kind(&with, 3), kind(&with, 4)), (Number, Number));
        // Only a timeout can fail.
        assert!(!with.effects.flags_written.contains(ExecFlag::ExecError));
        let timeout = layout_of(EW_SENDMESSAGE, [8, 1, 2, 3, 4, 100 << 2]);
        assert!(timeout.effects.flags_written.contains(ExecFlag::ExecError));
        assert_eq!(
            kind(&layout_of(EW_FINDWINDOW, [-1, 1, 2, 0, 0, 0]), 0),
            Unused
        );
    }

    /// **Opcode 37 is two instructions**: NSIS 3's `LoadAndSetImage` keeps
    /// an optional output variable in slot 0, where NSIS 2's
    /// `SetBrandingImage` keeps the image path.
    #[test]
    fn opcode_37_is_what_the_version_says() {
        let v3 = layout_of(EW_LOADANDSETIMAGE, [-1, 5, 1200, 0, 0, 0]);
        assert_eq!(
            slots(&v3),
            [
                ("", Unused),
                ("image", Int),
                ("control", Int),
                ("flags", Int)
            ]
        );
        let by_path = layout_of(
            EW_LOADANDSETIMAGE,
            [9, 5, 7, LASIF_STRID | LASIF_HWND, 0, 0],
        );
        assert_eq!(
            slots(&by_path),
            [
                ("output", OUT),
                ("image", String),
                ("control", Number),
                ("flags", Int)
            ]
        );

        for version in [NsisVersion::V2, NsisVersion::Park] {
            let v2 = layout_in(EW_LOADANDSETIMAGE, version, [42, 1046, 0, 0, 0, 0]);
            assert_eq!(
                slots(&v2),
                [("image", String), ("control", Int), ("resize", Int)]
            );
        }
        assert_eq!(lookup(37).map(|op| op.mnemonic), Some("EW_LOADANDSETIMAGE"));
    }

    #[test]
    fn registry_data_is_what_the_kind_says() {
        let string = layout_of(EW_WRITEREG, [-2147483647, 1, 2, 3, REG_SZ, 1]);
        let dword = layout_of(EW_WRITEREG, [-2147483647, 1, 2, 3, REG_DWORD, 4]);
        let binary = layout_of(EW_WRITEREG, [-2147483647, 1, 2, 3, REG_BINARY, 3]);
        assert_eq!(kind(&string, 3), String);
        assert_eq!(kind(&dword, 3), Number);
        assert_eq!(kind(&binary, 3), DataOffset);
        // A predefined root reads no flag; SHCTX (0) reads the shell context.
        assert!(!string.effects.flags_read.contains(ExecFlag::AllUserVar));
        let shctx = layout_of(EW_WRITEREG, [0, 1, 2, 3, REG_SZ, 1]);
        assert!(shctx.effects.flags_read.contains(ExecFlag::AllUserVar));
        assert!(shctx.effects.flags_read.contains(ExecFlag::AlterRegView));

        // DeleteRegKey names no value.
        assert_eq!(
            kind(&layout_of(EW_DELREG, [0, -2147483647, 5, 6, 2, 0]), 3),
            Unused
        );
        assert_eq!(
            kind(&layout_of(EW_DELREG, [0, -2147483647, 5, 6, 0, 0]), 3),
            String
        );
    }

    #[test]
    fn an_ini_write_reads_only_what_it_writes() {
        // DeleteINISec: no key, no value.
        let delete_section = layout_of(EW_WRITEINI, [4, 0, 0, 9, 0, 0]);
        assert_eq!(
            (kind(&delete_section, 1), kind(&delete_section, 2)),
            (Unused, Unused)
        );
        let write = layout_of(EW_WRITEINI, [4, 5, 6, 9, 1, 0]);
        assert_eq!((kind(&write, 1), kind(&write, 2)), (String, String));
    }

    #[test]
    fn a_plugin_call_touches_everything() {
        let plugin = layout_of(EW_REGISTERDLL, [1, 2, 0, 0, 1, 0]);
        assert!(plugin.effects.plugin);
        assert_eq!(plugin.effects.hidden, HiddenVariables::Any);
        assert_eq!(plugin.effects.flags_written, FlagSet::ALL);
        assert_eq!(kind(&plugin, 2), Unused);

        let reg_dll = layout_of(EW_REGISTERDLL, [1, 2, -33, 0, 0, 0]);
        assert!(!reg_dll.effects.plugin);
        assert_eq!(kind(&reg_dll, 2), String);
    }

    #[test]
    fn set_out_path_writes_outdir_and_create_directory_does_not() {
        let set_out_path = layout_of(EW_CREATEDIR, [5, 1, 0, 0, 0, 0]);
        assert_eq!(set_out_path.effects.outdir, Some(Access::Write));
        let create = layout_of(EW_CREATEDIR, [5, 0, 0, 0, 0, 0]);
        assert_eq!(create.effects.outdir, None);
    }

    #[test]
    fn only_division_sets_the_error_flag() {
        let add = layout_of(EW_INTOP, [1, 2, 3, 0, 0, 0]);
        let divide = layout_of(EW_INTOP, [1, 2, 3, 3, 0, 0]);
        let modulo = layout_of(EW_INTOP, [1, 2, 3, 10, 0, 0]);
        assert!(!add.effects.flags_written.contains(ExecFlag::ExecError));
        assert!(divide.effects.flags_written.contains(ExecFlag::ExecError));
        assert!(modulo.effects.flags_written.contains(ExecFlag::ExecError));
    }

    #[test]
    fn get_os_info_reads_what_its_operation_says() {
        let known_folder = layout_of(EW_GETOSINFO, [0, 3, 7, 0, 0, 0]);
        assert_eq!(
            slots(&known_folder)[1..4],
            [("output", OUT), ("source", String), ("op", Int)]
        );
        let memory = layout_of(EW_GETOSINFO, [0, 3, 7, 1, 8, 0]);
        assert_eq!((kind(&memory, 2), kind(&memory, 4)), (Number, Number));
        assert_eq!(memory.effects.flags_read, FlagSet::ALL);
    }

    #[test]
    fn a_seek_without_a_variable_writes_none() {
        assert_eq!(kind(&layout_of(EW_FSEEK, [1, -1, 5, 0, 0, 0]), 1), Unused);
        assert_eq!(kind(&layout_of(EW_FSEEK, [1, 3, 5, 0, 0, 0]), 1), MAY_OUT);
    }

    #[test]
    fn reboot_without_its_magic_ends_the_installer() {
        let reboot = layout_of(EW_REBOOT, [REBOOT_MAGIC, 0, 0, 0, 0, 0]);
        assert_eq!(reboot.effects.terminates, Termination::Never);
        assert!(
            reboot
                .effects
                .flags_written
                .contains(ExecFlag::RebootCalled)
        );
        let corrupt = layout_of(EW_REBOOT, [1, 0, 0, 0, 0, 0]);
        assert_eq!(corrupt.effects.terminates, Termination::Always);
    }

    /// **Every slot a table marks as a variable is one the runtime reaches
    /// through `var0` or `var1`.** `exec.c` derives variables from the first
    /// two operands only (lines 266-272), so a variable anywhere else is a
    /// table error - the kind that put `EW_EXECUTE`'s exit code in slot 2.
    #[test]
    fn variables_live_in_the_first_two_slots() {
        for info in &OPCODES {
            for (index, kind) in info.param_types.iter().enumerate() {
                if kind.is_variable() {
                    assert!(index < 2, "{} slot {index}", info.mnemonic);
                }
            }
        }
    }

    #[test]
    fn opcodes_without_forms_keep_their_fixed_layout() {
        for (i, info) in OPCODES.iter().enumerate() {
            let layout = param_layout(i as u32, NsisVersion::V3, info, &[0; 6]);
            if layout != ParamLayout::fixed(info) {
                continue;
            }
            assert_eq!(layout.names, info.param_names, "{}", info.mnemonic);
            assert_eq!(layout.types, info.param_types, "{}", info.mnemonic);
            assert_eq!(layout.count, info.param_count, "{}", info.mnemonic);
            assert_eq!(layout.effects, info.effects, "{}", info.mnemonic);
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
        // The sixth slot is the error text, a language string by default.
        assert_eq!(op.param_types[5], String);
        assert_eq!(op.effects.outdir, Some(Access::Read));
    }
}
