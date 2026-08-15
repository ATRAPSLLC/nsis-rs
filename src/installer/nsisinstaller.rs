//! The main NSIS installer parser entry point.

use crate::{
    addressmap::PeOverlay,
    decompress::{self, CompressionMethod, CompressionMode, DecodeLimit},
    error::Error,
    header::{
        self,
        blockheader::BlockType,
        commonheader::CommonHeader,
        firstheader::{FH_FLAGS_UNINSTALL, FirstHeader},
    },
    installer::{
        analysis::{ExecIter, PluginCallIter, RegistryIter, ShortcutIter, UninstallerIter},
        callback::Callback,
        files::FileIter,
        script::{self, ScriptAnalysis},
    },
    nsis::{
        entry::{Entry, EntryIter},
        langtable::LangTableIter,
        page::PageIter,
        section::{Section, SectionIter},
    },
    opcode::{self, Nsis2SubVersion, NsisVersion, OpcodeInfo, ParkSubVersion, info::ParamType},
    strings::{self, NsisString, StringEncoding, StringSegment, StringTable, ansi::AnsiCodeRange},
};

/// The outcome of decompressing an installer's solid file-data stream.
///
/// Solid installers hold every embedded file in one compressed stream, which
/// the parser decodes up front under the caller's
/// [`max_decompressed_size`](NsisInstaller::max_decompressed_size) budget. A
/// stream that fails or overruns that budget does not fail the parse — header
/// structures remain fully readable — so this records why the file data is
/// absent or incomplete.
///
/// Read it through [`NsisInstaller::solid_status`]. Files that fall past a
/// truncation point, or any file at all when decoding failed, report the
/// corresponding error from [`FileIter`] and
/// [`ExtractedFile::decompress`](crate::installer::ExtractedFile::decompress)
/// rather than a bounds error that looks like data corruption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolidStatus {
    /// The installer is not solid; file data is sliced from the original file.
    NotSolid,
    /// The whole solid stream decompressed within the budget.
    Complete,
    /// Decoding stopped at the budget. Files stored before that point are
    /// still readable; the rest report [`Error::OutputTooLarge`].
    Truncated {
        /// The budget the stream was cut at, in bytes.
        limit: usize,
    },
    /// Decoding failed outright, leaving no file data. Every file reports this
    /// error.
    Failed(Error),
}

/// Parsed NSIS installer providing access to all internal structures.
///
/// The installer borrows the original file bytes (`'a` lifetime) and owns
/// the decompressed header data. All view types returned by accessor methods
/// borrow from one of these two buffers.
///
/// # Example
///
/// ```no_run
/// use nsis::NsisInstaller;
///
/// let file = std::fs::read("installer.exe").unwrap();
/// let inst = NsisInstaller::from_bytes(&file).unwrap();
///
/// println!("Version: {:?}", inst.version());
/// println!("Sections: {}", inst.section_count());
/// for section in inst.sections() {
///     let section = section.unwrap();
///     println!("  code_size={}", section.code_size());
/// }
/// ```
pub struct NsisInstaller<'a> {
    /// The original file bytes (borrowed).
    file: &'a [u8],
    /// Byte offset of the FirstHeader within the file.
    first_header_file_offset: usize,
    /// Decompressed header data (owned).
    header_data: Vec<u8>,
    /// Detected compression method.
    compression: CompressionMethod,
    /// Detected compression mode.
    mode: CompressionMode,
    /// Detected NSIS version.
    version: NsisVersion,
    /// Detected string encoding.
    encoding: StringEncoding,
    /// Which ANSI special-code range the string table uses.
    ///
    /// Only meaningful for an ANSI table, where NSIS 3 and NSIS 2 disagree on
    /// which byte values are codes rather than text.
    ansi_codes: AnsiCodeRange,
    /// FirstHeader flags (cached).
    first_header_flags: u32,
    /// Whether the FirstHeader uses a legacy signature.
    is_legacy: bool,
    /// Byte offset of the data block within the original file (non-solid only).
    data_block_offset: usize,
    /// Decompressed solid file data (solid mode only).
    ///
    /// In solid mode, this contains the decompressed file data stream
    /// (everything after the header in the solid decompressed stream).
    /// Each file entry is framed with a 4-byte length prefix.
    /// `EW_EXTRACTFILE` `data_offset` values are byte positions into this buffer.
    solid_data: Vec<u8>,
    /// Outcome of decompressing the solid stream (solid mode only).
    solid_status: SolidStatus,
    /// Parsed block offsets and counts: (offset_in_header, item_count).
    blocks: [(u32, i32); 8],
    /// Common header flags.
    common_flags: u32,
    /// Language table entry size.
    langtable_size: i32,
    /// On-disk section size (base 24 bytes + optional inline name buffer).
    section_size: usize,
    /// Callback entry indices from the common header.
    ///
    /// Order: onInit, onInstSuccess, onInstFailed, onUserAbort, onGUIInit,
    /// onGUIEnd, onMouseOverSection, onVerifyInstDir, onSelChange, onRebootFailed.
    /// Value of -1 means the callback is not defined.
    callbacks: [i32; 10],
    /// Park sub-version (only meaningful when `version == Park`).
    park_sub: Option<ParkSubVersion>,
    /// NSIS 2 variable layout (only meaningful when `version == V2`).
    nsis2_sub: Option<Nsis2SubVersion>,
    /// Maximum decompressed size budget for unknown-size streams.
    ///
    /// Applied per decompression call to embedded files, the solid file-data
    /// stream, and uninstaller overlays. A stream that would expand past this
    /// budget is rejected with [`Error::OutputTooLarge`] rather than silently
    /// truncated. Set via [`NsisInstaller::builder`].
    max_decompressed_size: usize,
}

