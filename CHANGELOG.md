# Changelog

All notable changes to the `nsis` crate are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-08-15

### Added

- Support for NSIS 1.x installers, which previously failed to parse at all.
  1.x predates the block table: a fixed 240-byte header is followed by the
  section, instruction and string tables laid end to end, its sections are 20
  bytes and its instructions 24. It also has its own opcode numbering, its own
  string encoding, and its own bzip2 and first-header-flag layouts — see below.
  `header::V1Header` exposes the header, and `NsisInstaller` reads such a file
  through the same interface as any other.
- `opcode::OPCODES_V1` and `opcode::lookup_v1()`, the NSIS 1.x instruction
  table, with `opcode::canonical_opcode()` mapping a 1.x opcode onto the
  numbering this crate reports elsewhere and `opcode::param_layout_v1()` giving
  its operand layout.
- `strings::v1`, the NSIS 1.x string encoding, where a single byte at or above
  `VAR_CODES_START` is a whole variable reference.
- `StringTable::var_instdir()`, `var_outdir()`, `var_exedir()`, `var_temp()`
  and `var_pluginsdir()`, resolving a built-in variable's index through the
  installer's own layout — 1.x numbers them differently, and `$PLUGINSDIR`
  predates it entirely.
- `opcode::ParamLayout` and `opcode::param_layout()`, resolving the operand
  layout of an instruction whose opcode carries several script commands.
- `nsis::SectionLayout` and `Section::parse_with_layout()`, plus
  `EntryIter::with_stride()` and `Entry::parse_sized()`, for reading the
  narrower 1.x tables.
- `header::firstheader::FH_V1_FLAGS_*` and `NsisInstaller::is_silent()`. NSIS
  2.0 renumbered the first-header flags, so which bit means what depends on the
  version.
- `header::v1header::V1HeaderKind`. NSIS 1.x writes a different, shorter header
  for an uninstaller — 120 bytes with no section table, where an installer has
  240 followed by one. The FirstHeader says which, so an extracted 1.x
  uninstaller now decodes its own script rather than reading string bytes as
  instructions.

- `NsisInstaller::write_entry()`, `write_entry_params()` and
  `write_entry_params_with_analysis()`, rendering into a caller's buffer so a
  disassembly loop needs one allocation rather than one per line.
- `opcode::normalize_log_opcode()` and `opcode::find_bad_opcode()`, translating
  a logging-enabled build onto the standard opcode layout and reporting whether
  an entry block is consistent with a layout at all, plus
  `opcode::standard_layout()` and `opcode::log_layout()`, the two resolvers a
  build is told apart by.
- `NsisInstaller::is_log_build()`, reporting whether the installer came from a
  makensis compiled with logging.
- `NsisInstaller::instructions()`, walking the entry stream once and yielding a
  typed `Instruction`. The existing typed iterators are now filters over the
  same walk, so a consumer wanting several kinds of instruction no longer
  traverses the script several times.
- `ExtractedFile::dest_path()` and `out_dir()`, reconstructing where a file is
  actually written by following `SetOutPath`. Render with `to_install_path()`
  for 7-Zip's form or `to_path()` for a path safe to join onto an output
  directory.
- Common-header accessors for the rest of the structure:
  `NsisInstaller::install_dir()`, `install_dir_auto_append()`,
  `install_reg_key()`, `install_reg_value()`, `install_reg_root_name()`,
  `uninstall_command()`, `uninstall_child()` and `wininit_path()`.
- `strings::StringTable`, the decoding context (encoding, special-code range,
  variable layout, and the table itself), with `read()`, `variable_name()` and
  `shell_target()`. Reachable as `NsisInstaller::string_table()`.
- `strings::ShellTarget`, describing what a shell-folder reference resolves to,
  including the registry-backed Program Files and Common Files directories.
- `Debug` on the public types that lacked it, including `NsisInstaller`,
  `ExtractedFile`, every analysis view and every iterator.
- `strings::ansi::AnsiCodeRange`, selecting which ANSI special-code range a
  string table uses, with `for_version()`.
