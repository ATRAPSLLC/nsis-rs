//! High-level NSIS installer API.
//!
//! This module ties together all lower-level parsers into a convenient
//! [`NsisInstaller`] entry point. It handles PE overlay detection,
//! decompression, header parsing, version detection, and provides
//! iterators over all installer structures.
//!
//! The [`analysis`] submodule provides security-focused iterators for
//! malware analysis: plugin calls, exec commands, registry operations,
//! shortcuts, and embedded uninstaller stubs.

pub mod analysis;
pub mod callback;
mod files;
mod nsisinstaller;
pub mod script;

pub use analysis::{
    ExecCommand, ExecIter, ExecOp, PluginCall, PluginCallIter, RegDelete, RegRead, RegValueType,
    RegWrite, RegistryIter, RegistryOp, ShellExecOp, Shortcut, ShortcutIter, Uninstaller,
    UninstallerIter,
};
pub use callback::Callback;
pub use files::{ExtractedFile, FileIter};
pub use nsisinstaller::{NsisInstaller, NsisInstallerBuilder, SolidStatus};
pub use script::{
    BasicBlock, BranchCondition, ControlFlowEdge, ControlFlowTarget, EdgeKind, PageHandler,
    ScriptAnalysis, ScriptAnalysisDiagnostic, ScriptFunction, ScriptRoot, ScriptRootKind,
};