impl<'a> NsisInstaller<'a> {
    /// Default decompression budget for unknown-size streams (64 MiB).
    ///
    /// Used by [`from_bytes`](Self::from_bytes) and as the
    /// [`builder`](Self::builder)'s starting value. Override per-parse with
    /// [`NsisInstallerBuilder::max_decompressed_size`].
    pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 64 * 1024 * 1024;

    /// Parses an NSIS installer from the given file bytes using default limits.
    ///
    /// Equivalent to `NsisInstaller::builder(file).parse()`. To configure the
    /// decompression budget, use [`builder`](Self::builder) instead.
    ///
    /// # Errors
    ///
    /// Returns an error if any step fails (not a PE, no overlay, no NSIS
    /// signature, decompression failure, invalid headers).
    pub fn from_bytes(file: &'a [u8]) -> Result<Self, Error> {
        Self::builder(file).parse()
    }

    /// Creates a [`NsisInstallerBuilder`] for parsing `file` with configurable
    /// limits.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use nsis::NsisInstaller;
    ///
    /// let file = std::fs::read("installer.exe").unwrap();
    /// let inst = NsisInstaller::builder(&file)
    ///     .max_decompressed_size(256 * 1024 * 1024)
    ///     .parse()
    ///     .unwrap();
    /// # let _ = inst.version();
    /// ```
    pub fn builder(file: &'a [u8]) -> NsisInstallerBuilder<'a> {
        NsisInstallerBuilder {
            file,
            max_decompressed_size: Self::DEFAULT_MAX_DECOMPRESSED_SIZE,
        }
    }

    /// Parses an NSIS installer with an explicit decompression budget.
    ///
    /// This performs the full parsing pipeline:
    /// 1. Locate PE overlay
    /// 2. Scan for FirstHeader at 512-byte aligned offsets
    /// 3. Decompress the header block
    /// 4. Parse the common header and block descriptors
    /// 5. Detect string encoding and NSIS version
    ///
    /// # Errors
    ///
    /// Returns an error if any step fails (not a PE, no overlay, no NSIS
    /// signature, decompression failure, invalid headers).
    fn parse_with_budget(file: &'a [u8], max_decompressed_size: usize) -> Result<Self, Error> {
        // Step 1: Locate the PE overlay.
        let overlay_info = PeOverlay::from_bytes(file)?;
        let overlay = overlay_info.overlay();
        let overlay_file_offset = overlay_info.overlay_offset();

        // Step 2: Scan for the FirstHeader.
        let (fh_overlay_offset, first_header) = header::scan_for_first_header(overlay)?;
        let first_header_file_offset =
            overlay_file_offset
                .checked_add(fh_overlay_offset)
                .ok_or(Error::TooShort {
                    expected: usize::MAX,
                    actual: overlay.len(),
                    context: "first header file offset overflow",
                })?;
        let is_legacy = first_header.is_legacy();

        // Data after the FirstHeader.
        let after_fh_start =
            fh_overlay_offset
                .checked_add(FirstHeader::SIZE)
                .ok_or(Error::TooShort {
                    expected: usize::MAX,
                    actual: overlay.len(),
                    context: "first header overflow",
                })?;
        let after_fh = overlay.get(after_fh_start..).ok_or(Error::TooShort {
            expected: after_fh_start.saturating_add(1),
            actual: overlay.len(),
            context: "data after FirstHeader",
        })?;
        if after_fh.is_empty() {
            return Err(Error::TooShort {
                expected: after_fh_start.saturating_add(1),
                actual: overlay.len(),
                context: "data after FirstHeader",
            });
        }

        // Step 3: Decompress the header block.
        let expected_size = first_header.length_of_header() as usize;
        let (header_data, compression, mode, header_compressed_size) =
            decompress::decompress_header(after_fh, expected_size)?;

        // Step 4: Parse the common header and extract all values before
        // moving header_data into the struct (CommonHeader borrows header_data).
        let (blocks, common_flags, langtable_size, callbacks) = {
            let common_header = CommonHeader::parse(&header_data)?;
            let mut blocks = [(0u32, 0i32); 8];
            for (block, bh) in blocks.iter_mut().zip(common_header.blocks().iter()) {
                *block = (bh.offset(), bh.num());
            }
            let callbacks = [
                common_header.code_on_init(),
                common_header.code_on_inst_success(),
                common_header.code_on_inst_failed(),
                common_header.code_on_user_abort(),
                common_header.code_on_gui_init(),
                common_header.code_on_gui_end(),
                common_header.code_on_mouse_over_section(),
                common_header.code_on_verify_inst_dir(),
                common_header.code_on_sel_change(),
                common_header.code_on_reboot_failed(),
            ];
            (
                blocks,
                common_header.flags(),
                common_header.langtable_size(),
                callbacks,
            )
        };

        // Compute section size from block header gaps (7-Zip NsisIn.cpp line 5260).
        let (sec_off_raw, sec_count_raw) = blocks
            .get(BlockType::Sections as usize)
            .copied()
            .unwrap_or((0, 0));
        let (ent_off_raw, ent_count_raw) = blocks
            .get(BlockType::Entries as usize)
            .copied()
            .unwrap_or((0, 0));
        let sec_offset = sec_off_raw as usize;
        let sec_count = sec_count_raw.max(0) as usize;
        let ent_offset = ent_off_raw as usize;
        let section_size = if ent_offset > sec_offset {
            ent_offset
                .saturating_sub(sec_offset)
                .checked_div(sec_count)
                .unwrap_or(Section::BASE_SIZE)
        } else {
            Section::BASE_SIZE
        };

        // Step 5: Detect string encoding and version.
        let string_block_offset = blocks
            .get(BlockType::Strings as usize)
            .copied()
            .unwrap_or((0, 0))
            .0 as usize;
        let string_table = header_data.get(string_block_offset..).unwrap_or(&[]);
        let encoding = if string_table.is_empty() {
            StringEncoding::Ansi
        } else {
            strings::detect_encoding(string_table)
        };

        let version = NsisVersion::detect(encoding, is_legacy, string_table);
        let ansi_codes = AnsiCodeRange::for_version(version);

        // Step 5b: Detect Park sub-version by scanning entries.
        let ent_count = ent_count_raw.max(0) as usize;
        let park_sub = if version == NsisVersion::Park {
            Some(opcode::detect_park_sub_version(
                &header_data,
                ent_offset,
                ent_count,
            ))
        } else {
            None
        };

        // Step 5c: NSIS 2 moved its built-in variables around, so an ANSI
        // NSIS-2 installer needs its variable layout pinned down before any
        // variable reference can be decoded correctly.
        let nsis2_sub = if version == NsisVersion::V2 {
            Some(opcode::detect_nsis2_sub_version(
                &header_data,
                ent_offset,
                ent_count,
                |offset| {
                    // A provisional table: the variable *count* is not known
                    // until this detection finishes. That is sound because the
                    // detection compares variable indices, never their names.
                    let table = StringTable::new(
                        &header_data,
                        string_block_offset,
                        encoding,
                        ansi_codes,
                        strings::DEFAULT_INTERNAL_VARS,
                    );
                    match table.read(offset).ok()?.segments.as_slice() {
                        [StringSegment::Variable { index, .. }] => Some(*index),
                        _ => None,
                    }
                },
            ))
        } else {
            None
        };

        // Step 6: Handle data block.
        let (data_block_offset, solid_data, solid_status) = if mode == CompressionMode::Solid {
            // In solid mode, the entire post-FirstHeader data is one compressed
            // stream. Decompress the full stream to get the file data.
            //
            // Decompressed stream: [4B header_len][header][4B file1_len][file1]...
            //
            // The NSIS overlay may be followed by extra data (digital signatures,
            // padding) beyond what `length_of_all_following_data` covers. Also,
            // the last 4 bytes within the NSIS data may be a CRC32 that is NOT
            // part of the compressed stream. Trim to the exact NSIS data bounds
            // and exclude the CRC to avoid LZMA "trailing bytes" errors.
            let nsis_data_len = (first_header.length_of_all_following_data() as usize)
                .saturating_sub(FirstHeader::SIZE);
            let has_crc = !first_header.has_no_crc();
            let stream_len = if has_crc {
                nsis_data_len.saturating_sub(4)
            } else {
                nsis_data_len
            };
            let compressed_data = after_fh
                .get(..stream_len.min(after_fh.len()))
                .unwrap_or(after_fh);

            // Decompress the full stream under the caller's budget. Header
            // parsing has already succeeded, so a failure here degrades to
            // "no file data" rather than failing the whole parse — but the
            // reason is recorded in `solid_status` so that file access can
            // report it instead of a misleading bounds error.
            let (full_stream, status) = match decompress::decompress_block(
                compressed_data,
                compression,
                DecodeLimit::Truncate(max_decompressed_size),
            ) {
                Ok(decoded) => {
                    let status = if decoded.truncated {
                        SolidStatus::Truncated {
                            limit: max_decompressed_size,
                        }
                    } else {
                        SolidStatus::Complete
                    };
                    (decoded.data, status)
                }
                Err(e) => (Vec::new(), SolidStatus::Failed(e)),
            };

            // The first 4 bytes are the header length prefix, then header_data.
            // File data starts after: 4 + header_data.len()
            let file_data_start = header_data.len().saturating_add(4);
            let solid_file_data = full_stream
                .get(file_data_start..)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();

            (0, solid_file_data, status)
        } else {
            // In non-solid mode, the data block follows the compressed header.
            // `header_compressed_size` already includes the 4-byte length prefix.
            let offset = first_header_file_offset
                .saturating_add(FirstHeader::SIZE)
                .saturating_add(header_compressed_size);
            (offset, Vec::new(), SolidStatus::NotSolid)
        };

        Ok(Self {
            file,
            first_header_file_offset,
            header_data,
            compression,
            mode,
            version,
            encoding,
            ansi_codes,
            first_header_flags: first_header.flags(),
            is_legacy,
            data_block_offset,
            solid_data,
            solid_status,
            blocks,
            common_flags,
            langtable_size,
            section_size,
            callbacks,
            park_sub,
            nsis2_sub,
            max_decompressed_size,
        })
    }

    /// Returns the decompression budget for unknown-size streams.
    ///
    /// See [`NsisInstallerBuilder::max_decompressed_size`].
    #[inline]
    pub fn max_decompressed_size(&self) -> usize {
        self.max_decompressed_size
    }

    /// Returns the detected NSIS version.
    #[inline]
    pub fn version(&self) -> NsisVersion {
        self.version
    }

    /// Returns the NSIS 2 variable layout, if this is an NSIS 2 installer.
    ///
    /// `None` for every other version. NSIS 2 moved its built-in variables
    /// between releases, so decoding a variable reference — or locating the
    /// internal `$_OUTDIR` — depends on which layout applies. See
    /// [`Nsis2SubVersion`].
    #[inline]
    pub fn nsis2_sub_version(&self) -> Option<Nsis2SubVersion> {
        self.nsis2_sub
    }

    /// Returns the detected compression method.
    #[inline]
    pub fn compression(&self) -> CompressionMethod {
        self.compression
    }

    /// Returns the detected compression mode (solid or non-solid).
    #[inline]
    pub fn compression_mode(&self) -> CompressionMode {
        self.mode
    }

    /// Returns the detected string encoding.
    #[inline]
    pub fn string_encoding(&self) -> StringEncoding {
        self.encoding
    }

    /// Returns `true` if this is an uninstaller.
    #[inline]
    pub fn is_uninstaller(&self) -> bool {
        self.first_header_flags & FH_FLAGS_UNINSTALL != 0
    }

    /// Returns `true` if this is a legacy NSIS 1.x installer.
    #[inline]
    pub fn is_legacy(&self) -> bool {
        self.is_legacy
    }

    /// Returns the byte offset of the FirstHeader in the file.
    #[inline]
    pub fn first_header_file_offset(&self) -> usize {
        self.first_header_file_offset
    }

    /// Returns the common header flags.
    #[inline]
    pub fn common_flags(&self) -> u32 {
        self.common_flags
    }

    /// Returns a reference to the decompressed header data.
    #[inline]
    pub fn header_data(&self) -> &[u8] {
        &self.header_data
    }

    /// Returns the original file bytes.
    #[inline]
    pub fn file_data(&self) -> &'a [u8] {
        self.file
    }