- `NsisString::to_install_path()`, rendering the path as the installer itself
  would write it — references kept verbatim, backslash separators, a leading
  `$INSTDIR\` removed — which is what 7-Zip lists for the same archive.
  `to_path()` remains the sanitised form for extraction.
- `NsisString::write_path()` and `strings::PathStyle`, the allocation-free form
  behind both renderers, for callers rendering many paths into a reused buffer.
- `Nsis2SubVersion`, identifying which NSIS 2.x variable layout an installer
  uses (`UpTo203`, `UpTo225`, `From226`), with `internal_var_count()` and
  `spec_outdir_var_index()`. Read it with `NsisInstaller::nsis2_sub_version()`,
  which returns `None` for every other version.
- `strings::detect_ansi_nsis3()`, which reports whether an ANSI string table
  uses the NSIS 3 special-code range.
- `SolidStatus`, reporting how an installer's solid file-data stream
  decompressed: `NotSolid`, `Complete`, `Truncated { limit }`, or
  `Failed(Error)`. Read it with `NsisInstaller::solid_status()`.
- `decompress::Decoded`, the `{ data, truncated }` pair now returned by the
  decompression entry points. `truncated` distinguishes a buffer that ended
  naturally from one stopped by a `DecodeLimit::Truncate` budget.

### Changed

- **Breaking:** `opcode::OPCODES` replaces `OPCODES_NSIS2` and `OPCODES_NSIS3`,
  which were the same table. NSIS 2 and NSIS 3 number their instructions
  identically; what varies is which instructions a build has, and that is
  handled by normalisation. `opcode::lookup` drops its version argument and
  `lookup_normalized` is removed — `NsisInstaller::resolve_opcode` applies
  whichever normalisation the installer needs.
- **Breaking:** `NsisInstaller::script_analysis()` returns `&ScriptAnalysis`.
  It is built on first use and borrowed thereafter, rather than rebuilt on every
  call.
- **Breaking:** `StringSegment::Variable` and `ShellFolder` are struct variants
  carrying what the reference resolved to — the variable's name, and a
  `ShellTarget` — decided while decoding, where the variable layout and string
  table are in reach. Rendering is now context-free, which is what makes
  `Display` correct: it takes no extra argument, so passing context into
  rendering could never have fixed it.
- **Breaking:** the `EW_*` opcode constants live in `opcode` rather than the
  crate root, beside the tables that give them meaning. `NsisVersion`,
  `ParkSubVersion` and `OpcodeInfo` are re-exported at the root alongside
  `Nsis2SubVersion`.
- **Breaking:** `strings::read_nsis_string` and the per-encoding readers take a
  `StringTable` instead of a byte slice plus loose parameters;
  `strings::read_string_at` is replaced by `StringTable::read`.
- **Breaking:** `strings::shell_folder_name` takes the resolved folder ids and
  target rather than a packed `u16`.
- **Breaking:** `CommonHeader::parse` no longer takes a version hint, and
  `header::NsisVersionHint` is removed. It was always `Unknown` in practice and
  never changed a parsing decision.
- `NsisString`'s `Display` now matches 7-Zip's wording for references that
  cannot be resolved statically: language strings render as `$(LSTR_n)` rather
  than `${LANG:n}`, and an unmappable shell folder as
  `$_ERROR_UNSUPPORTED_SHELL_[primary,fallback]` rather than `$SHELL(a,b)`.
- `to_path()` renders language strings as `_lang_<id>` instead of dropping
  them, so two files whose names differ only by the reference no longer collide
  on one path, and unmappable shell folders as `_shell_<primary>_<fallback>`.
- **Breaking:** `NsisVersion::detect` takes the string table as a third
  argument, which it needs to tell an ANSI NSIS-3 installer from an NSIS 2 one.
- **Breaking:** `decompress::decompress_block` and the per-codec entry points
  (`decompress_deflate`, `decompress_bzip2`, `decompress_lzma`) return
  `Decoded` instead of `Vec<u8>`. Callers that only want the bytes can take
  `.data`.

### Fixed

- NSIS 1.x installers failed to parse. The parser read every file as though it
  had a block table, so a 1.x header's string pointers were read as block
  descriptors and rejected as out of range.
- The opcode table was shifted from `EW_WRITEUNINSTALLER` up, numbering the log
  instruction as though every build has it. `SectionSetText` was reported as
  `Log`, `GetKnownFolderPath` as `InstTypeSet` and `FileWriteUTF16LE` as
  `LockWindow`. Which layout a file uses is now decided by which one its entry
  stream is consistent with.
- Instructions that pack several script commands into one opcode lost operands
  rather than merely mislabelling them: `SectionSetText` keeps its text in the
  fifth slot, which the fixed layout marks unused, so the text never appeared.
  `LogSet`'s on/off flag was read as a string offset and rendered as whatever
  text sat at offset 1.
- NSIS 1.x bzip2 streams could not be decoded. 1.x keeps standard bzip2's
  per-block randomised flag, which NSIS 2.0 dropped, so the decoder read every
  such block one bit out of alignment.
- `files()` and `section_entries()` built entry iterators with a fixed 28-byte
  stride instead of the installer's, so an installer whose entries are a
  different size yielded no files.
- `Uninstaller::data_offset()` read an operand NSIS 1.x does not have, which
  reads as `0`, so extracting a 1.x uninstaller returned whatever sat at the
  start of the data block. 1.x records the offset on the header instead.
- A block table's offsets were only range-checked, not checked for being in
  the order NSIS writes them. An NSIS 1.x header has no block table at all, so
  a large enough one passed validation and was parsed as NSIS 2 — NSIS's own
  1.98 distribution installer reported 5846 sections and no files.

- Opcodes above `EW_WRITEUNINSTALLER` are no longer off by one. The table
  numbered the log instruction as though it were always present, but NSIS
  compiles it conditionally and a standard makensis leaves it out — so
  `SectionSetText` was reported as `EW_LOG`, `GetOsInfo` as `EW_INSTTYPESET`,
  `LockWindow` as `EW_RESERVEDOPCODE`, `FileWriteUTF16LE` as `EW_LOCKWINDOW`,
  and `FindProc` had no table entry at all. Mnemonic, parameter names, types
  and count were wrong for every one of them. A logging-enabled build is
  detected and translated onto the same layout.
- Twelve opcodes had parameter counts below what NSIS actually passes,
  including `EW_WRITEREG`, whose sixth operand distinguishes `REG_EXPAND_SZ`
  from `REG_SZ` — the disassembly was dropping it. Counts now match both
  reference implementations.
- Extracted files report the directory they are written to. `files()` exposed
  only what `EW_EXTRACTFILE` stores — usually a bare name — so installers that
  place files into subdirectories through the instruction stream lost the
  structure: 7-Zip listed `Lang\de_DE.ini` where this crate said `de_DE.ini`.
  The entry walk now follows `SetOutPath`, including targets written relative
  to the current directory or to `$_OUTDIR`, whose variable index moved in NSIS
  2.26.
- ANSI shell-folder references decode from the two raw folder ids rather than
  through the 14-bit number transform, which is for numbers and folds the
  fallback id into the primary. An installer declaring
  `InstallDir "$PROGRAMFILES\App"` reported `$INTERNET\App`.
- Park keeps the fallback shell-folder id instead of discarding the high byte,
  matching the Unicode decoder.
- Registry-backed shell folders resolve their value name, so `$COMMONFILES` is
  no longer reported as `$PROGRAMFILES`.
- Variable names honour the NSIS 2 variable layout, including the shift above
  `$EXEPATH` in 2.04-2.25, rather than assuming the modern 32-variable table.
- The dump example extracts every file. It deduplicated by data offset, but
  NSIS stores one copy of duplicated content and extracts it to several
  destinations, so real files were dropped.
- ANSI string tables no longer decode with both special-code ranges at once,
  which mangled text. The decoder accepted `0x01-0x04` *and* `0xFC-0xFF` as
  codes, but only one range is live per table and each is ordinary character
  data under the other convention: `0xFC-0xFF` are the Latin-1 characters
  `ü ý þ ÿ`, and `0x01-0x04` are control characters. An NSIS 3 ANSI installer
  containing `grüße.txt` and `þýÿ.ini` reported them as `grße.txt` and
  `$PROGRAMFILES64.ini`. The range now follows the detected version.
- `to_path()` no longer keeps absolute prefixes, which let extraction escape the
  output directory. `C:\evil.exe` rendered as `C:/evil.exe`, and on Windows
  joining an absolute path onto a base discards the base — so an installer
  could write to the drive root through the crate's own extraction example.
  Drive specifiers, UNC prefixes and leading separators are now removed.
- `to_path()` handles `..` per path component instead of by substring, so
  ordinary names survive: `file..txt` stayed `file..txt` rather than becoming
  `file_txt`. Repeated separators now collapse fully (`a///b` gave `a//b`), and
  `.` components are dropped.
