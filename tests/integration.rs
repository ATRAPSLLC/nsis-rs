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

use nsis::{Error, NsisInstaller};

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
fn all_fixtures_produce_consistent_headers() {
    let fixtures = [
        "deflate_nonsolid.exe",
        "deflate_solid.exe",
        "lzma_nonsolid.exe",
        "lzma_solid.exe",
        "bzip2_nonsolid.exe",
        "bzip2_solid.exe",
        "full_featured.exe",
        "ansi_deflate.exe",
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