    /// Returns the (offset, count) pair for a block, or `(0, 0)` if missing.
    fn block_info(&self, bt: BlockType) -> (u32, i32) {
        self.blocks.get(bt as usize).copied().unwrap_or((0, 0))
    }

    /// Returns the slice of `header_data` starting at the given block's offset.
    fn block_data(&self, bt: BlockType) -> &[u8] {
        let (offset, _) = self.block_info(bt);
        self.header_data.get(offset as usize..).unwrap_or(&[])
    }

    /// Returns the number of sections.
    #[inline]
    pub fn section_count(&self) -> usize {
        self.block_info(BlockType::Sections).1.max(0) as usize
    }

    /// Returns the number of entries (instructions).
    #[inline]
    pub fn entry_count(&self) -> usize {
        self.block_info(BlockType::Entries).1.max(0) as usize
    }

    /// Returns the number of pages.
    #[inline]
    pub fn page_count(&self) -> usize {
        self.block_info(BlockType::Pages).1.max(0) as usize
    }

    /// Returns an iterator over install sections.
    pub fn sections(&self) -> SectionIter<'_> {
        let (_, count) = self.block_info(BlockType::Sections);
        let is_unicode = matches!(
            self.encoding,
            StringEncoding::Unicode | StringEncoding::Park
        );
        SectionIter::new(
            self.block_data(BlockType::Sections),
            count.max(0) as usize,
            self.section_size,
            is_unicode,
        )
    }

    /// Returns an iterator over bytecode entries (instructions).
    pub fn entries(&self) -> EntryIter<'_> {
        let (_, count) = self.block_info(BlockType::Entries);
        EntryIter::new(self.block_data(BlockType::Entries), count.max(0) as usize)
    }

    /// Returns an iterator over installer pages.
    pub fn pages(&self) -> PageIter<'_> {
        let (_, count) = self.block_info(BlockType::Pages);
        PageIter::new(self.block_data(BlockType::Pages), count.max(0) as usize)
    }

    /// Returns an iterator over language tables.
    pub fn lang_tables(&self) -> LangTableIter<'_> {
        let (_, count) = self.block_info(BlockType::LangTables);
        let entry_size = self.langtable_size.max(8) as usize;
        LangTableIter::new(
            self.block_data(BlockType::LangTables),
            count.max(0) as usize,
            entry_size,
        )
    }

    /// Reads and decodes a string from the string table at the given offset.
    ///
    /// The `offset` is a TCHAR index into the string table, as stored in
    /// section `name_ptr` fields and entry parameter slots. For Unicode
    /// installers (NSIS 3.x), each TCHAR is 2 bytes, so the byte position
    /// is `offset * 2`. For ANSI installers, each TCHAR is 1 byte.
    pub fn read_string(&self, offset: i32) -> Result<NsisString, Error> {
        self.string_table().read(offset)
    }

    /// Returns a view of this installer's string table.
    ///
    /// Carries the encoding, special-code range and variable layout that
    /// decoding depends on. [`read_string`](Self::read_string) is the shorthand
    /// for `string_table().read(offset)`.
    pub fn string_table(&self) -> StringTable<'_> {
        StringTable::new(
            &self.header_data,
            self.block_info(BlockType::Strings).0 as usize,
            self.encoding,
            self.ansi_codes,
            self.nsis2_sub
                .map_or(strings::DEFAULT_INTERNAL_VARS, |sub| {
                    sub.internal_var_count()
                }),
        )
    }

    /// Returns an iterator over embedded files (`EW_EXTRACTFILE` entries).
    ///
    /// Each [`ExtractedFile`](crate::installer::ExtractedFile) provides the
    /// file name, raw data as a borrowed slice (zero-copy), and a
    /// [`decompress`](crate::installer::ExtractedFile::decompress) method.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let data = std::fs::read("installer.exe").unwrap();
    /// # let inst = nsis::NsisInstaller::from_bytes(&data).unwrap();
    /// for file in inst.files() {
    ///     let file = file.unwrap();
    ///     println!("{}: {} bytes", file.name().unwrap(), file.data().len());
    /// }
    /// ```
    pub fn files(&self) -> FileIter<'_> {
        let (_, count) = self.block_info(BlockType::Entries);
        let entries = EntryIter::new(self.block_data(BlockType::Entries), count.max(0) as usize);
        FileIter::new(self, entries)
    }

    /// Normalizes a raw opcode to its V2-equivalent number.
    ///
    /// For Park version, raw opcodes are shifted due to inserted extra
    /// opcodes. This method reverses that shift. For other versions the
    /// raw opcode is returned unchanged.
    #[inline]
    pub fn normalize_opcode(&self, raw: i32) -> i32 {
        if raw < 0 {
            return raw;
        }
        if self.version == NsisVersion::Park
            && let Some(sub) = self.park_sub
        {
            return opcode::normalize_park_opcode(raw as u32, sub) as i32;
        }
        raw
    }

    /// Resolves an opcode index to its metadata.
    ///
    /// For Park version, the raw opcode is first normalized to its V2
    /// equivalent before lookup.
    pub fn resolve_opcode(&self, which: i32) -> Option<&'static OpcodeInfo> {
        if which < 0 {
            return None;
        }
        opcode::lookup_normalized(self.version, which as u32, self.park_sub)
    }

    /// Formats an entry as a single script-like line.
    ///
    /// This resolves the opcode mnemonic and formats each known parameter using
    /// the opcode metadata. It also handles opcodes whose parameter meaning is
    /// conditional, such as [`opcode::EW_PUSHPOP`].
    ///
    /// # Arguments
    ///
    /// * `entry` - An entry returned by [`entries`](Self::entries),
    ///   [`section_entries`](Self::section_entries), or another crate API.
    ///
    /// # Returns
    ///
    /// A stable, diagnostic string containing the `EW_*` mnemonic and decoded
    /// operands. Unknown opcodes are rendered as `"???"` plus the raw opcode.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # let bytes = std::fs::read("installer.exe").unwrap();
    /// # let installer = nsis::NsisInstaller::from_bytes(&bytes).unwrap();
    /// for entry in installer.entries() {
    ///     let entry = entry.unwrap();
    ///     println!("{}", installer.format_entry(&entry));
    /// }
    /// ```
    pub fn format_entry(&self, entry: &Entry<'_>) -> String {
        let mnemonic = self
            .resolve_opcode(entry.which())
            .map(|info| info.mnemonic)
            .unwrap_or("???");
        let params = self.format_entry_params(entry);
        if params.is_empty() {
            mnemonic.to_string()
        } else {
            format!("{mnemonic} {params}")
        }
    }

    /// Formats the parameters for an entry using opcode-aware resolution.
    ///
    /// String offsets are decoded through the installer string table,
    /// variables are rendered with conventional NSIS names, jump targets are
    /// marked with `=>`, and unused operands are omitted.
    ///
    /// # Arguments
    ///
    /// * `entry` - The entry whose parameter slots should be formatted.
    ///
    /// # Returns
    ///
    /// A comma-separated operand string without the opcode mnemonic. Returns an
    /// empty string for opcodes with no meaningful operands.
    pub fn format_entry_params(&self, entry: &Entry<'_>) -> String {
        let Some(info) = self.resolve_opcode(entry.which()) else {
            return format!("which={}", entry.which());
        };

        if entry.which() == opcode::EW_PUSHPOP {
            return self.format_pushpop_params(entry);
        }

        let offsets = entry.offsets();
        let count = info.param_count as usize;
        if count == 0 {
            return String::new();
        }

        let mut parts = Vec::new();
        for (i, ((&val, &name), &ptype)) in offsets
            .iter()
            .zip(info.param_names.iter())
            .zip(info.param_types.iter())
            .take(count)
            .enumerate()
        {
            match ptype {
                ParamType::String => parts.push(format_param(name, self.format_string_param(val))),
                ParamType::Variable => {
                    parts.push(format_param(
                        name,
                        format_variable_param(&self.string_table(), val),
                    ));
                }
                ParamType::Jump => {
                    if val != 0 {
                        if name.is_empty() {
                            parts.push(format!("=>{val}"));
                        } else {
                            parts.push(format!("{name}=>{val}"));
                        }
                    }
                }
                ParamType::Int => {
                    if val != 0 || i < count.min(2) {
                        parts.push(format_param(name, val.to_string()));
                    }
                }
                ParamType::Unused => {}
            }
        }
        parts.join(", ")
    }

    /// Formats an entry as a single script-like line with analysis symbols.
    ///
    /// Jump and call operands that target known entries are annotated with the
    /// best symbol from [`ScriptAnalysis`], such as a callback, section, label,
    /// or discovered function name.
    ///
    /// # Arguments
    ///
    /// * `entry` - The entry to format.
    /// * `analysis` - A script analysis produced by
    ///   [`script_analysis`](Self::script_analysis) for the same installer.
    ///
    /// # Returns
    ///
    /// A single-line mnemonic plus operand string. Static jump/call targets are
    /// rendered with symbols where available, for example
    /// `jump_addr=>label_42(@42)`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # let bytes = std::fs::read("installer.exe").unwrap();
    /// # let installer = nsis::NsisInstaller::from_bytes(&bytes).unwrap();
    /// let analysis = installer.script_analysis().unwrap();
    /// for entry in installer.entries() {
    ///     let entry = entry.unwrap();
    ///     println!("{}", installer.format_entry_with_analysis(&entry, &analysis));
    /// }
    /// ```
    pub fn format_entry_with_analysis(
        &self,
        entry: &Entry<'_>,
        analysis: &ScriptAnalysis,
    ) -> String {
        let mnemonic = self
            .resolve_opcode(entry.which())
            .map(|info| info.mnemonic)
            .unwrap_or("???");
        let params = self.format_entry_params_with_analysis(entry, analysis);
        if params.is_empty() {
            mnemonic.to_string()
        } else {
            format!("{mnemonic} {params}")
        }
    }

    /// Formats entry parameters with control-flow symbols from an analysis.
    ///
    /// # Arguments
    ///
    /// * `entry` - The entry whose parameters should be formatted.
    /// * `analysis` - A script analysis produced by
    ///   [`script_analysis`](Self::script_analysis) for the same installer.
    ///
    /// # Returns
    ///
    /// A comma-separated operand string without the opcode mnemonic. Static
    /// jump/call targets are annotated with symbols when the analysis knows
    /// one.
    pub fn format_entry_params_with_analysis(
        &self,
        entry: &Entry<'_>,
        analysis: &ScriptAnalysis,
    ) -> String {
        let Some(info) = self.resolve_opcode(entry.which()) else {
            return format!("which={}", entry.which());
        };

        if entry.which() == opcode::EW_PUSHPOP {
            return self.format_pushpop_params(entry);
        }

        let offsets = entry.offsets();
        let count = info.param_count as usize;
        if count == 0 {
            return String::new();
        }

        let mut parts = Vec::new();
        for (i, ((&val, &name), &ptype)) in offsets
            .iter()
            .zip(info.param_names.iter())
            .zip(info.param_types.iter())
            .take(count)
            .enumerate()
        {
            match ptype {
                ParamType::String => parts.push(format_param(name, self.format_string_param(val))),
                ParamType::Variable => {
                    parts.push(format_param(
                        name,
                        format_variable_param(&self.string_table(), val),
                    ));
                }
                ParamType::Jump => {
                    if val != 0 {
                        parts.push(format_symbolic_jump_param(
                            &self.string_table(),
                            name,
                            val,
                            analysis,
                        ));
                    }
                }
                ParamType::Int => {
                    if val != 0 || i < count.min(2) {
                        parts.push(format_param(name, val.to_string()));
                    }
                }
                ParamType::Unused => {}
            }
        }
        parts.join(", ")
    }

    /// Builds a script-level control-flow analysis.
    ///
    /// The analysis identifies section/callback/page/function roots, basic
    /// blocks, and static or dynamic control-flow edges.
    ///
    /// # Returns
    ///
    /// A [`ScriptAnalysis`] containing owned graph data. The result does not
    /// borrow entries, so consumers can serialize or cache it independently of
    /// entry iterator lifetimes.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if an entry, section, or page structure fails to parse
    /// while collecting roots and control-flow edges.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # let bytes = std::fs::read("installer.exe").unwrap();
    /// # let installer = nsis::NsisInstaller::from_bytes(&bytes).unwrap();
    /// let analysis = installer.script_analysis().unwrap();
    /// for edge in &analysis.edges {
    ///     println!("{:?} -> {:?}", edge.kind, edge.target);
    /// }
    /// ```
    pub fn script_analysis(&self) -> Result<ScriptAnalysis, Error> {
        script::analyze(self)
    }

    fn format_pushpop_params(&self, entry: &Entry<'_>) -> String {
        let offsets = entry.offsets();
        if offsets[2] != 0 {
            if offsets[2] == 1 {
                "op=Exch".to_string()
            } else {
                format!("op=Exch, index={}", offsets[2])
            }
        } else if offsets[1] != 0 {
            format!(
                "op=Pop, var={}",
                format_variable_param(&self.string_table(), offsets[0])
            )
        } else {
            format!("op=Push, value={}", self.format_string_param(offsets[0]))
        }
    }

    fn format_string_param(&self, offset: i32) -> String {
        if offset > 0
            && let Ok(value) = self.read_string(offset)
        {
            let value = value.to_string();
            if !value.is_empty() {
                return format!("{value:?}");
            }
        }
        offset.to_string()
    }

    /// Returns the data block offset within the original file (non-solid only).
    #[inline]
    pub fn data_block_offset(&self) -> usize {
        self.data_block_offset
    }

    /// Returns the decompressed solid file data (solid mode only).
    ///
    /// Each file entry in this buffer is framed with a 4-byte length prefix.
    /// Returns an empty slice for non-solid installers.
    ///
    /// An empty or short buffer is not necessarily corruption — consult
    /// [`solid_status`](Self::solid_status) to tell a complete stream from one
    /// that hit the decompression budget or failed to decode.
    #[inline]
    pub fn solid_data(&self) -> &[u8] {
        &self.solid_data
    }

    /// Returns how the solid file-data stream decompressed.
    ///
    /// Always [`SolidStatus::NotSolid`] for non-solid installers. For solid
    /// ones it distinguishes a complete decode from a stream cut short by the
    /// [`max_decompressed_size`](Self::max_decompressed_size) budget or one
    /// that failed outright — neither of which fails the parse, since header
    /// structures stay readable either way.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use nsis::{NsisInstaller, SolidStatus};
    ///
    /// let file = std::fs::read("installer.exe").unwrap();
    /// let inst = NsisInstaller::from_bytes(&file).unwrap();
    ///
    /// if let SolidStatus::Truncated { limit } = inst.solid_status() {
    ///     eprintln!("solid stream cut at {limit} bytes; re-parse with a larger budget");
    /// }
    /// ```
    #[inline]
    pub fn solid_status(&self) -> &SolidStatus {
        &self.solid_status
    }

    /// Returns an iterator over the entries belonging to the given section.
    ///
    /// Uses the section's `code` (start index) and `code_size` (count) to
    /// slice the entries block.
    pub fn section_entries(&self, section: &Section<'_>) -> EntryIter<'_> {
        let (block_offset, _) = self.block_info(BlockType::Entries);
        let code = section.code().max(0) as usize;
        let count = section.code_size().max(0) as usize;
        let Some(byte_offset) = code
            .checked_mul(Entry::SIZE)
            .and_then(|n| n.checked_add(block_offset as usize))
        else {
            return EntryIter::new(&[], 0);
        };
        match self.header_data.get(byte_offset..) {
            Some(slice) => EntryIter::new(slice, count),
            None => EntryIter::new(&[], 0),
        }
    }

    /// Returns the entry index for the `.onInit` callback.
    ///
    /// Called before the installer UI is shown. Takes no parameters and has
    /// no return value. This is the primary callback used by malware to
    /// perform early payload decryption, memory allocation, and anti-analysis
    /// checks before any user interaction.
    pub fn on_init(&self) -> Option<usize> {
        Self::callback_index(self.callbacks.first().copied().unwrap_or(-1))
    }

    /// Returns the entry index for the `.onInstSuccess` callback.
    ///
    /// Called after all sections have been successfully executed. Takes no
    /// parameters. Often used by malware to launch the decrypted payload
    /// or perform cleanup after installation completes.
    pub fn on_inst_success(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[1])
    }

    /// Returns the entry index for the `.onInstFailed` callback.
    ///
    /// Called when installation fails or is aborted by an error. Takes no
    /// parameters.
    pub fn on_inst_failed(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[2])
    }

    /// Returns the entry index for the `.onUserAbort` callback.
    ///
    /// Called when the user clicks Cancel. Takes no parameters. Can call
    /// `Abort` to prevent the cancellation.
    pub fn on_user_abort(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[3])
    }

    /// Returns the entry index for the `.onGUIInit` callback.
    ///
    /// Called after the installer dialog has been created but before it is
    /// shown. Receives `$HWNDPARENT` as the parent window handle. Used for
    /// custom UI modifications.
    pub fn on_gui_init(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[4])
    }

    /// Returns the entry index for the `.onGUIEnd` callback.
    ///
    /// Called after the installer dialog is destroyed. Takes no parameters.
    pub fn on_gui_end(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[5])
    }

    /// Returns the entry index for the `.onMouseOverSection` callback.
    ///
    /// Called when the mouse hovers over a section in the component page.
    /// Receives the section index in `$0`.
    pub fn on_mouse_over_section(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[6])
    }

    /// Returns the entry index for the `.onVerifyInstDir` callback.
    ///
    /// Called every time the user changes the install directory. Can call
    /// `Abort` to reject the directory. Takes no parameters; the directory
    /// is in `$INSTDIR`.
    pub fn on_verify_inst_dir(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[7])
    }

    /// Returns the entry index for the `.onSelChange` callback.
    ///
    /// Called when the user changes the section selection on the components
    /// page. Takes no parameters.
    pub fn on_sel_change(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[8])
    }

    /// Returns the entry index for the `.onRebootFailed` callback.
    ///
    /// Called if the reboot (via `EW_REBOOT`) fails. Takes no parameters.
    pub fn on_reboot_failed(&self) -> Option<usize> {
        Self::callback_index(self.callbacks[9])
    }

    /// Returns `true` if the section at `section_idx` contains `entry_idx`
    /// in its code range.
    ///
    /// Returns `false` if `section_idx` is out of range or the section
    /// fails to parse. Equivalent to `self.sections().nth(section_idx)`
    /// followed by [`Section::contains_entry`], but without exposing the
    /// `Result<Section, _>` and signed-arithmetic boundary handling to
    /// callers.
    pub fn section_contains_entry(&self, section_idx: usize, entry_idx: usize) -> bool {
        match self.sections().nth(section_idx) {
            Some(Ok(section)) => section.contains_entry(entry_idx),
            _ => false,
        }
    }

    /// Returns the entry index for a given [`Callback`] slot.
    ///
    /// Equivalent to the named `on_*` accessors but indexed by the
    /// [`Callback`] enum, which lets callers iterate over
    /// [`Callback::ALL`] without hardcoding callback names.
    pub fn callback(&self, cb: Callback) -> Option<usize> {
        let raw = self.callbacks.get(cb.index()).copied().unwrap_or(-1);
        Self::callback_index(raw)
    }

    /// Returns an iterator over plugin DLL calls in the script.
    ///
    /// Yields [`crate::installer::analysis::PluginCall`] for each `EW_REGISTERDLL` entry.
    /// This is how NSIS plugins like `System.dll`, `nsDialogs.dll` are
    /// invoked.
    pub fn plugin_calls(&self) -> PluginCallIter<'_> {
        PluginCallIter::new(self, self.make_entry_iter())
    }

    /// Returns an iterator over execution commands in the script.
    ///
    /// Yields [`crate::installer::analysis::ExecCommand`] for each `EW_EXECUTE` and
    /// `EW_SHELLEXEC` entry.
    pub fn exec_commands(&self) -> ExecIter<'_> {
        ExecIter::new(self, self.make_entry_iter())
    }

    /// Returns an iterator over registry operations in the script.
    ///
    /// Yields [`crate::installer::analysis::RegistryOp`] for each `EW_WRITEREG`, `EW_DELREG`,
    /// and `EW_READREGSTR` entry.
    pub fn registry_ops(&self) -> RegistryIter<'_> {
        RegistryIter::new(self, self.make_entry_iter())
    }

    /// Returns an iterator over shortcut creation operations.
    ///
    /// Yields [`crate::installer::analysis::Shortcut`] for each `EW_CREATESHORTCUT` entry.
    pub fn shortcuts(&self) -> ShortcutIter<'_> {
        ShortcutIter::new(self, self.make_entry_iter())
    }

    /// Returns an iterator over embedded uninstaller stubs.
    ///
    /// Yields [`crate::installer::analysis::Uninstaller`] for each `EW_WRITEUNINSTALLER` entry.
    pub fn uninstallers(&self) -> UninstallerIter<'_> {
        UninstallerIter::new(self, self.make_entry_iter())
    }

    fn callback_index(val: i32) -> Option<usize> {
        if val >= 0 { Some(val as usize) } else { None }
    }

    fn make_entry_iter(&self) -> EntryIter<'_> {
        let (_, count) = self.block_info(BlockType::Entries);
        EntryIter::new(self.block_data(BlockType::Entries), count.max(0) as usize)
    }
}

