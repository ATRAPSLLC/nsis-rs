//! What an instruction reads and writes: the state the runtime keeps between
//! instructions, and the effects no operand slot names.
//!
//! An NSIS script works on three kinds of state besides its variables. The
//! **execution flags** (`exec_flags_t` in `api.h`) are fourteen integers the
//! script reads and sets with `IfErrors`, `SetDetailsPrint`, `IfRebootFlag`
//! and the like, and which many instructions change or consult without naming
//! them: nearly every command that can fail sets the error flag. The **string
//! stack** is what `Push` and `Pop` work on, and what a plugin takes its
//! arguments from. And a few built-in variables - `$OUTDIR` above all - are
//! read or written by commands that do not name them in a slot.
//!
//! A slot names the state it touches through [`ParamType`](super::ParamType);
//! [`Effects`] states the rest. Together they are everything an instruction
//! does to the script's state, taken from the runtime (`exec.c`), so that a
//! consumer can follow values through a script without re-deriving it.

use std::fmt;

/// How an instruction uses a variable or flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Access {
    /// Reads the current value and leaves it.
    Read,
    /// Replaces the value without reading it, every time the instruction runs.
    Write,
    /// Replaces the value on some runs and leaves it on others.
    ///
    /// Which one happens is decided while the instruction runs - a `Pop` from
    /// an empty stack writes nothing, a `FileRead` asked for no characters
    /// writes nothing - so the value from before may survive it.
    MayWrite,
    /// Reads the value, then replaces it.
    Update,
}

impl Access {
    /// Reports whether the instruction reads the value before any write.
    #[must_use]
    pub fn reads(self) -> bool {
        matches!(self, Self::Read | Self::Update)
    }

    /// Reports whether the instruction can replace the value.
    #[must_use]
    pub fn writes(self) -> bool {
        !matches!(self, Self::Read)
    }

    /// Reports whether the value from before can still be the value after.
    #[must_use]
    pub fn may_keep(self) -> bool {
        matches!(self, Self::Read | Self::MayWrite)
    }
}

/// One of the runtime's execution flags.
///
/// The discriminants are the flags' indices in `exec_flags_t` (`api.h`), which
/// is how a flag instruction names one: `SetErrors` is `EW_SETFLAG` with index
/// 2. NSIS appended fields to the structure over its releases and never moved
/// one. NSIS 1.x keeps the same flags as separate globals and names none of
/// them by index; its instructions state which they use through [`Effects`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ExecFlag {
    /// `SetAutoClose`: close the install page when it finishes.
    AutoClose = 0,
    /// `SetShellVarContext`: shell folders and `SHCTX` resolve for all users.
    AllUserVar = 1,
    /// `IfErrors`, `ClearErrors`, `SetErrors`: set by a command that fails.
    ExecError = 2,
    /// `IfAbort`: the user cancelled.
    ///
    /// The interface raises it while the script runs, between any two of its
    /// instructions ([`is_external`](Self::is_external)).
    Abort = 3,
    /// `IfRebootFlag`, `SetRebootFlag`: something waits for a reboot.
    ExecReboot = 4,
    /// `Reboot` was reached.
    RebootCalled = 5,
    /// The install type, before it had its own instruction. Unused since.
    CurInstType = 6,
    /// The plugin interface version the runtime offers.
    PluginApiVersion = 7,
    /// `IfSilent`, `SetSilent`.
    Silent = 8,
    /// `GetInstDirError`: why the directory page refused the directory.
    InstdirError = 9,
    /// `IfRtlLanguage`: the language reads right to left.
    Rtl = 10,
    /// `SetErrorLevel`: the process exit code.
    ErrorLevel = 11,
    /// `SetRegView`: which registry view `HKLM` and `HKCU` open.
    AlterRegView = 12,
    /// `SetDetailsPrint`: where status text goes.
    StatusUpdate = 13,
}

impl ExecFlag {
    /// Every flag, in index order.
    pub const ALL: [Self; 14] = [
        Self::AutoClose,
        Self::AllUserVar,
        Self::ExecError,
        Self::Abort,
        Self::ExecReboot,
        Self::RebootCalled,
        Self::CurInstType,
        Self::PluginApiVersion,
        Self::Silent,
        Self::InstdirError,
        Self::Rtl,
        Self::ErrorLevel,
        Self::AlterRegView,
        Self::StatusUpdate,
    ];

    /// Returns the flag at `index` in `exec_flags_t`, or `None` past the end.
    #[must_use]
    pub fn from_index(index: i32) -> Option<Self> {
        usize::try_from(index)
            .ok()
            .and_then(|index| Self::ALL.get(index))
            .copied()
    }

