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
//! # Supported versions
//!
//! NSIS changed its container format, its instruction numbering and its string
//! encoding several times. This crate reads all of them through one interface
//! and works out which applies from the file itself, since nothing in a file
//! records what built it.
//!
//! - **NSIS 1.x** — a different container rather than a variant. It predates
//!   the block table, so its tables are found by adding up sizes rather than
//!   looked up ([`header::V1Header`]); its sections are 20 bytes and its
//!   instructions 24; it numbers its instructions differently and moves the
//!   parameters of several ([`opcode::OPCODES_V1`]); a variable reference is a
//!   single byte ([`strings::v1`]); its bzip2 keeps a per-block flag NSIS 2.0
//!   dropped; its first-header flags mean different things; and an uninstaller
//!   uses a shorter header again, with no sections
//!   ([`header::v1header::V1HeaderKind`]).
//! - **NSIS 2.x** — including the variable-layout changes in 2.04 and 2.26,
//!   which move the built-in variables and so change what a reference decodes
//!   to ([`Nsis2SubVersion`]).
//! - **NSIS 3.x** — ANSI and Unicode targets. NSIS 3 moved the ANSI special
//!   codes from the top of the byte range to the bottom
//!   ([`strings::ansi::AnsiCodeRange`]).
//! - **The Jim Park Unicode fork** — 2.46.1, 2.46.2 and 2.46.3, each inserting
//!   instructions at a different point ([`ParkSubVersion`]).
//! - **Logging builds** — a makensis compiled with logging carries an extra
//!   instruction, shifting everything above `WriteUninstaller`
//!   ([`NsisInstaller::is_log_build`]).
//!
//! Compression is deflate, bzip2 (both NSIS block layouts) or LZMA, solid or
//! non-solid, in any combination the compiler could produce.
//!
//! [`NsisInstaller::version`] reports what was detected. Where a property
//! cannot be read from the file, it is inferred by decoding the entry stream
//! under each candidate layout and keeping the one it is consistent with — an
//! instruction whose parameters do not fit the entry cannot be the right
//! reading. A file that is consistent with none of them is rejected rather
//! than decoded into plausible nonsense.
//!
//! Builds of makensis with a non-default compile-time configuration are a
//! known limit: nearly every header field and instruction sits behind an
//! `#ifdef`, so a custom build numbers them differently.
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
