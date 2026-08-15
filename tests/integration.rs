//! Integration tests for the NSIS parser using self-built test fixtures.
//!
//! All test fixtures are built from `.nsi` scripts in `tests/build_fixtures/`
//! using `makensis` and cover specific compression/encoding/feature combinations.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use nsis::{
    Error, NsisInstaller, SolidStatus, header::firstheader::FirstHeader, opcode::NsisVersion,
    strings::StringEncoding,
};

fn fixture_bytes(name: &str) -> &'static [u8] {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    Vec::leak(data)
}

fn parse_fixture(name: &str) -> NsisInstaller<'static> {
    NsisInstaller::from_bytes(fixture_bytes(name))
        .unwrap_or_else(|e| panic!("failed to parse {name}: {e}"))
}

fn validate_all_structures(inst: &NsisInstaller<'_>) {
    for (i, section) in inst.sections().enumerate() {
        section.unwrap_or_else(|e| panic!("section {i} failed: {e}"));
    }
    for (i, entry) in inst.entries().enumerate() {
        entry.unwrap_or_else(|e| panic!("entry {i} failed: {e}"));
    }
    for (i, page) in inst.pages().enumerate() {
        page.unwrap_or_else(|e| panic!("page {i} failed: {e}"));
    }
}

#[test]
fn deflate_nonsolid() {
    let inst = parse_fixture("deflate_nonsolid.exe");
    assert_eq!(
        inst.compression(),
        nsis::decompress::CompressionMethod::Deflate
    );
    assert_eq!(
        inst.compression_mode(),
        nsis::decompress::CompressionMode::NonSolid
    );
    assert!(inst.section_count() > 0);
    assert!(inst.entry_count() > 0);
    validate_all_structures(&inst);
}

#[test]
fn deflate_solid() {
    let inst = parse_fixture("deflate_solid.exe");
    assert_eq!(
        inst.compression(),
        nsis::decompress::CompressionMethod::Deflate
    );
    assert_eq!(
        inst.compression_mode(),
        nsis::decompress::CompressionMode::Solid
    );
    assert!(inst.section_count() > 0);
    validate_all_structures(&inst);
}

#[test]
fn lzma_nonsolid() {
    let inst = parse_fixture("lzma_nonsolid.exe");
    assert_eq!(
        inst.compression(),
        nsis::decompress::CompressionMethod::Lzma
    );
    assert_eq!(
        inst.compression_mode(),
        nsis::decompress::CompressionMode::NonSolid
    );
    assert!(inst.section_count() > 0);
    validate_all_structures(&inst);
}

#[test]
fn lzma_solid() {
    let inst = parse_fixture("lzma_solid.exe");
    assert_eq!(
        inst.compression(),
        nsis::decompress::CompressionMethod::Lzma
    );
    assert_eq!(
        inst.compression_mode(),
        nsis::decompress::CompressionMode::Solid
    );
    assert!(inst.section_count() > 0);
    validate_all_structures(&inst);
}

#[test]
fn full_featured_sections() {
    let inst = parse_fixture("full_featured.exe");
    assert_eq!(
        inst.compression(),
        nsis::decompress::CompressionMethod::Lzma
    );
    assert_eq!(
        inst.compression_mode(),
        nsis::decompress::CompressionMode::Solid
    );
    assert_eq!(inst.section_count(), 2);
    let sections: Vec<_> = inst.sections().collect();
    let s0 = sections[0].as_ref().unwrap();
    let s1 = sections[1].as_ref().unwrap();
    let name0 = s0
        .inline_name()
        .or_else(|| inst.read_string(s0.name_ptr()).ok().map(|n| n.to_string()))
        .unwrap_or_default();
    let name1 = s1
        .inline_name()
        .or_else(|| inst.read_string(s1.name_ptr()).ok().map(|n| n.to_string()))
        .unwrap_or_default();
    assert_eq!(name0, "Core Files");
    assert_eq!(name1, "Optional Docs");
}