- ANSI installers built by makensis 3.x are no longer reported as NSIS 2.
  Version detection mapped an ANSI string table to NSIS 2 unconditionally, but
  makensis 3.x compiles an ANSI target whenever a script omits `Unicode true`.
  The two are now told apart by which special-code range the table uses, as
  7-Zip's `DetectNsisType` does. Latin-1 text cannot be mistaken for the NSIS 2
  range: `0xFC-0xFF` are the ordinary characters `ü ý þ ÿ`, so detection keys
  off the NSIS 3 codes instead.
- Installer stubs that a strict PE parse rejects are now parsed. Overlay
  detection needs only the optional header and section table, so the PE is
  parsed permissively with resources, imports, TLS, certificates and RVA
  resolution switched off, and sections whose raw range ends past EOF are
  ignored when locating the overlay. Stock output from makensis 2.03 (resource
  directory past the appended data) and from the Park 2.46.2+ Unicode fork
  (unmappable base-relocation RVA, plus a `.reloc` header claiming more bytes
  than the file holds) previously failed with a `Goblin` error. 7-Zip
  mis-detects the same Park stubs as plain PE files.
- NSIS-bzip2 no longer emits an RLE repeat count as file data. When a block's
  final BWT byte was consumed as a run's repeat count, that byte was re-emitted
  as data, inserting one spurious byte and shifting every file stored after it.
  Only streams of more than one block are affected — over ~900 KB of payload —
  where it silently corrupted the extracted data.
