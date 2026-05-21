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
    installer::NsisInstaller,
    nsis::entry::{Entry, EntryIter},
    opcode,
    strings::NsisString,
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
/// In solid mode, all files are concatenated in a single compressed stream.
/// Individual file data cannot be sliced from the original buffer without
/// decompressing the entire stream. [`data`](Self::data) returns an empty
/// slice, and [`decompress`](Self::decompress) is not yet supported for
/// solid installers (it returns an error).
pub struct ExtractedFile<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> ExtractedFile<'a> {
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
            return Err(Error::TooShort {
                expected: 4,
                actual: 0,
                context: "file data length prefix",
            });
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

        let payload = source.get(offset..end).ok_or(Error::TooShort {
            expected: end,
            actual: source.len(),
            context: "file data payload",
        })?;

        if !is_compressed {
            return Ok(payload.to_vec());
        }

        let max_output = (size as usize).saturating_mul(10).max(64 * 1024 * 1024);
        decompress::decompress_block(
            payload,
            self.installer.compression(),
            max_output,
            Some(max_output),
        )
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

    /// Validates that the file length prefix and payload are within the source.
    fn validate_data_bounds(&self) -> Result<(), Error> {
        let source = self.data_source();
        let prefix_offset = self.source_offset();
        let prefix_end = prefix_offset.checked_add(4).ok_or(Error::TooShort {
            expected: usize::MAX,
            actual: source.len(),
            context: "file data length prefix",
        })?;
        let prefix = source
            .get(prefix_offset..prefix_end)
            .ok_or(Error::TooShort {
                expected: prefix_end,
                actual: source.len(),
                context: "file data length prefix",
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
            .ok_or(Error::TooShort {
                expected: payload_end,
                actual: source.len(),
                context: "file data payload",
            })
    }
}

/// Iterator over embedded files in an NSIS installer.
///
/// Scans all `EW_EXTRACTFILE` entries in the script and yields an
/// [`ExtractedFile`] for each one.
pub struct FileIter<'a> {
    installer: &'a NsisInstaller<'a>,
    entries: EntryIter<'a>,
}

impl<'a> FileIter<'a> {
    pub(crate) fn new(installer: &'a NsisInstaller<'a>, entries: EntryIter<'a>) -> Self {
        Self { installer, entries }
    }
}

impl<'a> Iterator for FileIter<'a> {
    type Item = Result<ExtractedFile<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry_result = self.entries.next()?;
            match entry_result {
                Ok(entry) => {
                    if self.installer.normalize_opcode(entry.which()) == opcode::EW_EXTRACTFILE {
                        let file = ExtractedFile {
                            installer: self.installer,
                            entry,
                        };
                        if let Err(e) = file.validate_data_bounds() {
                            return Some(Err(e));
                        }
                        return Some(Ok(file));
                    }
                    // Skip non-EXTRACTFILE entries.
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}
