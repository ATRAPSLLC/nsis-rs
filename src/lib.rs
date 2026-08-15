//! Parse and inspect NSIS installer binaries.
//!
//! This crate provides typed access to all internal structures within an
//! NSIS (NullSoft Scriptable Install System) installer executable, from
//! the PE overlay through decompressed headers down to individual script
//! instructions and embedded files.
//!
//! # Quick Start
//!
//! ```no_run
//! use nsis::NsisInstaller;
//!
//! let file_bytes = std::fs::read("installer.exe").unwrap();
//! let installer = NsisInstaller::from_bytes(&file_bytes).unwrap();
//!
//! println!("Version: {:?}", installer.version());
//! for section in installer.sections() {
//!     let section = section.unwrap();
//!     println!("  Section: code_size={}", section.code_size());
//! }
//! ```
//!
//! # Architecture
//!
//! The crate is organized in layers:
//!
//! - **PE overlay detection** ([`addressmap::PeOverlay`]): Locates the NSIS data
//!   appended after the PE sections.
//! - **Decompression** ([`decompress`]): Handles zlib, bzip2, and LZMA
//!   decompression of the header block.
//! - **Low-level structures** ([`header`], [`nsis`], [`strings`], [`opcode`]):
//!   View types for each structure in the NSIS format.
//! - **High-level API** ([`NsisInstaller`]): Ties everything together into
//!   a convenient exploration interface.
//!
//! The `EW_*` opcode constants live in [`opcode`], alongside the tables that
//! give them meaning:
//!
//! ```
//! use nsis::opcode::{EW_EXTRACTFILE, EW_RET};
//! ```
//!
//! # Design
//!
//! Low-level structure types borrow from either the original file byte slice
//! or the decompressed header buffer. Accessor methods read directly from the
//! underlying buffer using little-endian byte decoding. The only heap
//! allocations are for decompressed data and decoded strings.

// `missing_docs`, `unsafe_code`, plus the clippy panic-prevention set
// (`unwrap_used`, `expect_used`, `panic`, `arithmetic_side_effects`,
// `indexing_slicing`) are declared in `Cargo.toml` under `[lints]` so
// they enforce on every build regardless of the consuming workspace.
// nsis is used in malware-analysis pipelines where every input byte is
// adversarial and the parser must not panic.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing
    )
)]

pub mod addressmap;
pub mod decompress;
pub mod error;
pub mod header;
pub mod installer;
pub mod nsis;
pub mod opcode;
pub mod strings;

mod util;

pub use error::Error;
pub use installer::{
    BasicBlock, BranchCondition, Callback, ControlFlowEdge, ControlFlowTarget, EdgeKind,
    ExecCommand, ExecIter, ExecOp, ExtractedFile, FileIter, Instruction, InstructionIter,
    NsisInstaller, NsisInstallerBuilder, PageHandler, PluginCall, PluginCallIter, RegDelete,
    RegRead, RegValueType, RegWrite, RegistryIter, RegistryOp, ScriptAnalysis,
    ScriptAnalysisDiagnostic, ScriptFunction, ScriptRoot, ScriptRootKind, ShellExecOp, Shortcut,
    ShortcutIter, SolidStatus, Uninstaller, UninstallerIter,
};
pub use opcode::{
    Nsis2SubVersion, NsisVersion, OpcodeInfo, ParamLayout, ParamType, ParkSubVersion,
};
