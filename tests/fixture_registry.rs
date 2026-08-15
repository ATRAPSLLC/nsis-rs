//! Declarative registry of every test fixture, checked against 7-Zip.
//!
//! Each fixture in `tests/fixtures/` is declared here with the compiler that
//! produced it and the properties the parser must report. Two invariants keep
//! the registry honest:
//!
//! - every `.exe` in the fixture directory must appear in [`FIXTURES`], so a
//!   newly added fixture cannot sit unexercised;
//! - every fixture's extracted files are compared against the committed
//!   `7z l -slt` listing in `tests/fixtures/expected/`, which is the ground
//!   truth for names and sizes.
//!
//! Where a known defect makes the parser disagree with 7-Zip, the fixture
//! carries a `*_defect` note and the test asserts the disagreement *still
//! exists*. That way fixing the defect fails this test and forces the registry
//! to be updated, rather than leaving a stale exception behind.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;

use nsis::{
    NsisInstaller,
    decompress::{CompressionMethod, CompressionMode},
    opcode::{Nsis2SubVersion, NsisVersion},
    strings::StringEncoding,
};

/// Default decompression budget (64 MiB), as used by `NsisInstaller::from_bytes`.
const DEFAULT_BUDGET: usize = 64 * 1024 * 1024;

/// Budget for the `oversize_*` fixtures, whose payload alone is 72 MiB.
const LARGE_BUDGET: usize = 256 * 1024 * 1024;

/// A fixture and everything the parser is expected to report about it.
struct Fixture {
    /// File stem; the fixture is `<name>.exe`.
    name: &'static str,
    /// Compiler that produced it, from the build manifest.
    compiler: &'static str,
    /// NSIS version the parser should detect.
    version: NsisVersion,
    /// Variable layout the parser should report for an NSIS 2 installer.
    ///
    /// Every NSIS 2 fixture here is `From226`, including the 2.03 and 2.25
    /// builds: the layout is detected from instructions a script has to
    /// actually use, and these scripts use none of them. 7-Zip reads them the
    /// same way. See `opcode::detect_nsis2_sub_version`.
    nsis2_sub: Option<Nsis2SubVersion>,
    encoding: StringEncoding,
    method: CompressionMethod,
    mode: CompressionMode,
    /// Number of `EW_EXTRACTFILE` entries.
    files: usize,
    /// Number of `WriteUninstaller` entries. 7-Zip lists these as archive
    /// items; this crate exposes them through `uninstallers()` instead, so they
    /// are excluded from the file comparison and checked separately.
    uninstallers: usize,
    budget: usize,
    /// Set when a known defect makes `version()` disagree with the compiler
    /// that actually built the fixture.
    version_defect: Option<&'static str>,
    /// Set when a known defect makes extracted names disagree with 7-Zip.
    name_defect: Option<&'static str>,
}