    /// Returns the flag's index in `exec_flags_t`.
    #[must_use]
    pub fn index(self) -> u8 {
        self as u8
    }

    /// Returns the flag's field name in `exec_flags_t`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::AutoClose => "autoclose",
            Self::AllUserVar => "all_user_var",
            Self::ExecError => "exec_error",
            Self::Abort => "abort",
            Self::ExecReboot => "exec_reboot",
            Self::RebootCalled => "reboot_called",
            Self::CurInstType => "cur_insttype",
            Self::PluginApiVersion => "plugin_api_version",
            Self::Silent => "silent",
            Self::InstdirError => "instdir_error",
            Self::Rtl => "rtl",
            Self::ErrorLevel => "errlvl",
            Self::AlterRegView => "alter_reg_view",
            Self::StatusUpdate => "status_update",
        }
    }

    /// Reports whether something outside the script changes the flag while
    /// the script runs.
    ///
    /// The interface raises [`Abort`](Self::Abort) when the user cancels,
    /// between any two instructions, so a value read from it once says
    /// nothing about the next read. Every other flag changes only through an
    /// instruction or before the script starts.
    #[must_use]
    pub fn is_external(self) -> bool {
        matches!(self, Self::Abort)
    }
}

impl fmt::Display for ExecFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of [`ExecFlag`]s.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FlagSet(u16);

impl FlagSet {
    /// No flag.
    pub const EMPTY: Self = Self(0);

    /// Every flag.
    pub const ALL: Self = Self(0x3FFF);

    /// Returns the set of `flags`.
    #[must_use]
    pub const fn of(flags: &[ExecFlag]) -> Self {
        let mut bits = 0u16;
        let mut rest = flags;
        while let [flag, tail @ ..] = rest {
            bits |= bit(*flag);
            rest = tail;
        }
        Self(bits)
    }

    /// Returns this set with `flag` added.
    #[must_use]
    pub const fn with(self, flag: ExecFlag) -> Self {
        Self(self.0 | bit(flag))
    }

    /// Returns the flags in either set.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Reports whether `flag` is in the set.
    #[must_use]
    pub const fn contains(self, flag: ExecFlag) -> bool {
        self.0 & bit(flag) != 0
    }

    /// Reports whether the set holds no flag.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the flags in the set, in index order.
    pub fn iter(self) -> impl Iterator<Item = ExecFlag> {
        ExecFlag::ALL
            .into_iter()
            .filter(move |flag| self.contains(*flag))
    }
}

/// The bit a flag occupies in a [`FlagSet`]; every index is below 16.
const fn bit(flag: ExecFlag) -> u16 {
    1u16.wrapping_shl(flag as u32)
}

impl fmt::Debug for FlagSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// What an instruction does to the string stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StackEffect {
    /// Leaves it alone.
    #[default]
    None,
    /// Pushes one string.
    Push,
    /// Pops one string, when there is one: an empty stack sets the error flag
    /// and writes nothing.
    Pop,
    /// Swaps the top string with the one at the depth its slot gives.
    Exch,
}

/// Whether an instruction can end the code it runs in.
///
/// The runtime stops a code segment - a section, a function and whatever
/// called it - when an instruction returns `EXEC_ERROR`; the installer then
/// fails or quits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Termination {
    /// Always continues.
    #[default]
    Never,
    /// Continues on some runs and ends the code on others: a `File` the user
    /// aborts, a function that aborts.
    May,
    /// Always ends the code: `Abort`, `Quit`.
    Always,
}

/// Which variables an instruction touches without naming them in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HiddenVariables {
    /// None beyond the built-ins [`Effects`] names.
    #[default]
    None,
    /// It expands a string chosen while it runs - the text a script stored
    /// for an install type, say - so it may read any variable.
    ReadsAny,
    /// It hands every variable to a plugin, which may read or write any.
    Any,
}

/// What an instruction does beyond its slots.
///
/// Everything here is a *may*: the runtime sets the error flag only when a
/// command fails, for instance, and reads `$OUTDIR` only for a relative path.
/// A consumer following values keeps the value from before alive across a
/// write stated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effects {
    /// How it uses `$OUTDIR`, which relative paths and new processes resolve
    /// against, and which `SetOutPath` sets.
    pub outdir: Option<Access>,
    /// How it uses `$INSTDIR`, where NSIS 1.x puts a relative uninstaller
    /// path.
    pub instdir: Option<Access>,
    /// Other variables it touches.
    pub hidden: HiddenVariables,
    /// Flags it reads.
    pub flags_read: FlagSet,
    /// Flags it may change.
    pub flags_written: FlagSet,
    /// What it does to the string stack.
    pub stack: StackEffect,
    /// Whether it runs a plugin, which receives the variables, the stack and
    /// the flags and may change any of them, and may run script functions.
    pub plugin: bool,
    /// Whether it can end the code it runs in.
    pub terminates: Termination,
    /// Whether it acts on anything outside the script's own state - the file
    /// system, the registry, windows, processes, the environment, the
    /// installer's own interface - by reading it or by changing it.
    ///
    /// An instruction that does not computes only on the variables, flags and
    /// stack it names, so the same inputs give the same result and nothing
    /// else observes it. One that does may not be repeated, dropped or merged
    /// with another however little of its result the script reads: a second
    /// `IfFileExists` can see a file a `File` between them wrote.
    pub outside: bool,
}

