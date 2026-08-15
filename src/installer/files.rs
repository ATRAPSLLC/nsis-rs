//! File extraction API for NSIS installers.
//!
//! Provides zero-copy iteration over embedded files found via `EW_EXTRACTFILE`
//! instructions in the NSIS script.
//!
//! # Example
//!
//! ```no_run
//! use nsis::NsisInstaller;
//!
//! let data = std::fs::read("installer.exe").unwrap();
//! let inst = NsisInstaller::from_bytes(&data).unwrap();
//!
//! for file in inst.files() {
//!     let file = file.unwrap();
//!     println!("{}: {} bytes (compressed: {})",
//!         file.name().unwrap(),
//!         file.data().len(),
//!         file.is_compressed());
//! }
//! ```

use crate::{
    decompress::{self, CompressionMode},
    error::Error,
    installer::{
        NsisInstaller,
        analysis::{Instruction, InstructionIter},
        nsisinstaller::SolidStatus,
    },
    nsis::entry::{Entry, EntryIter},
    strings::{NsisString, StringSegment},
};

/// A single embedded file found in an NSIS installer.
///
/// Provides zero-copy access to the file's metadata and raw data. The raw
/// data slice borrows directly from the original file buffer — no copies
/// are made until you call [`decompress`](Self::decompress).
///
/// # Data layout (non-solid mode)
///
/// In non-solid mode, each file in the data block is prefixed with a 4-byte
/// length header: bit 31 indicates whether the payload is compressed, and
/// the lower 31 bits give the byte count. The [`data`](Self::data) method
/// returns the payload bytes after this prefix.
///
/// # Solid mode
///
/// In solid mode, all files are concatenated in a single compressed stream,
/// which the parser decompresses once and caches. [`data`](Self::data) and
/// [`decompress`](Self::decompress) then slice that cache, so both work the
/// same as in non-solid mode.
///
/// If the solid stream hit the decompression budget or failed to decode, the
/// cache is short or empty. Files that fall past that point report the reason —
/// [`Error::OutputTooLarge`] or the underlying decode error — rather than a
/// bounds error. See
/// [`NsisInstaller::solid_status`](crate::installer::NsisInstaller::solid_status).
#[derive(Debug)]
pub struct ExtractedFile<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
    /// Output directory in effect when this file was extracted, or `None` for
    /// a name that already carries its own root.
    out_dir: Option<NsisString>,
}

impl<'a> ExtractedFile<'a> {
    /// Builds a view over an `EW_EXTRACTFILE` entry.
    pub(crate) fn new(
        installer: &'a NsisInstaller<'a>,
        entry: Entry<'a>,
        out_dir: Option<NsisString>,
    ) -> Self {
        Self {
            installer,
            entry,
            out_dir,
        }
    }

    /// Returns the file's full destination path.
    ///
    /// [`name`](Self::name) gives only what the `EW_EXTRACTFILE` instruction
    /// stores, which is usually a bare file name. The directory comes from the
    /// `SetOutPath` in effect at that point in the script, so this joins the
    /// two and yields the path the installer would actually write:
    ///
    /// ```no_run
    /// # let data = std::fs::read("installer.exe").unwrap();
    /// # let inst = nsis::NsisInstaller::from_bytes(&data).unwrap();
    /// for file in inst.files() {
    ///     let path = file.unwrap().dest_path().unwrap();
    ///
    ///     // "Lang\de_DE.ini", relative to the install directory, as 7-Zip lists it.
    ///     println!("{}", path.to_install_path());
    ///
    ///     // "Lang/de_DE.ini", safe to join onto an output directory.
    ///     println!("{}", path.to_path());
    /// }
    /// ```
    ///
    /// A name that already carries its own root is returned unchanged, so an
    /// electron-builder payload stays `$PLUGINSDIR\app-64.7z` rather than being
    /// moved under the install directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the name cannot be read from the string table.
    pub fn dest_path(&self) -> Result<NsisString, Error> {
        let name = self.name()?;
        let Some(out_dir) = &self.out_dir else {
            return Ok(name);
        };
        if out_dir.is_empty() {
            return Ok(name);
        }

        let mut segments = out_dir.segments.clone();
        // Only add a separator if the directory does not already end with one.
        let ends_with_separator = matches!(
            segments.last(),
            Some(StringSegment::Literal(text)) if text.ends_with('\\') || text.ends_with('/')
        );
        if !ends_with_separator {
            segments.push(StringSegment::Literal("\\".into()));
        }
        segments.extend(name.segments);
        Ok(NsisString { segments })
    }