/// Configures and parses an [`NsisInstaller`].
///
/// Created via [`NsisInstaller::builder`]. Currently the only tunable is the
/// decompression budget; additional limits can be added here without changing
/// the [`NsisInstaller::from_bytes`] entry point.
///
/// # Examples
///
/// ```no_run
/// use nsis::NsisInstaller;
///
/// let file = std::fs::read("installer.exe").unwrap();
/// let inst = NsisInstaller::builder(&file)
///     .max_decompressed_size(256 * 1024 * 1024)
///     .parse()
///     .unwrap();
/// # let _ = inst.section_count();
/// ```
pub struct NsisInstallerBuilder<'a> {
    file: &'a [u8],
    max_decompressed_size: usize,
}

impl<'a> NsisInstallerBuilder<'a> {
    /// Sets the maximum decompressed size for any single unknown-size stream
    /// (embedded files, the solid file-data stream, uninstaller overlays).
    ///
    /// A stream that would expand past this budget is rejected with
    /// [`Error::OutputTooLarge`] rather than silently truncated. Defaults to
    /// [`NsisInstaller::DEFAULT_MAX_DECOMPRESSED_SIZE`].
    #[must_use]
    pub fn max_decompressed_size(mut self, bytes: usize) -> Self {
        self.max_decompressed_size = bytes;
        self
    }