impl Effects {
    /// Nothing beyond the slots.
    pub const NONE: Self = Self {
        outdir: None,
        instdir: None,
        hidden: HiddenVariables::None,
        flags_read: FlagSet::EMPTY,
        flags_written: FlagSet::EMPTY,
        stack: StackEffect::None,
        plugin: false,
        terminates: Termination::Never,
        outside: false,
    };

    /// Returns these effects acting outside the script ([`Self::outside`]).
    #[must_use]
    pub const fn acting_outside(mut self) -> Self {
        self.outside = true;
        self
    }

    /// Returns these effects with `flags` added to the flags read.
    #[must_use]
    pub const fn reading(mut self, flags: FlagSet) -> Self {
        self.flags_read = self.flags_read.union(flags);
        self
    }

    /// Returns these effects with `flags` added to the flags changed.
    #[must_use]
    pub const fn writing(mut self, flags: FlagSet) -> Self {
        self.flags_written = self.flags_written.union(flags);
        self
    }

    /// Returns these effects with the error flag among the flags changed: the
    /// instruction sets it when it fails.
    #[must_use]
    pub const fn failing(self) -> Self {
        self.writing(FlagSet::of(&[ExecFlag::ExecError]))
    }

    /// Returns these effects with the reads of a status line added: the
    /// runtime's `update_status_text` consults `SetDetailsPrint`.
    #[must_use]
    pub const fn reporting(self) -> Self {
        self.reading(FlagSet::of(&[ExecFlag::StatusUpdate]))
    }

    /// Returns these effects with the reads of a message box added:
    /// `my_MessageBox` answers with the default button when silent, and lays
    /// the box out right to left for such a language.
    #[must_use]
    pub const fn asking(self) -> Self {
        self.reading(FlagSet::of(&[ExecFlag::Silent, ExecFlag::Rtl]))
    }

    /// Returns these effects with a use of `$OUTDIR` added.
    #[must_use]
    pub const fn with_outdir(mut self, access: Access) -> Self {
        self.outdir = Some(access);
        self
    }

    /// Returns these effects with a string-stack effect.
    #[must_use]
    pub const fn with_stack(mut self, stack: StackEffect) -> Self {
        self.stack = stack;
        self
    }

    /// Returns these effects with a termination.
    #[must_use]
    pub const fn with_termination(mut self, terminates: Termination) -> Self {
        self.terminates = terminates;
        self
    }

    /// Returns the effects of a plugin call: every variable, the whole
    /// stack, and every flag the plugin interface exposes.
    #[must_use]
    pub const fn of_plugin(self, flags: FlagSet) -> Self {
        let mut effects = self.reading(flags).writing(flags);
        effects.hidden = HiddenVariables::Any;
        effects.plugin = true;
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_is_named_by_its_index_in_exec_flags() {
        for (index, flag) in ExecFlag::ALL.iter().enumerate() {
            assert_eq!(usize::from(flag.index()), index);
            assert_eq!(ExecFlag::from_index(index as i32), Some(*flag));
        }
        assert_eq!(ExecFlag::from_index(2), Some(ExecFlag::ExecError));
        assert_eq!(ExecFlag::from_index(14), None);
        assert_eq!(ExecFlag::from_index(-1), None);
    }

    #[test]
    fn a_flag_set_holds_exactly_its_flags() {
        let set = FlagSet::of(&[ExecFlag::ExecError, ExecFlag::StatusUpdate]);
        assert!(set.contains(ExecFlag::ExecError));
        assert!(!set.contains(ExecFlag::Abort));
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [ExecFlag::ExecError, ExecFlag::StatusUpdate]
        );
        assert_eq!(FlagSet::ALL.iter().count(), ExecFlag::ALL.len());
        assert!(FlagSet::EMPTY.is_empty());
    }

    #[test]
    fn access_says_what_survives() {
        assert!(Access::Read.reads() && !Access::Read.writes());
        assert!(!Access::Write.reads() && !Access::Write.may_keep());
        assert!(Access::MayWrite.writes() && Access::MayWrite.may_keep());
        assert!(Access::Update.reads() && Access::Update.writes());
    }
}