- Over-budget solid installers no longer report truncation as data corruption.
  The solid stream was decoded with `DecodeLimit::Truncate` and the outcome
  discarded, so every file past the cut failed with a bounds error
  (`file data payload: expected at least 75497476 bytes, got 67104998`) that
  gave no hint the budget was responsible. Such files now fail with
  `Error::OutputTooLarge { limit }`, matching the semantics single-file
  extraction has had since 0.3.0, and files stored before the cut still
  extract.
- Solid-stream decompression failures are no longer swallowed. An
  `unwrap_or_else(|_| Vec::new())` turned any decode error into an empty
  `solid_data`, leaving parsing to "succeed" while every file reported a
  bounds error. The error is now retained and reported from `files()` and
  `ExtractedFile::decompress()`. Parsing still succeeds, since header
  structures remain readable either way.

- The NSIS-bzip2 decoder no longer emits filler until the decompression budget
  is exhausted. Its BWT/RLE output loop wrote the tail of a block without
  advancing the consumed-byte counter, so it re-emitted the same byte until
  `max_output` was reached. A 38 KB solid installer decompressed to 67,104,886
  bytes — the entire 64 MiB default budget — where the real payload is 89 bytes.
  Every bzip2 solid installer was affected; raising `max_decompressed_size`
  scaled the wasted allocation with it. Decoded output is unchanged; only the
  spurious trailing filler is gone.

## [0.3.1] - 2026-08-09

### Changed

- Recorded ATRAPS LLC as copyright holder and added a `NOTICE` file. No functional change.
- Dropped the deprecated `authors` field and repointed `repository` at the organisation.
- Refreshed transitive dependencies (`cargo update`); no direct dependency changed version.
- Publishing now uses crates.io trusted publishing instead of a stored registry token.

## [0.3.0] - 2026-06-03

### Added

- `NsisInstaller::builder()` returning a `NsisInstallerBuilder` for configuring
  a parse, plus `NsisInstallerBuilder::max_decompressed_size()` to set the
  decompression budget. `NsisInstaller::from_bytes()` is retained as a
  convenience that parses with default limits.
- `NsisInstaller::max_decompressed_size()` accessor and the
  `NsisInstaller::DEFAULT_MAX_DECOMPRESSED_SIZE` constant (64 MiB).