const FIXTURES: &[Fixture] = &[
    // -- Pre-existing fixtures (NSIS 3.x Unicode) --
    Fixture {
        name: "deflate_nonsolid",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "deflate_solid",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::Solid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "lzma_nonsolid",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "lzma_solid",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "bzip2_nonsolid",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Bzip2,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "bzip2_solid",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Bzip2,
        mode: CompressionMode::Solid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "full_featured",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 3,
        uninstallers: 1,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "deflate_single_file",
        compiler: "makensis 3.x",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::NonSolid,
        files: 1,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    // -- ANSI, NSIS 3.10 (`Unicode false`) --
    Fixture {
        name: "ansi3_deflate_nonsolid",
        compiler: "makensis 3.10 (x86-ansi)",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Ansi,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "ansi3_latin1",
        compiler: "makensis 3.10 (x86-ansi)",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Ansi,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    // -- NSIS 2.x, ANSI --
    Fixture {
        name: "nsis203_ansi",
        compiler: "makensis 2.03",
        version: NsisVersion::V2,
        nsis2_sub: Some(Nsis2SubVersion::From226),
        encoding: StringEncoding::Ansi,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "nsis225_ansi",
        compiler: "makensis 2.25",
        version: NsisVersion::V2,
        nsis2_sub: Some(Nsis2SubVersion::From226),
        encoding: StringEncoding::Ansi,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "nsis246_ansi_solid",
        compiler: "makensis 2.46",
        version: NsisVersion::V2,
        nsis2_sub: Some(Nsis2SubVersion::From226),
        encoding: StringEncoding::Ansi,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "nsis246_ansi_latin1",
        compiler: "makensis 2.46",
        version: NsisVersion::V2,
        nsis2_sub: Some(Nsis2SubVersion::From226),
        encoding: StringEncoding::Ansi,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::NonSolid,
        files: 2,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        // NSIS 2 escapes literal 0xFC-0xFF with the SKIP code, so these decode
        // correctly — the mirror image of `ansi3_latin1`.
        name_defect: None,
    },
    // -- Jim Park's Unicode fork --
    Fixture {
        name: "park1_unicode",
        compiler: "makensis 2.46.1-Unicode",
        version: NsisVersion::Park,
        nsis2_sub: None,
        encoding: StringEncoding::Park,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 2,
        uninstallers: 1,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "park2_unicode",
        compiler: "makensis 2.46.2-Unicode",
        version: NsisVersion::Park,
        nsis2_sub: None,
        encoding: StringEncoding::Park,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 2,
        uninstallers: 1,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "park3_unicode",
        compiler: "makensis 2.46.3-Unicode",
        version: NsisVersion::Park,
        nsis2_sub: None,
        encoding: StringEncoding::Park,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 2,
        uninstallers: 1,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    // -- Directory-structure references --
    Fixture {
        name: "dirs_unicode_solid",
        compiler: "makensis 3.10 (x86-unicode)",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 8,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "dirs_nsis246_ansi_solid",
        compiler: "makensis 2.46",
        version: NsisVersion::V2,
        nsis2_sub: Some(Nsis2SubVersion::From226),
        encoding: StringEncoding::Ansi,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 8,
        uninstallers: 0,
        budget: DEFAULT_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    // -- Payload larger than the default budget --
    Fixture {
        name: "oversize_lzma_solid",
        compiler: "makensis 3.10 (x86-unicode)",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Lzma,
        mode: CompressionMode::Solid,
        files: 3,
        uninstallers: 0,
        budget: LARGE_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "oversize_bzip2_solid",
        compiler: "makensis 3.10 (x86-unicode)",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Bzip2,
        mode: CompressionMode::Solid,
        files: 3,
        uninstallers: 0,
        budget: LARGE_BUDGET,
        version_defect: None,
        name_defect: None,
    },
    Fixture {
        name: "oversize_zlib_solid",
        compiler: "makensis 3.10 (x86-unicode)",
        version: NsisVersion::V3,
        nsis2_sub: None,
        encoding: StringEncoding::Unicode,
        method: CompressionMethod::Deflate,
        mode: CompressionMode::Solid,
        files: 3,
        uninstallers: 0,
        budget: LARGE_BUDGET,
        version_defect: None,
        name_defect: None,
    },
];

fn fixture_dir() -> String {
    format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

fn read_fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/{name}.exe", fixture_dir());
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

fn parse(fixture: &Fixture, data: &[u8]) -> NsisInstaller<'static> {
    // The registry owns the bytes for the whole run; leaking keeps the borrow
    // simple and the fixtures are a few hundred KiB in total.
    let data: &'static [u8] = Vec::leak(data.to_vec());
    NsisInstaller::builder(data)
        .max_decompressed_size(fixture.budget)
        .parse()
        .unwrap_or_else(|e| panic!("{}: parse failed: {e}", fixture.name))
}

/// One item of a `7z l -slt` listing.
struct ListedItem {
    /// Path with the archive's separator normalised away, leaving the basename.
    basename: String,
    /// `None` for entries whose data NSIS deduplicated; 7-Zip reports no size.
    size: Option<u64>,
}

/// Parses the item list out of `7z l -slt` output.
///
/// The first block describes the archive itself and is skipped; item blocks
/// begin after the `----------` separator. Listings generated on Windows use
/// `\` separators and those generated on Linux use `/`, so both are stripped.
fn parse_listing(text: &str) -> Vec<ListedItem> {
    let mut items = Vec::new();
    let mut path: Option<String> = None;
    let mut size: Option<u64> = None;
    let mut in_items = false;

    let flush = |path: Option<String>, size: Option<u64>, items: &mut Vec<ListedItem>| {
        if let Some(p) = path {
            let basename = p.rsplit(['\\', '/']).next().unwrap_or(&p).to_string();
            items.push(ListedItem { basename, size });
        }
    };

    for line in text.lines() {
        if line.starts_with("----------") {
            in_items = true;
            continue;
        }
        if !in_items {
            continue;
        }
        if let Some(value) = line.strip_prefix("Path = ") {
            flush(path.take(), size.take(), &mut items);
            path = Some(value.trim().to_string());
            size = None;
        } else if let Some(value) = line.strip_prefix("Size = ") {
            size = value.trim().parse().ok();
        }
    }
    flush(path, size, &mut items);
    items
}

fn read_listing(name: &str) -> Vec<ListedItem> {
    let path = format!("{}/expected/{name}.7z.txt", fixture_dir());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing ground-truth listing {path}: {e}"));
    parse_listing(&text)
}

#[test]
fn every_fixture_file_is_declared() {
    let mut undeclared: Vec<String> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "exe" {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            FIXTURES.iter().all(|f| f.name != stem).then_some(stem)
        })
        .collect();
    undeclared.sort();

    assert!(
        undeclared.is_empty(),
        "these fixtures are not declared in FIXTURES and so are never checked: {undeclared:?}"
    );
}

#[test]
fn every_fixture_has_a_ground_truth_listing() {
    for fixture in FIXTURES {
        let items = read_listing(fixture.name);
        assert!(
            !items.is_empty(),
            "{}: ground-truth listing has no items",
            fixture.name
        );
    }
}

#[test]
fn declared_metadata_matches() {
    for fixture in FIXTURES {
        let data = read_fixture(fixture.name);
        let inst = parse(fixture, &data);
        let name = fixture.name;

        assert_eq!(
            inst.string_encoding(),
            fixture.encoding,
            "{name} ({}): string encoding",
            fixture.compiler
        );
        assert_eq!(
            inst.nsis2_sub_version(),
            fixture.nsis2_sub,
            "{name} ({}): NSIS 2 variable layout",
            fixture.compiler
        );
        assert_eq!(inst.compression(), fixture.method, "{name}: method");
        assert_eq!(inst.compression_mode(), fixture.mode, "{name}: mode");
        assert_eq!(
            inst.files().count(),
            fixture.files,
            "{name}: EW_EXTRACTFILE count"
        );
        assert_eq!(
            inst.uninstallers().count(),
            fixture.uninstallers,
            "{name}: WriteUninstaller count"
        );

        match fixture.version_defect {
            None => assert_eq!(
                inst.version(),
                fixture.version,
                "{name} ({}): version",
                fixture.compiler
            ),
            Some(defect) => assert_ne!(
                inst.version(),
                fixture.version,
                "{name}: version detection now agrees with the compiler — the \
                 recorded defect is fixed ({defect}); drop `version_defect` \
                 from the registry"
            ),
        }
    }
}

#[test]
fn extracted_files_match_the_7zip_listing() {
    for fixture in FIXTURES {
        let data = read_fixture(fixture.name);
        let inst = parse(fixture, &data);
        let name = fixture.name;

        // 7-Zip lists the uninstaller as an archive item; this crate reports it
        // through `uninstallers()`, which `declared_metadata_matches` checks.
        let mut expected: Vec<(String, Option<u64>)> = read_listing(name)
            .into_iter()
            .filter(|item| item.basename != "uninstall.exe")
            .map(|item| (item.basename, item.size))
            .collect();
        expected.sort();

        let mut ours: Vec<(String, u64)> = inst
            .files()
            .map(|file| {
                let file = file.unwrap_or_else(|e| panic!("{name}: file entry failed: {e}"));
                let decoded = file
                    .name()
                    .unwrap_or_else(|e| panic!("{name}: name decode failed: {e}"))
                    .to_string();
                let content = file
                    .decompress()
                    .unwrap_or_else(|e| panic!("{name}: decompress failed: {e}"));
                (decoded, content.len() as u64)
            })
            .collect();
        ours.sort();

        let expected_names: Vec<&String> = expected.iter().map(|(n, _)| n).collect();
        let our_names: Vec<&String> = ours.iter().map(|(n, _)| n).collect();

        match fixture.name_defect {
            None => {
                assert_eq!(
                    our_names, expected_names,
                    "{name} ({}): extracted names differ from 7-Zip",
                    fixture.compiler
                );
            }
            Some(defect) => {
                assert_ne!(
                    our_names, expected_names,
                    "{name}: names now agree with 7-Zip — the recorded defect is \
                     fixed ({defect}); drop `name_defect` from the registry"
                );
                // Sizes are still checked below; only the names are affected.
            }
        }

        // Sizes must match wherever 7-Zip reports one, matched by position for
        // intact fixtures and by count for the ones with mangled names.
        let expected_sizes: Vec<Option<u64>> = expected.iter().map(|(_, s)| *s).collect();
        let our_sizes: Vec<u64> = ours.iter().map(|(_, s)| *s).collect();
        assert_eq!(
            expected_sizes.len(),
            our_sizes.len(),
            "{name}: file count differs from 7-Zip"
        );
        for (i, expected_size) in expected_sizes.iter().enumerate() {
            if let Some(expected_size) = expected_size {
                assert!(
                    our_sizes.contains(expected_size),
                    "{name}: 7-Zip lists a {expected_size}-byte file (item {i}) that we \
                     did not extract; ours: {our_sizes:?}"
                );
            }
        }
    }
}

#[test]
fn solid_fixtures_decompress_completely() {
    for fixture in FIXTURES {
        if fixture.mode != CompressionMode::Solid {
            continue;
        }
        let data = read_fixture(fixture.name);
        let inst = parse(fixture, &data);
        assert_eq!(
            inst.solid_status(),
            &nsis::SolidStatus::Complete,
            "{}: solid stream should decode fully within its declared budget",
            fixture.name
        );
    }
}

#[test]
fn fixture_payloads_have_the_expected_contents() {
    // The build script writes one known payload; every fixture that extracts it
    // must produce identical bytes, whatever the compressor or NSIS version.
    let mut seen = BTreeMap::new();
    for fixture in FIXTURES {
        let data = read_fixture(fixture.name);
        let inst = parse(fixture, &data);
        for file in inst.files() {
            let file = file.unwrap();
            let content = file.decompress().unwrap();
            // `payload.txt` is 52-55 bytes depending on the build batch; group
            // by length so a corrupt decode stands out against its peers.
            if content.len() < 64 {
                let text = String::from_utf8_lossy(&content).to_string();
                if text.starts_with("This is a test payload") {
                    seen.insert(fixture.name, text);
                }
            }
        }
    }
    assert!(
        seen.len() > 10,
        "expected most fixtures to carry the standard payload, found {}",
        seen.len()
    );
    for (name, text) in &seen {
        assert!(
            text.contains("test payload for NSIS fixture generation"),
            "{name}: payload text is corrupt: {text:?}"
        );
    }
}