    /// Returns the output directory this file is extracted into.
    ///
    /// `None` when the name carries its own root and no directory applies. The
    /// directory is reconstructed by replaying `SetOutPath` instructions, so it
    /// still contains unresolved references such as `$INSTDIR`.
    #[inline]
    pub fn out_dir(&self) -> Option<&NsisString> {
        self.out_dir.as_ref()
    }

    /// Returns the file name as a decoded NSIS string.
    ///
    /// The name may contain variable references (e.g., `$INSTDIR\app.exe`).
    /// Use the [`Display`](std::fmt::Display) impl on [`NsisString`] to
    /// render it with resolved variable names.
    pub fn name(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(1))
    }

    /// Returns the overwrite mode flags from the `EW_EXTRACTFILE` instruction.
    #[inline]
    pub fn overwrite_flags(&self) -> i32 {
        self.entry.offset(0)
    }

    /// Returns the byte offset of this file within the data block.
    #[inline]
    pub fn data_block_offset(&self) -> u32 {
        self.entry.offset(2) as u32
    }

    /// Returns the FILETIME timestamp as `(low, high)`, or `None` if unset.
    pub fn datetime(&self) -> Option<(u32, u32)> {
        let lo = self.entry.offset(3);
        let hi = self.entry.offset(4);
        if lo == 0 && hi == 0 {
            None
        } else {
            Some((lo as u32, hi as u32))
        }
    }

    /// Returns `true` if the file payload is compressed.
    ///
    /// In non-solid mode this is determined by bit 31 of the length prefix.
    /// In solid mode, individual file entries within the decompressed stream
    /// may still have their own compression (bit 31 of their length prefix).
    pub fn is_compressed(&self) -> bool {
        let Some((is_compressed, _)) = self.length_prefix() else {
            return false;
        };
        is_compressed
    }

    /// Returns the raw payload bytes for this file (after the length prefix).
    ///
    /// For non-solid mode, this is a zero-copy slice into the original file
    /// buffer. For solid mode, this is a slice into the decompressed solid
    /// data cache. In both cases, no copies are made.
    ///
    /// For compressed entries (bit 31 set in length prefix), this returns the
    /// compressed bytes. For uncompressed entries, this is the raw file content.
    pub fn data(&self) -> &[u8] {
        let Some((_, size)) = self.length_prefix() else {
            return &[];
        };
        let source = self.data_source();
        let Some(offset) = self.source_offset().checked_add(4) else {
            return &[];
        };
        let Some(end) = offset.checked_add(size as usize) else {
            return &[];
        };
        source.get(offset..end).unwrap_or(&[])
    }

    /// Decompresses the file and returns its content.
    ///
    /// For uncompressed entries, this simply copies the raw bytes. For
    /// compressed entries within a non-solid archive, this decompresses
    /// using the installer's compression method. For solid archives, the
    /// entries in the decompressed stream are typically uncompressed
    /// (bit 31 clear), so this just copies them.
    ///
    /// # Errors
    ///
    /// Returns an error if the data is out of bounds or decompression fails.
    pub fn decompress(&self) -> Result<Vec<u8>, Error> {
        let Some((is_compressed, size)) = self.length_prefix() else {
            return Err(self.solid_failure().unwrap_or(Error::TooShort {
                expected: 4,
                actual: 0,
                context: "file data length prefix",
            }));
        };

        let source = self.data_source();
        let offset = self.source_offset().checked_add(4).ok_or(Error::TooShort {
            expected: usize::MAX,
            actual: source.len(),
            context: "file data offset overflow",
        })?;
        let end = offset.checked_add(size as usize).ok_or(Error::TooShort {
            expected: usize::MAX,
            actual: source.len(),
            context: "file data end overflow",
        })?;

        let payload = source.get(offset..end).ok_or_else(|| {
            self.solid_failure().unwrap_or(Error::TooShort {
                expected: end,
                actual: source.len(),
                context: "file data payload",
            })
        })?;

        if !is_compressed {
            return Ok(payload.to_vec());
        }

        // The per-file uncompressed size is unknown: the length prefix encodes
        // the *compressed* size, not the decompressed one. `Capped` lets the
        // decoder run to the stream's end-of-stream marker rather than
        // demanding a fixed size (a fixed size made the LZMA decoder reject the
        // EOS marker), and rejects an over-budget stream outright with
        // `Error::OutputTooLarge` instead of truncating it. A successful decode
        // is therefore always complete, so the truncation flag says nothing
        // here.
        decompress::decompress_block(
            payload,
            self.installer.compression(),
            decompress::DecodeLimit::Capped(self.installer.max_decompressed_size()),
        )
        .map(|decoded| decoded.data)
    }

    /// Returns the underlying entry.
    #[inline]
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }

    /// Returns the data source buffer for this file's payload.
    ///
    /// For non-solid: the original file bytes.
    /// For solid: the decompressed solid data cache.
    fn data_source(&self) -> &[u8] {
        if self.installer.compression_mode() == CompressionMode::Solid {
            self.installer.solid_data()
        } else {
            self.installer.file_data()
        }
    }

    /// Returns the byte offset within [`data_source`](Self::data_source) where
    /// this file's length prefix starts.
    fn source_offset(&self) -> usize {
        if self.installer.compression_mode() == CompressionMode::Solid {
            // In solid mode, data_block_offset is a position within the
            // decompressed solid file data stream.
            self.data_block_offset() as usize
        } else {
            // In non-solid mode, data_block_offset is relative to the data
            // block start in the original file.
            self.installer
                .data_block_offset()
                .saturating_add(self.data_block_offset() as usize)
        }
    }

    /// Reads the 4-byte length prefix for this file's data entry.
    fn length_prefix(&self) -> Option<(bool, u32)> {
        let source = self.data_source();
        let offset = self.source_offset();
        let slice = source.get(offset..)?;
        if slice.len() < 4 {
            return None;
        }
        decompress::read_length_prefix(slice).ok()
    }

    /// Returns the error to report when this file's data cannot be reached
    /// because the solid stream is incomplete.
    ///
    /// A bounds failure against a truncated or missing solid buffer says
    /// nothing useful on its own — it reads like corrupt input. Where the
    /// installer recorded why the stream is short, that reason is reported
    /// instead.
    fn solid_failure(&self) -> Option<Error> {
        if self.installer.compression_mode() != CompressionMode::Solid {
            return None;
        }
        match self.installer.solid_status() {
            SolidStatus::Truncated { limit } => Some(Error::OutputTooLarge { limit: *limit }),
            SolidStatus::Failed(e) => Some(e.clone()),
            SolidStatus::NotSolid | SolidStatus::Complete => None,
        }
    }

    /// Validates that the file length prefix and payload are within the source.
    pub(crate) fn validate_data_bounds(&self) -> Result<(), Error> {
        let source = self.data_source();
        let prefix_offset = self.source_offset();
        let prefix_end = prefix_offset.checked_add(4).ok_or(Error::TooShort {
            expected: usize::MAX,
            actual: source.len(),
            context: "file data length prefix",
        })?;
        let prefix = source.get(prefix_offset..prefix_end).ok_or_else(|| {
            self.solid_failure().unwrap_or(Error::TooShort {
                expected: prefix_end,
                actual: source.len(),
                context: "file data length prefix",
            })
        })?;
        let (_, size) = decompress::read_length_prefix(prefix)?;
        let payload_end = prefix_end
            .checked_add(size as usize)
            .ok_or(Error::TooShort {
                expected: usize::MAX,
                actual: source.len(),
                context: "file data payload",
            })?;
        source
            .get(prefix_end..payload_end)
            .map(|_| ())
            .ok_or_else(|| {
                self.solid_failure().unwrap_or(Error::TooShort {
                    expected: payload_end,
                    actual: source.len(),
                    context: "file data payload",
                })
            })
    }
}

/// Iterator over embedded files in an NSIS installer.
///
/// Filters [`InstructionIter`](crate::installer::analysis::InstructionIter)
/// down to `EW_EXTRACTFILE` entries. Created by [`NsisInstaller::files`].
#[derive(Debug)]
pub struct FileIter<'a>(InstructionIter<'a>);

impl<'a> FileIter<'a> {
    pub(crate) fn new(installer: &'a NsisInstaller<'a>, entries: EntryIter<'a>) -> Self {
        Self(InstructionIter::new(installer, entries))
    }
}

impl<'a> Iterator for FileIter<'a> {
    type Item = Result<ExtractedFile<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.0.next()? {
                Ok(Instruction::File(file)) => return Some(Ok(file)),
                Ok(_) => continue,
                Err(e) => return Some(Err(e)),
            }
        }
    }
}