- `decompress::DecodeLimit`, an enum that makes the three real decode intents
  explicit: `Exact(n)` (known size — stop at `n`, ignore trailing input),
  `Capped(n)` (unknown size — decode to end-of-stream, error if it exceeds
  `n`), and `Truncate(n)` (unknown size — decode to end-of-stream, stop at `n`
  without error). Includes a `size()` accessor.
- `Error::OutputTooLarge { limit }`, returned when a `Capped` stream would
  expand past its budget.

### Changed

- **Breaking:** decompression budgets are now configurable instead of guessed.
  The previous `max(compressed_size * 10, 64 MiB)` heuristic — used for embedded
  files, the solid file-data stream, and uninstaller overlays — is replaced by a
  single budget threaded from `NsisInstaller::builder().max_decompressed_size()`.
- **Breaking:** `decompress::decompress_block` now takes a single `limit:
  DecodeLimit` argument in place of the previous `max_output: usize` and
  `expected_size: Option<usize>` parameters.
- **Breaking:** `decompress::{decompress_deflate, decompress_bzip2,
  decompress_lzma}` now take `limit: DecodeLimit` in place of their
  `max_output` / `expected_size` parameters.
- **Breaking:** `Error` gained the `OutputTooLarge` variant; exhaustive matches
  on `Error` must handle it.
- Over-budget extracted artifacts (files, uninstallers) now fail with
  `Error::OutputTooLarge` instead of being silently truncated, so callers no
  longer receive partial data reported as success.
- The LZMA decoder is now bounded *during* decompression via a size-limited
  writer rather than decoding fully and truncating afterward.

### Fixed

- Extract LZMA non-solid embedded files correctly. Per-file LZMA streams carry
  no stored uncompressed size and terminate with an end-of-stream marker;
  passing a fixed expected size made `lzma-rs` reject the marker
  (`"Expected unpacked size of N but decompressed to M"`), so every file in
  affected installers was dropped. Unknown-size streams now rely on the EOS
  marker.
- Apply the same end-of-stream fix to the uninstaller-overlay decode path,
  which carried the identical latent defect.

## [0.2.1] - 2026-05-20

### Fixed

- Handle packed NSIS installers whose `FirstHeader` is structurally valid but
  not 512-byte aligned relative to the PE overlay, as seen in Adload samples.
- Report out-of-bounds `EW_EXTRACTFILE` payloads as iterator errors instead
  of exposing malformed file records as empty extracted files.

## [0.2.0] - 2026-05-20

### Added

- `NsisInstaller::format_entry(&Entry) -> String` for rendering an opcode
  mnemonic plus decoded parameters as a script-like line.
- `NsisInstaller::format_entry_params(&Entry) -> String` for consumers that
  need reusable opcode-aware parameter formatting without reimplementing the
  string/variable/jump decoding rules.
- `NsisInstaller::script_analysis()` for script-level control-flow analysis,
  including roots, functions, basic blocks, edges, entry-to-block mapping, and
  diagnostics for invalid targets.
- Symbol-aware formatting via `format_entry_with_analysis()` and
  `format_entry_params_with_analysis()`, which annotate jump and call targets
  with names from `ScriptAnalysis`.

### Changed

- The `dump` example now uses the crate-provided instruction formatting API.
- The `dump` example uses script analysis symbols when rendering control-flow
  targets.
- Opcode metadata now matches NSIS operand layouts more closely for stack,
  UI, execution, registry, and file I/O instructions.

### Fixed

- `EW_PUSHPOP` formatting now treats `param0` as a variable for `Pop` and as a
  string for `Push`, preventing variable IDs from being decoded as offsets into
  unrelated strings such as `ProgramFilesDir`.
- Corrected high-level accessor layouts for `ShellExecOp` and `RegDelete`.
- Corrected several opcode parameter layouts and types, including
  `EW_GETFULLPATHNAME`, `EW_INTOP`, `EW_INTFMT`, `EW_FINDWINDOW`,
  `EW_SENDMESSAGE`, `EW_GETDLGITEM`, `EW_SHELLEXEC`, `EW_DELREG`, `EW_FOPEN`,
  `EW_FSEEK`, and `EW_FINDFIRST`.

