//! PE overlay detection for NSIS installers.
//!
//! NSIS installation data is appended as a PE overlay after the last PE section.
//! This module locates the overlay start and provides access to the overlay bytes.

use goblin::pe::{
    PE,
    optional_header::MAGIC_32,
    options::{ParseMode, ParseOptions},
};

use crate::error::Error;

/// Provides access to the PE overlay region of an NSIS installer.
///
/// The overlay is the region of the file after the last PE section's raw data.
/// NSIS appends all installation data (FirstHeader, compressed headers, data block)
/// in this region.
///
/// # Example
///
/// ```no_run
/// use nsis::addressmap::PeOverlay;
///
/// let file = std::fs::read("installer.exe").unwrap();
/// let overlay = PeOverlay::from_bytes(&file).unwrap();
/// println!("Overlay starts at offset 0x{:X}", overlay.overlay_offset());
/// println!("Overlay size: {} bytes", overlay.overlay().len());
/// ```
#[derive(Debug)]
pub struct PeOverlay<'a> {
    file: &'a [u8],
    overlay_offset: usize,
}

impl<'a> PeOverlay<'a> {
    /// Parses the PE headers and locates the overlay region.
    ///
    /// Returns an error if the file is not a valid PE32 executable or
    /// if no overlay data exists after the PE sections.
    pub fn from_bytes(file: &'a [u8]) -> Result<Self, Error> {
        let pe = PE::parse_with_opts(file, &Self::parse_options()).map_err(Error::from)?;
        Self::from_goblin(file, &pe)
    }

    /// Returns the PE parse options this crate uses to locate an overlay.
    ///
    /// Overlay detection needs only the optional header magic and the section
    /// table, so every optional structure is switched off and parsing runs in
    /// permissive mode. Installer stubs routinely carry auxiliary structures a
    /// strict parser rejects — NSIS 2.03 stubs point their resource directory
    /// past the appended data, and Park 2.46.2+ stubs declare base relocations
    /// at an RVA that cannot be mapped — and failing on those would reject an
    /// installer whose NSIS data is perfectly intact.
    ///
    /// Pass these to [`PE::parse_with_opts`] when pre-parsing a PE
    /// for [`from_goblin`](Self::from_goblin).
    pub fn parse_options() -> ParseOptions {
        let mut opts = ParseOptions::default();
        opts.resolve_rva = false;
        opts.parse_attribute_certificates = false;
        opts.parse_tls_data = false;
        opts.parse_resources = false;
        opts.parse_imports = false;
        opts.parse_mode = ParseMode::Permissive;
        opts
    }

    /// Locates the overlay region using a pre-parsed goblin PE.
    ///
    /// This is useful when the caller already has a parsed PE and wants to
    /// avoid re-parsing.
    pub fn from_goblin(file: &'a [u8], pe: &PE<'_>) -> Result<Self, Error> {
        // Validate PE32 (not PE32+).
        if let Some(oh) = pe.header.optional_header {
            let magic = oh.standard_fields.magic;
            if magic != MAGIC_32 {
                return Err(Error::Not32Bit { magic });
            }
        }

        // Find the end of the last PE section's raw data.
        //
        // Sections whose raw range runs past the end of the file are skipped:
        // they cannot hold data the file does not contain, and taking their
        // claimed end would put the overlay beyond EOF and lose an installer
        // that is otherwise intact. Park 2.46.2+ stubs do exactly this — their
        // `.reloc` header claims 4096 bytes at an offset that leaves only ~1.5 KB
        // before EOF, with the NSIS FirstHeader sitting inside that claimed
        // range. 7-Zip mis-detects those same stubs as plain PE files.
        let overlay_offset = pe
            .sections
            .iter()
            .map(|s| (s.pointer_to_raw_data as usize).saturating_add(s.size_of_raw_data as usize))
            .filter(|end| *end <= file.len())
            .max()
            .unwrap_or(0);

        if overlay_offset == 0 || overlay_offset >= file.len() {
            return Err(Error::OverlayNotFound);
        }

        Ok(Self {
            file,
            overlay_offset,
        })
    }

    /// Returns the overlay bytes (everything after the last PE section).
    pub fn overlay(&self) -> &'a [u8] {
        // overlay_offset is validated < file.len() in `parse`.
        self.file.get(self.overlay_offset..).unwrap_or(&[])
    }

    /// Returns the byte offset where the overlay begins in the file.
    pub fn overlay_offset(&self) -> usize {
        self.overlay_offset
    }

    /// Returns `true` if the PE contains a `.ndata` section.
    ///
    /// Per SANS ISC: "NSIS-created executables contain a distinctive section
    /// named '.ndata'." This is a quick heuristic to check if a PE is likely
    /// an NSIS installer before attempting full parsing.
    pub fn has_ndata_section(pe: &PE<'_>) -> bool {
        pe.sections.iter().any(|s| {
            let name = s.name().unwrap_or("");
            name == ".ndata"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_not_found_on_empty() {
        // A buffer too small to be a valid PE.
        let data = [0u8; 64];
        let result = PeOverlay::from_bytes(&data);
        assert!(result.is_err());
    }
}
