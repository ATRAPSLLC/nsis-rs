//! NSIS data structure view types.
//!
//! This module contains zero-copy view types for the core NSIS data structures
//! found in the decompressed header block:
//!
//! - [`Section`]: Install section descriptors.
//! - [`Entry`]: Bytecode instructions (opcode + 6 parameters).
//! - [`Page`]: Installer page definitions.
//! - [`CtlColors`]: Control color structures.
//! - [`LangTable`]: Language table entries.

pub mod ctlcolors;
pub mod entry;
pub mod langtable;
pub mod page;
pub mod section;

pub use ctlcolors::CtlColors;
pub use entry::{Entry, EntryIter, MAX_ENTRY_OFFSETS};
pub use langtable::LangTable;
pub use page::{Page, PageIter, PageType};
pub use section::{Section, SectionIter};