## [0.1.2] - 2026-05-04

### Added

- `fmt::Display` implementations for the public diagnostic enums so
  consumers no longer need `format!("{:?}", ...)`:
  - `CompressionMethod` → `"deflate"`, `"bzip2"`, `"lzma"`, `"none"`.
  - `CompressionMode` → `"solid"`, `"non-solid"`.
  - `StringEncoding` → `"ANSI"`, `"Unicode"`, `"Park"`.
  - `NsisVersion` → `"NSIS 1"`, `"NSIS 2"`, `"NSIS 3"`, `"NSIS Park"`.
  - `RegValueType` → `"REG_SZ"`, `"REG_EXPAND_SZ"`, `"REG_BINARY"`,
    `"REG_DWORD"`, `"REG_MULTI_SZ"`, `"REG_UNKNOWN(N)"`.
- `Callback` enum (`src/installer/callback.rs`) covering the ten
  common-header callback slots (`OnInit`, `OnInstSuccess`,
  `OnInstFailed`, `OnUserAbort`, `OnGuiInit`, `OnGuiEnd`,
  `OnMouseOverSection`, `OnVerifyInstDir`, `OnSelChange`,
  `OnRebootFailed`). Provides:
  - `Callback::ALL` — all ten variants in common-header order.
  - `Callback::name()` — canonical NSIS script name (e.g. `".onInit"`).
  - `Callback::index()` — slot index (`0..10`) into the common-header
    callback array.
  - `fmt::Display` — delegates to `name()`.
- `NsisInstaller::callback(Callback) -> Option<usize>` — generic
  accessor that returns the entry index for any callback slot,
  complementing the existing per-callback `on_init()` / `on_inst_success()`
  / etc. methods.
- All standard `EW_*` opcode constants are re-exported at the crate root,
  so upstream analysis crates can match opcode numbers without importing
  the internal `opcode` module.
- `Section::contains_entry(usize) -> bool` — returns `true` when the
  given entry index falls within the section's `[code, code+code_size)`
  range. Treats negative `code`/`code_size` as zero, so consumers no
  longer need defensive `.max(0)` casts before doing range checks.
- `NsisInstaller::section_contains_entry(section_idx, entry_idx) -> bool`
  — convenience wrapper that resolves the section by index and applies
  `Section::contains_entry`.

### Changed

- The clippy panic-prevention lint set
  (`unwrap_used`, `expect_used`, `panic`, `arithmetic_side_effects`,
  `indexing_slicing`) is now declared `deny` in
  `Cargo.toml [lints.clippy]`, so the policy holds regardless of the
  consuming workspace. Previously only `missing_docs` and `unsafe_code`
  were denied (in `src/lib.rs`).
- Triage and clearance of the 310 lint violations exposed by the new
  policy. Affected files (paths relative to `src/`):
  `addressmap.rs`, `decompress/{bzip2,lzma,mod}.rs`,
  `header/{blockheader,commonheader,firstheader,mod}.rs`,
  `installer/{analysis,files,nsisinstaller}.rs`,
  `nsis/{ctlcolors,entry,langtable,page,section}.rs`,
  `opcode/mod.rs`,
  `strings/{ansi,mod,park,unicode}.rs`,
  `util.rs`.
  Patterns applied:
  - `&[u8]` indexing → `.get(...)` with `Option`/`Result` propagation.
  - Offset/size arithmetic → `checked_add` / `checked_sub` /
    `saturating_*`.
  - `.unwrap()` / `.expect()` on parse steps → `?`-propagated
    `Error` arms.
  Tests retain the convenience of `unwrap`/`expect`/`panic` via the
  `cfg_attr(test, allow(...))` escape hatch in `src/lib.rs`.

### Fixed

- Carried forward from in-progress work on `main`: park opcode mapping
  correction and uninstaller data extraction (see commit `1778513`).

## [0.1.1] - prior

- `fix: park opcode mapping`
- `fix: uninstaller data extraction`
- `feat: bump version to v0.1.1`
- `fix: docrs error on private item`
- `fix: docrs errors`

## [0.1.0] - initial release

- Initial release of the `nsis` crate.

[0.4.0]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ATRAPSLLC/nsis-rs/releases/tag/v0.1.0
