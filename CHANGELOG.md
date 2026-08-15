# Changelog

All notable changes to the `nsis` crate are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
  uses the NSIS 3 special-code range, and `strings::read_string_at()`, the
  free function behind `NsisInstaller::read_string`.
- `SolidStatus`, reporting how an installer's solid file-data stream
  decompressed: `NotSolid`, `Complete`, `Truncated { limit }`, or
  `Failed(Error)`. Read it with `NsisInstaller::solid_status()`.
- `decompress::Decoded`, the `{ data, truncated }` pair now returned by the
  decompression entry points. `truncated` distinguishes a buffer that ended
  naturally from one stopped by a `DecodeLimit::Truncate` budget.

### Changed

- **Breaking:** `strings::read_nsis_string`, `strings::read_string_at` and
  `strings::ansi::read_ansi_string` take the ANSI code range to decode with.
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

[0.3.1]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/ATRAPSLLC/nsis-rs/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ATRAPSLLC/nsis-rs/releases/tag/v0.1.0