    /// Parses the installer with the configured limits.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing fails (not a PE, no overlay, no NSIS
    /// signature, decompression failure, invalid headers).
    pub fn parse(self) -> Result<NsisInstaller<'a>, Error> {
        NsisInstaller::parse_with_budget(self.file, self.max_decompressed_size)
    }
}

fn format_param(name: &str, value: String) -> String {
    if name.is_empty() {
        value
    } else {
        format!("{name}={value}")
    }
}

fn format_variable_param(table: &StringTable<'_>, index: i32) -> String {
    if index >= 0 {
        table.variable_name(index as u16).into_owned()
    } else {
        index.to_string()
    }
}

fn format_symbolic_jump_param(
    table: &StringTable<'_>,
    name: &str,
    raw: i32,
    analysis: &ScriptAnalysis,
) -> String {
    let value = if raw > 0 {
        let target = usize::try_from(raw)
            .ok()
            .and_then(|value| value.checked_sub(1));
        match target {
            Some(entry) => match analysis.symbol_for_entry(entry) {
                Some(symbol) => format!("=>{symbol}(@{entry})"),
                None => format!("=>@{entry}"),
            },
            None => format!("=>invalid({raw})"),
        }
    } else if raw < 0 {
        let variable = i64::from(raw)
            .checked_neg()
            .and_then(|value| value.checked_sub(1));
        match variable.and_then(|value| i32::try_from(value).ok()) {
            Some(index) => format!("=>{}", format_variable_param(table, index)),
            None => format!("=>invalid({raw})"),
        }
    } else {
        "0".to_string()
    };

    if name.is_empty() {
        value
    } else {
        format!("{name}{value}")
    }
}