#[test]
fn full_featured_callbacks() {
    let inst = parse_fixture("full_featured.exe");
    assert!(inst.on_init().is_some(), "should have .onInit");
}

#[test]
fn full_featured_registry() {
    let inst = parse_fixture("full_featured.exe");
    let writes: Vec<_> = inst
        .registry_ops()
        .filter_map(|op| match op.ok()? {
            nsis::RegistryOp::Write(w) => Some(w),
            _ => None,
        })
        .collect();
    assert!(writes.len() >= 3, "should have registry writes");
    let has_version = writes.iter().any(|w| {
        w.value_name()
            .map(|n| n.to_string() == "Version")
            .unwrap_or(false)
    });
    assert!(has_version, "should write Version registry value");
}

#[test]
fn full_featured_shortcuts() {
    let inst = parse_fixture("full_featured.exe");
    let shortcuts: Vec<_> = inst.shortcuts().collect();
    assert_eq!(shortcuts.len(), 2, "should have 2 shortcuts");
}

#[test]
fn full_featured_uninstaller() {
    let inst = parse_fixture("full_featured.exe");
    let uninstallers: Vec<_> = inst.uninstallers().collect();
    assert_eq!(uninstallers.len(), 1, "should have 1 uninstaller");
    let u = uninstallers[0].as_ref().unwrap();
    let path = u.path().unwrap().to_string();
    assert!(
        path.contains("uninstall"),
        "path should contain 'uninstall', got '{path}'"
    );
}

#[test]
fn uninstaller_registry_delete_uses_correct_param_layout() {
    let inst = parse_fixture("full_featured.exe");
    let uninstaller = inst.uninstallers().next().unwrap().unwrap();
    let data = uninstaller.decompress().unwrap();
    let uninst = NsisInstaller::from_bytes(&data).unwrap();

    let deletes: Vec<_> = uninst
        .registry_ops()
        .filter_map(|op| match op.ok()? {
            nsis::RegistryOp::Delete(delete) => Some((
                delete.root_name(),
                delete.key().ok()?.to_string(),
                delete.value_name().ok()?.to_string(),
            )),
            _ => None,
        })
        .collect();

    assert!(
        deletes.iter().any(|(root, key, value)| {
            *root == "HKLM" && key == "Software\\FullFeaturedTest" && value.is_empty()
        }),
        "DeleteRegKey should read root from param1 and key from param2"
    );
}

#[test]
fn file_extraction_nonsolid() {
    let inst = parse_fixture("deflate_nonsolid.exe");
    let mut count = 0;
    for file in inst.files() {
        let file = file.unwrap();
        assert!(!file.data().is_empty(), "non-solid file should have data");
        let content = file.decompress().unwrap();
        assert!(
            !content.is_empty(),
            "decompressed content should not be empty"
        );
        count += 1;
    }
    assert!(count > 0, "should find files");
}

