//! NSIS callback identifiers.
//!
//! The NSIS common header reserves ten callback slots whose entry indices
//! are loaded by the installer at well-defined lifecycle points (UI init,
//! install success/failure, user abort, etc.). [`Callback`] enumerates
//! these slots in their fixed common-header order so consumers can iterate
//! them without hardcoding name literals or array offsets.
//!
//! The indices match the order of fields in the common header (see
//! `src/header/commonheader.rs`) and are stable across NSIS versions.

use core::fmt;

/// Identifies a script-level callback exposed by the NSIS common header.
///
/// Every NSIS installer reserves these ten callback slots. The variant
/// order matches the on-disk common header layout - see
/// [`Callback::index`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Callback {
    /// `.onInit` - runs before the installer UI is shown.
    OnInit,
    /// `.onInstSuccess` - runs after all sections complete successfully.
    OnInstSuccess,
    /// `.onInstFailed` - runs when installation fails or aborts.
    OnInstFailed,
    /// `.onUserAbort` - runs when the user clicks Cancel.
    OnUserAbort,
    /// `.onGUIInit` - runs after the installer dialog is created.
    OnGuiInit,
    /// `.onGUIEnd` - runs after the installer dialog is destroyed.
    OnGuiEnd,
    /// `.onMouseOverSection` - runs on section mouse-over in the components page.
    OnMouseOverSection,
    /// `.onVerifyInstDir` - runs whenever the install directory changes.
    OnVerifyInstDir,
    /// `.onSelChange` - runs when section selection changes.
    OnSelChange,
    /// `.onRebootFailed` - runs if a reboot triggered by the installer fails.
    OnRebootFailed,
}

impl Callback {
    /// All ten callbacks in common-header order.
    pub const ALL: [Callback; 10] = [
        Callback::OnInit,
        Callback::OnInstSuccess,
        Callback::OnInstFailed,
        Callback::OnUserAbort,
        Callback::OnGuiInit,
        Callback::OnGuiEnd,
        Callback::OnMouseOverSection,
        Callback::OnVerifyInstDir,
        Callback::OnSelChange,
        Callback::OnRebootFailed,
    ];

    /// Returns the canonical NSIS script name (e.g., `".onInit"`).
    pub fn name(self) -> &'static str {
        match self {
            Callback::OnInit => ".onInit",
            Callback::OnInstSuccess => ".onInstSuccess",
            Callback::OnInstFailed => ".onInstFailed",
            Callback::OnUserAbort => ".onUserAbort",
            Callback::OnGuiInit => ".onGUIInit",
            Callback::OnGuiEnd => ".onGUIEnd",
            Callback::OnMouseOverSection => ".onMouseOverSection",
            Callback::OnVerifyInstDir => ".onVerifyInstDir",
            Callback::OnSelChange => ".onSelChange",
            Callback::OnRebootFailed => ".onRebootFailed",
        }
    }

    /// Returns this callback's slot index in the common-header callback array (0..10).
    pub fn index(self) -> usize {
        match self {
            Callback::OnInit => 0,
            Callback::OnInstSuccess => 1,
            Callback::OnInstFailed => 2,
            Callback::OnUserAbort => 3,
            Callback::OnGuiInit => 4,
            Callback::OnGuiEnd => 5,
            Callback::OnMouseOverSection => 6,
            Callback::OnVerifyInstDir => 7,
            Callback::OnSelChange => 8,
            Callback::OnRebootFailed => 9,
        }
    }
}

impl fmt::Display for Callback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_in_index_order() {
        for (i, cb) in Callback::ALL.iter().enumerate() {
            assert_eq!(cb.index(), i);
        }
    }

    #[test]
    fn names_match_nsis_script() {
        assert_eq!(Callback::OnInit.name(), ".onInit");
        assert_eq!(Callback::OnGuiInit.name(), ".onGUIInit");
        assert_eq!(Callback::OnRebootFailed.name(), ".onRebootFailed");
    }

    #[test]
    fn display_uses_name() {
        assert_eq!(Callback::OnInit.to_string(), ".onInit");
    }
}