#[test]
fn decompression_budget_rejects_oversized_file() {
    // With a tiny budget, every embedded file that decompresses to more than
    // the budget must surface `OutputTooLarge` rather than truncated `Ok`.
    // `deflate_nonsolid` has a genuinely compressed entry (`config.ini`).
    let inst = NsisInstaller::builder(fixture_bytes("deflate_nonsolid.exe"))
        .max_decompressed_size(8)
        .parse()
        .expect("header parsing is independent of the file budget");

    // Compressed entries that expand past the budget must error; uncompressed
    // (stored) entries are copied verbatim and cannot be a bomb, so they're
    // exempt from the budget.
    let mut saw_over_budget = false;
    for file in inst.files() {
        let file = file.unwrap();
        let compressed = file.is_compressed();
        match file.decompress() {
            Ok(_) => {}
            Err(Error::OutputTooLarge { limit }) => {
                assert!(compressed, "only compressed streams are budget-capped");
                assert_eq!(limit, 8);
                saw_over_budget = true;
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
    assert!(
        saw_over_budget,
        "expected at least one compressed file to exceed the 8-byte budget"
    );
}

#[test]
fn generous_budget_extracts_all_files() {
    // The same fixture parses and extracts cleanly with a generous budget,
    // confirming the budget is the only thing the tiny cap changed.
    let inst = NsisInstaller::builder(fixture_bytes("deflate_nonsolid.exe"))
        .max_decompressed_size(256 * 1024 * 1024)
        .parse()
        .unwrap();
    let mut count = 0;
    for file in inst.files() {
        let content = file.unwrap().decompress().unwrap();
        assert!(!content.is_empty());
        count += 1;
    }
    assert!(count > 0);
}

#[test]
fn file_extraction_reports_out_of_bounds_payload() {
    let path = format!(
        "{}/tests/fixtures/deflate_nonsolid.exe",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut data = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let prefix_offset = {
        let inst = NsisInstaller::from_bytes(&data).unwrap();
        let file = inst.files().next().unwrap().unwrap();
        inst.data_block_offset() + file.data_block_offset() as usize
    };
    data[prefix_offset..prefix_offset + 4].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());

    let inst = NsisInstaller::from_bytes(&data).unwrap();
    let first_file = inst.files().next().unwrap();
    assert!(
        first_file.is_err(),
        "out-of-bounds payload should not produce an empty file"
    );
}

#[test]
fn file_extraction_solid() {
    let inst = parse_fixture("lzma_solid.exe");
    let mut count = 0;
    for file in inst.files() {
        let file = file.unwrap();
        assert!(
            !file.data().is_empty(),
            "solid file should have data from cache"
        );
        let content = file.decompress().unwrap();
        assert!(
            !content.is_empty(),
            "decompressed content should not be empty"
        );
        count += 1;
    }
    assert!(count > 0, "should find files");
}

#[test]
fn section_entries_mapping() {
    let inst = parse_fixture("full_featured.exe");
    for section in inst.sections() {
        let section = section.unwrap();
        if section.code_size() > 0 {
            let entries: Vec<_> = inst.section_entries(&section).collect();
            assert_eq!(entries.len(), section.code_size() as usize);
            for entry in &entries {
                entry.as_ref().unwrap();
            }
            return;
        }
    }
    panic!("no section with code found");
}

#[test]
fn opcode_resolution() {
    let inst = parse_fixture("full_featured.exe");
    let mut resolved = 0;
    for entry in inst.entries() {
        let entry = entry.unwrap();
        if inst.resolve_opcode(entry.which()).is_some() {
            resolved += 1;
        }
    }
    assert!(resolved > 0, "no opcodes resolved");
}

#[test]
fn script_formatting_uses_opcode_aware_param_types() {
    let inst = parse_fixture("full_featured.exe");
    let lines: Vec<_> = inst
        .entries()
        .map(|entry| inst.format_entry(&entry.unwrap()))
        .collect();

    assert!(
        lines.iter().any(|line| {
            line.starts_with("EW_GETDLGITEM ")
                && line.contains("dialog=\"$HWNDPARENT\"")
                && line.contains("item_id=\"1037\"")
        }),
        "GetDlgItem should render dialog and item id as strings"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("EW_SETCTLCOLORS ") && line.contains("hwnd=\"$_0_\"")),
        "SetCtlColors hwnd should render as a string parameter"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.contains("$_65503_") && !line.contains("==>")),
        "formatting should not wrap negative output vars or duplicate jump separators"
    );
}

#[test]
fn script_analysis_builds_roots_blocks_and_edges() {
    let inst = parse_fixture("full_featured.exe");
    let analysis = inst.script_analysis().unwrap();

    assert_eq!(analysis.entry_count, inst.entry_count());
    assert!(!analysis.roots.is_empty(), "should discover script roots");
    assert!(
        analysis
            .roots
            .iter()
            .any(|root| matches!(root.kind, nsis::ScriptRootKind::Callback { .. })),
        "should include callback roots"
    );
    assert!(
        analysis
            .roots
            .iter()
            .any(|root| matches!(root.kind, nsis::ScriptRootKind::Section { index: 0 })),
        "should include section roots"
    );
    assert!(!analysis.blocks.is_empty(), "should build basic blocks");
    assert!(
        analysis
            .edges
            .iter()
            .any(|edge| matches!(edge.kind, nsis::EdgeKind::Return)),
        "should include return edges"
    );
    assert!(
        analysis
            .edges
            .iter()
            .any(|edge| matches!(edge.kind, nsis::EdgeKind::Branch { .. })),
        "should include branch edges"
    );
    assert_eq!(
        analysis.entry_to_block.len(),
        analysis.entry_count,
        "entry-to-block map should cover all entries"
    );
    let block = analysis
        .block_for_entry(49)
        .expect("entry 49 should be in a block");
    assert!(
        analysis
            .outgoing_edges(block.id)
            .any(|edge| matches!(edge.kind, nsis::EdgeKind::Branch { .. })),
        "entry 49's block should have a branch edge"
    );
    assert!(
        analysis
            .function_for_entry(81)
            .map(|function| function.name.as_str())
            == Some("section_0"),
        "section body should be assigned to section_0"
    );
    assert!(
        analysis
            .roots_for_entry(79)
            .any(|root| matches!(root.kind, nsis::ScriptRootKind::Callback { .. })),
        "entry 79 should have a callback root"
    );
}

#[test]
fn symbolic_formatting_uses_script_analysis_symbols() {
    let inst = parse_fixture("full_featured.exe");
    let analysis = inst.script_analysis().unwrap();

    let mut saw_symbolic_target = false;
    for (index, entry) in inst.entries().enumerate() {
        let entry = entry.unwrap();
        let line = inst.format_entry_with_analysis(&entry, &analysis);
        if line.contains("=>") && line.contains("(@") {
            saw_symbolic_target = true;
        }
        if index == 49 {
            assert!(
                line.contains("=>") && !line.contains("==>"),
                "symbolic formatting should preserve jump syntax"
            );
        }
    }

    assert!(
        saw_symbolic_target,
        "should render at least one symbolic target"
    );
}

#[test]
fn opcode_constants_are_exported_at_crate_root() {
    let exported = [
        nsis::EW_INVALID_OPCODE,
        nsis::EW_RET,
        nsis::EW_CALL,
        nsis::EW_FGETWS,
    ];
    assert_eq!(exported, [0, 1, 5, 70]);
}

#[test]
fn string_resolution() {
    let inst = parse_fixture("full_featured.exe");
    for section in inst.sections() {
        let section = section.unwrap();
        let _ = inst.read_string(section.name_ptr());
    }
}

#[test]
fn bzip2_nonsolid() {
    let inst = parse_fixture("bzip2_nonsolid.exe");
    assert_eq!(
        inst.compression(),
        nsis::decompress::CompressionMethod::Bzip2
    );
    assert_eq!(
        inst.compression_mode(),
        nsis::decompress::CompressionMode::NonSolid
    );
    assert!(inst.section_count() > 0);
    assert!(inst.entry_count() > 0);
    validate_all_structures(&inst);
}

#[test]
fn bzip2_solid() {
    let inst = parse_fixture("bzip2_solid.exe");
    assert_eq!(
        inst.compression(),
        nsis::decompress::CompressionMethod::Bzip2
    );
    assert_eq!(
        inst.compression_mode(),
        nsis::decompress::CompressionMode::Solid
    );
    assert!(inst.section_count() > 0);
    validate_all_structures(&inst);
}

#[test]
fn bzip2_file_extraction_nonsolid() {
    let inst = parse_fixture("bzip2_nonsolid.exe");
    let mut count = 0;
    for file in inst.files() {
        let file = file.unwrap();
        assert!(
            !file.data().is_empty(),
            "bzip2 non-solid file should have data"
        );
        let content = file.decompress().unwrap();
        assert!(!content.is_empty());
        count += 1;
    }
    assert!(count > 0, "should find files");
}

#[test]
fn bzip2_file_extraction_solid() {
    let inst = parse_fixture("bzip2_solid.exe");
    let mut count = 0;
    for file in inst.files() {
        let file = file.unwrap();
        assert!(!file.data().is_empty(), "bzip2 solid file should have data");
        let content = file.decompress().unwrap();
        assert!(!content.is_empty());
        count += 1;
    }
    assert!(count > 0, "should find files");
}

#[test]
fn bzip2_solid_data_is_not_padded_to_the_budget() {
    // Regression: the NSIS-bzip2 output loop never terminated at the end of a
    // block, so it emitted `0x0A` filler until the decompression budget was
    // reached. This 38 KB installer yielded 67,104,886 bytes of solid data —
    // the whole 64 MiB default budget — instead of 89.
    let inst = parse_fixture("bzip2_solid.exe");
    let solid = inst.solid_data();

    assert!(
        solid.len() < 1024,
        "solid data should be the real payload, got {} bytes (budget is {})",
        solid.len(),
        inst.max_decompressed_size()
    );
    assert!(
        !solid.ends_with(&[0x0A; 16]),
        "solid data should not end in repeated filler"
    );

    // The other solid fixtures hold the same two payloads; sizes should agree
    // to within the few bytes the fixtures' build runs differ by.
    let lzma = parse_fixture("lzma_solid.exe");
    let delta = solid.len().abs_diff(lzma.solid_data().len());
    assert!(
        delta <= 8,
        "bzip2 solid data ({} bytes) should match lzma solid data ({} bytes)",
        solid.len(),
        lzma.solid_data().len()
    );
}

#[test]
fn solid_status_reports_a_complete_decode() {
    for name in ["lzma_solid.exe", "bzip2_solid.exe", "deflate_solid.exe"] {
        let inst = parse_fixture(name);
        assert_eq!(
            inst.solid_status(),
            &SolidStatus::Complete,
            "{name} decodes fully within the default budget"
        );
    }
}

#[test]
fn non_solid_installers_report_not_solid() {
    for name in ["lzma_nonsolid.exe", "deflate_nonsolid.exe"] {
        let inst = parse_fixture(name);
        assert_eq!(inst.solid_status(), &SolidStatus::NotSolid, "{name}");
    }
}

/// Byte offset just past the first file's payload in the solid stream.
fn first_file_end(inst: &NsisInstaller<'_>) -> usize {
    let file = inst.files().next().unwrap().unwrap();
    // 4-byte stream length prefix + header block + the file's own framing.
    4 + inst.header_data().len() + file.data_block_offset() as usize + 4 + file.data().len()
}

#[test]
fn budget_truncation_reports_the_budget_not_a_bounds_error() {
    // Regression for the reported behaviour: an over-budget solid installer
    // parsed successfully but every file failed with
    //   "file data payload: expected at least 75497476 bytes, got 67104998"
    // which reads like data corruption rather than an exhausted budget.
    let full = parse_fixture("lzma_solid.exe");
    assert!(full.files().count() >= 2, "fixture needs two files");

    // A budget that admits the first file's data but not the second's.
    let budget = first_file_end(&full);
    let data = fixture_bytes("lzma_solid.exe");
    let inst = NsisInstaller::builder(data)
        .max_decompressed_size(budget)
        .parse()
        .expect("header parsing must still succeed over budget");

    assert_eq!(
        inst.solid_status(),
        &SolidStatus::Truncated { limit: budget },
        "the parser should record that the stream was cut at the budget"
    );

    let results: Vec<_> = inst.files().collect();

    // The file stored before the cut is unaffected.
    let first = results[0]
        .as_ref()
        .expect("first file is within the budget");
    assert!(!first.decompress().unwrap().is_empty());

    // Everything after it names the budget as the reason.
    for (i, result) in results.iter().enumerate().skip(1) {
        match result {
            Err(Error::OutputTooLarge { limit }) => assert_eq!(*limit, budget),
            Err(e) => panic!("file {i}: expected OutputTooLarge, got {e}"),
            Ok(f) => panic!("file {i}: expected an error, got {:?}", f.name().unwrap()),
        }
    }
}

#[test]
fn truncated_solid_decompress_reports_the_budget() {
    // The same substitution applies to `decompress()`, not just iteration.
    let data = fixture_bytes("lzma_solid.exe");
    let inst = NsisInstaller::builder(data)
        .max_decompressed_size(16)
        .parse()
        .expect("header parsing must still succeed");

    assert!(matches!(
        inst.solid_status(),
        SolidStatus::Truncated { limit: 16 }
    ));
    for result in inst.files() {
        assert!(matches!(result, Err(Error::OutputTooLarge { limit: 16 })));
    }
}

/// Copies a fixture and corrupts the tail of its solid stream, leaving the
/// header region — which decodes with an exact bound and stops early — intact.
fn fixture_with_corrupt_solid_tail(name: &str) -> Vec<u8> {
    let mut data = fixture_bytes(name).to_vec();
    let inst = parse_fixture(name);
    let fh_offset = inst.first_header_file_offset();
    let fh = FirstHeader::parse(&data[fh_offset..]).unwrap();

    let stream_start = fh_offset + FirstHeader::SIZE;
    let following = fh.length_of_all_following_data() as usize - FirstHeader::SIZE;
    let crc = if fh.has_no_crc() { 0 } else { 4 };
    let stream_end = stream_start + following - crc;

    // Scribble over the last stretch of compressed data.
    for byte in &mut data[stream_end - 16..stream_end] {
        *byte ^= 0xFF;
    }
    data
}

#[test]
fn failed_solid_decode_is_reported_not_swallowed() {
    // Regression: a failed solid decode became an empty `solid_data` via
    // `unwrap_or_else(|_| Vec::new())`, so parsing "succeeded" and every file
    // then reported a bounds error with no trace of the real cause.
    // LZMA, not bzip2: a bzip2 block must be decoded whole, so corrupting its
    // tail also breaks the header decode. An LZMA stream decodes forward, and
    // the header's exact-size decode stops long before the damage.
    let data = fixture_with_corrupt_solid_tail("lzma_solid.exe");
    let inst = NsisInstaller::from_bytes(&data)
        .expect("header parsing is unaffected by a corrupt stream tail");

    let SolidStatus::Failed(cause) = inst.solid_status() else {
        panic!("expected Failed, got {:?}", inst.solid_status());
    };
    assert!(
        matches!(cause, Error::DecompressionFailed { method: "lzma", .. }),
        "the decode error should be preserved verbatim, got {cause}"
    );

    // Every file reports that cause rather than a bounds error.
    let cause = cause.clone();
    let results: Vec<_> = inst.files().collect();
    assert!(!results.is_empty(), "the entry stream is still readable");
    for result in results {
        match result {
            Err(e) => assert_eq!(e, cause),
            Ok(f) => panic!("expected an error, got {:?}", f.name().unwrap()),
        }
    }
}

#[test]
fn stubs_rejected_by_strict_pe_parsing_still_parse() {
    // Overlay detection needs only the section table, so the PE is parsed with
    // every optional structure switched off and in permissive mode. These three
    // stubs are rejected outright by a strict parse:
    //
    //   nsis203_ansi  - resource directory offset points past the appended data
    //   park2_unicode - base relocations at an RVA that cannot be mapped
    //   park3_unicode - likewise
    //
    // 7-Zip mis-detects the two Park stubs as plain PE files for the same
    // reason and cannot list them without an explicit `-tnsis`.
    for name in ["nsis203_ansi.exe", "park2_unicode.exe", "park3_unicode.exe"] {
        let data = fixture_bytes(name);

        let strict = goblin::pe::PE::parse(data);
        assert!(
            strict.is_err(),
            "{name}: expected a strict parse to fail; if the stub is now clean, \
             this test no longer covers the lenient path"
        );

        let inst = NsisInstaller::from_bytes(data)
            .unwrap_or_else(|e| panic!("{name}: should parse despite the malformed PE: {e}"));
        assert!(inst.entry_count() > 0, "{name}: no entries");
        for file in inst.files() {
            let file = file.unwrap_or_else(|e| panic!("{name}: file entry failed: {e}"));
            assert!(!file.decompress().unwrap().is_empty(), "{name}: empty file");
        }
    }
}

#[test]
fn multi_block_bzip2_solid_stream_is_byte_exact() {
    // Regression: a block whose final BWT byte was consumed as an RLE repeat
    // count used to emit that count byte as data, inserting one spurious byte
    // and shifting every file stored after it. It only shows on streams of more
    // than one block, so no fixture caught it until a 72 MiB payload (~84
    // blocks) was added.
    //
    // `oversize_bzip2_solid` and `oversize_lzma_solid` are built from the same
    // script, so their decompressed solid streams must be identical.
    let budget = 256 * 1024 * 1024;
    let parse = |name: &str| {
        NsisInstaller::builder(fixture_bytes(name))
            .max_decompressed_size(budget)
            .parse()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let bzip2 = parse("oversize_bzip2_solid.exe");
    let lzma = parse("oversize_lzma_solid.exe");

    let (a, b) = (bzip2.solid_data(), lzma.solid_data());
    assert_eq!(
        a.len(),
        b.len(),
        "bzip2 and lzma builds of the same script must decode to the same length"
    );
    if let Some(i) = (0..a.len()).find(|&i| a[i] != b[i]) {
        panic!(
            "bzip2 output diverges from lzma at byte {i}: {:#04X} vs {:#04X}",
            a[i], b[i]
        );
    }

    // The file stored after the 72 MiB payload is the one a shifted stream
    // loses first.
    let names: Vec<String> = bzip2
        .files()
        .map(|f| f.unwrap().name().unwrap().to_string())
        .collect();
    assert_eq!(names, ["payload.txt", "big.bin", "config.ini"]);

    let big = bzip2
        .files()
        .nth(1)
        .unwrap()
        .unwrap()
        .decompress()
        .expect("the 72 MiB payload should decompress");
    assert_eq!(big.len(), 75_497_472);
    assert!(big.iter().all(|&b| b == 0), "payload should be all zeros");
}

#[test]
fn ansi_installers_are_not_assumed_to_be_nsis2() {
    // makensis 3.x compiles an ANSI target whenever a script omits
    // `Unicode true`, so the encoding alone does not give the version away.
    // These two are 3.10 builds with an ANSI string table.
    for name in ["ansi3_deflate_nonsolid.exe", "ansi3_latin1.exe"] {
        let inst = parse_fixture(name);
        assert_eq!(inst.string_encoding(), StringEncoding::Ansi, "{name}");
        assert_eq!(
            inst.version(),
            NsisVersion::V3,
            "{name}: built by makensis 3.10"
        );
        assert_eq!(
            inst.nsis2_sub_version(),
            None,
            "{name}: not an NSIS 2 installer"
        );
    }

    // Genuine NSIS 2 builds keep reporting NSIS 2.
    for name in [
        "nsis203_ansi.exe",
        "nsis225_ansi.exe",
        "nsis246_ansi_solid.exe",
        "nsis246_ansi_latin1.exe",
        "dirs_nsis246_ansi_solid.exe",
    ] {
        let inst = parse_fixture(name);
        assert_eq!(inst.string_encoding(), StringEncoding::Ansi, "{name}");
        assert_eq!(inst.version(), NsisVersion::V2, "{name}");
    }
}

#[test]
fn latin1_names_do_not_look_like_nsis2_variable_codes() {
    // `ansi3_latin1` stores `grüße.txt` and `þýÿ.ini`, whose bytes fall in the
    // NSIS 2 special-code range 0xFC-0xFF. Version detection keys off the
    // NSIS 3 code range instead, so those literals cannot drag it back to
    // NSIS 2 — the trap that a byte-frequency heuristic would fall into.
    let inst = parse_fixture("ansi3_latin1.exe");
    assert_eq!(inst.version(), NsisVersion::V3);

    // Its NSIS 2.46 counterpart carries the same text and must stay NSIS 2.
    let inst = parse_fixture("nsis246_ansi_latin1.exe");
    assert_eq!(inst.version(), NsisVersion::V2);
}

#[test]
fn all_fixtures_produce_consistent_headers() {
    let fixtures = [
        "deflate_nonsolid.exe",
        "deflate_solid.exe",
        "lzma_nonsolid.exe",
        "lzma_solid.exe",
        "bzip2_nonsolid.exe",
        "bzip2_solid.exe",
        "full_featured.exe",
        "deflate_single_file.exe",
    ];
    for name in fixtures {
        let inst = parse_fixture(name);
        // All fixtures should have valid header data.
        assert!(
            inst.header_data().len() >= 68,
            "{name}: header too short ({})",
            inst.header_data().len()
        );
        // All should have at least one section.
        assert!(inst.section_count() > 0, "{name}: no sections");
        // All should have entries.
        assert!(inst.entry_count() > 0, "{name}: no entries");
        // All structures should parse without errors.
        validate_all_structures(&inst);
    }
}

#[test]
fn all_fixtures_extract_files() {
    let fixtures = [
        "deflate_nonsolid.exe",
        "deflate_solid.exe",
        "lzma_nonsolid.exe",
        "lzma_solid.exe",
        "bzip2_nonsolid.exe",
        "bzip2_solid.exe",
        "full_featured.exe",
    ];
    for name in fixtures {
        let inst = parse_fixture(name);
        let mut file_count = 0;
        for file in inst.files() {
            let file = file.unwrap();
            let content = file.decompress().unwrap();
            assert!(!content.is_empty(), "{name}: decompressed file is empty");
            file_count += 1;
        }
        assert!(file_count > 0, "{name}: no files extracted");
    }
}

#[test]
fn extracted_file_content_is_valid() {
    // Our fixtures contain payload.txt with known content.
    let inst = parse_fixture("deflate_nonsolid.exe");
    for file in inst.files() {
        let file = file.unwrap();
        let name = file.name().unwrap().to_string();
        if name.contains("payload.txt") {
            let content = file.decompress().unwrap();
            let text = String::from_utf8_lossy(&content);
            assert!(
                text.contains("test payload"),
                "payload.txt should contain 'test payload', got: {text}"
            );
            return;
        }
    }
    panic!("payload.txt not found in fixture");
}

#[test]
fn solid_and_nonsolid_produce_same_content() {
    // Compare extracted payload.txt between solid and non-solid deflate.
    let nonsolid = parse_fixture("deflate_nonsolid.exe");
    let solid = parse_fixture("deflate_solid.exe");

    let get_payload = |inst: &nsis::NsisInstaller<'_>| -> Vec<u8> {
        for file in inst.files() {
            let file = file.unwrap();
            let name = file.name().unwrap().to_string();
            if name.contains("payload.txt") {
                return file.decompress().unwrap();
            }
        }
        panic!("payload.txt not found");
    };

    let ns_content = get_payload(&nonsolid);
    let s_content = get_payload(&solid);
    assert_eq!(
        ns_content, s_content,
        "solid and non-solid should produce identical payload content"
    );
}

#[test]
fn all_compression_methods_produce_same_content() {
    // Compare payload.txt across all 6 compression variants.
    let fixtures = [
        "deflate_nonsolid.exe",
        "deflate_solid.exe",
        "lzma_nonsolid.exe",
        "lzma_solid.exe",
        "bzip2_nonsolid.exe",
        "bzip2_solid.exe",
    ];

    let mut reference: Option<Vec<u8>> = None;
    for name in fixtures {
        let inst = parse_fixture(name);
        for file in inst.files() {
            let file = file.unwrap();
            let fname = file.name().unwrap().to_string();
            if fname.contains("payload.txt") {
                let content = file.decompress().unwrap();
                if let Some(ref expected) = reference {
                    assert_eq!(
                        &content, expected,
                        "{name}: payload.txt differs from deflate_nonsolid"
                    );
                } else {
                    reference = Some(content);
                }
                break;
            }
        }
    }
    assert!(reference.is_some(), "no payload.txt found in any fixture");
}
