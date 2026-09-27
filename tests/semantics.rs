//! What each instruction reads and writes, checked against installers the
//! real compilers built.
//!
//! The opcode tables state, for every operand slot, what it holds and how the
//! runtime uses it (`ParamType`), and what else the instruction does
//! (`Effects`). Those statements come from the runtime's source; these tests
//! hold them to what the compilers actually emit. `semantics.nsi` compiles one
//! instance of each form whose meaning depends on its operands, and the other
//! fixtures supply the rest of the corpus.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use nsis::{
    Access, ControlFlowTarget, ExecFlag, HiddenVariables, NsisInstaller, ParamLayout, ParamType,
    StackEffect, Termination, nsis::Entry,
};

fn parse_fixture(name: &str) -> NsisInstaller<'static> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    NsisInstaller::from_bytes(Vec::leak(data)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Every fixture, by file name.
fn all_fixtures() -> Vec<String> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.ends_with(".exe").then_some(name)
        })
        .collect();
    names.sort();
    names
}

/// One decoded instruction: its mnemonic, raw operands and layout.
struct Decoded {
    mnemonic: &'static str,
    offsets: [i32; 6],
    layout: ParamLayout,
}

fn decoded(inst: &NsisInstaller<'_>) -> Vec<Decoded> {
    inst.entries()
        .map(|entry| {
            let entry: Entry<'_> = entry.unwrap();
            let info = inst
                .resolve_opcode(entry.which())
                .unwrap_or_else(|| panic!("opcode {} resolves to nothing", entry.which()));
            Decoded {
                mnemonic: info.mnemonic,
                offsets: entry.offsets(),
                layout: inst.param_layout(&entry).unwrap(),
            }
        })
        .collect()
}

fn mnemonics(name: &str) -> Vec<&'static str> {
    decoded(&parse_fixture(name))
        .iter()
        .map(|d| d.mnemonic)
        .collect()
}

/// **Each Park release is read with its own numbering.**
///
/// The fork inserts `GetFontVersion` (release 2) and `GetFontName` (release 3)
/// before `EW_REGISTERDLL`, the UTF-16 file commands before `EW_FSEEK`, the log
/// instruction after `EW_WRITEUNINSTALLER` in release 3 - compiled with
/// logging - and `FindProc` last. These scripts have no `WriteUninstaller`, the
/// entry the release used to be read from: before the stub was consulted both
/// read as release 1, from `GetFontVersion` on (`EW_REGISTERDLL`, then every
/// file command one off). Release 3 also read `LogSet` as `SectionSet`.
#[test]
fn every_park_release_numbers_its_own_opcodes() {
    assert_eq!(
        mnemonics("park2_opcodes.exe"),
        [
            "EW_CREATEDIR",
            "EW_EXTRACTFILE",
            "EW_GETFONTVERSION",
            "EW_FOPEN",
            "EW_FPUTWS",
            "EW_FSEEK",
            "EW_FCLOSE",
            "EW_SECTIONSET",
            "EW_LOCKWINDOW",
            "EW_FINDPROC",
            "EW_RET",
        ]
    );
    let park3 = parse_fixture("park3_opcodes.exe");
    assert!(park3.is_log_build(), "the third release logs");
    assert_eq!(
        mnemonics("park3_opcodes.exe"),
        [
            "EW_CREATEDIR",
            "EW_EXTRACTFILE",
            "EW_GETFONTVERSION",
            "EW_GETFONTNAME",
            "EW_FOPEN",
            "EW_FPUTWS",
            "EW_FSEEK",
            "EW_FCLOSE",
            "EW_LOG",
            "EW_LOG",
            "EW_SECTIONSET",
            "EW_LOCKWINDOW",
            "EW_FINDPROC",
            "EW_RET",
        ]
    );
    // Control: the first release, and a Park 3 read through its
    // `WriteUninstaller`, are unchanged.
    assert!(!parse_fixture("park1_unicode.exe").is_log_build());
    assert!(
        mnemonics("park3_unicode.exe").contains(&"EW_WRITEUNINSTALLER"),
        "park3_unicode keeps its uninstaller"
    );
}

/// **A logging build that calls a plugin is read as one.**
///
/// The plugin call's four-operand `EW_SETFLAG` read as malformed under a
/// three-operand table in both layouts, and the script's two shifted
/// instructions fit both - `LogSet on` as `SectionGetText`, `SectionGetText`
/// as `InstTypeGetText` - so the operand counts could not tell. The stub says
/// it logs.
#[test]
fn a_log_build_that_calls_a_plugin_is_detected() {
    let inst = parse_fixture("plugin_logbuild.exe");
    assert!(inst.is_log_build());
    let names = mnemonics("plugin_logbuild.exe");
    assert_eq!(names[2], "EW_LOG", "LogSet on");
    assert!(
        names.contains(&"EW_SECTIONSET"),
        "SectionGetText: {names:?}"
    );
    assert!(!names.contains(&"EW_INSTTYPESET"), "{names:?}");
    // Control: a stock build is not a log build.
    assert!(!parse_fixture("semantics.exe").is_log_build());
    assert!(!parse_fixture("opcodes_high.exe").is_log_build());
    assert!(parse_fixture("opcodes_logbuild.exe").is_log_build());
}

/// **Every slot holds what its role says, across the whole corpus.**
///
/// A variable slot is an index, a string slot reads back from the table, a
/// flag slot names one of the fourteen flags, a jump lands in the script. A
/// table that puts a role on the wrong slot - `EW_EXECUTE`'s exit code once
/// sat where its wait flag is - fails here on the first fixture that uses the
/// form. Each role is asserted to occur, so a table that stopped emitting one
/// cannot pass by having nothing to check.
#[test]
fn every_slot_holds_what_its_role_says() {
    let mut variables = 0usize;
    let mut strings = 0usize;
    let mut flags = 0usize;
    let mut jumps = 0usize;
    for name in all_fixtures() {
        let inst = parse_fixture(&name);
        let table = inst.string_table();
        for d in decoded(&inst) {
            for (index, _, kind, raw) in d.layout.slots(&d.offsets) {
                let at = format!("{name}: {} slot {index} = {raw}", d.mnemonic);
                match kind {
                    ParamType::Variable(_) => {
                        assert!(u16::try_from(raw).is_ok(), "{at}: not a variable");
                        variables += 1;
                    }
                    ParamType::String | ParamType::Number | ParamType::RawString => {
                        table
                            .read(raw)
                            .unwrap_or_else(|e| panic!("{at}: does not read back: {e}"));
                        strings += 1;
                    }
                    ParamType::Flag(_) => {
                        assert!(ExecFlag::from_index(raw).is_some(), "{at}: no such flag");
                        flags += 1;
                    }
                    ParamType::Jump if raw != 0 => {
                        let target = ControlFlowTarget::resolve(raw, inst.entry_count());
                        assert!(
                            !matches!(target, ControlFlowTarget::Invalid(_)),
                            "{at}: lands outside the script"
                        );
                        jumps += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        variables > 0 && strings > 0 && flags > 0 && jumps > 0,
        "variables {variables}, strings {strings}, flags {flags}, jumps {jumps}"
    );
}

/// The layout of the first instruction in `semantics.exe` that is `mnemonic`
/// and whose operands satisfy `pick`.
fn find(all: &[Decoded], mnemonic: &str, pick: impl Fn(&[i32; 6]) -> bool) -> &'static ParamLayout {
    let found = all
        .iter()
        .find(|d| d.mnemonic == mnemonic && pick(&d.offsets))
        .unwrap_or_else(|| panic!("no {mnemonic} of that form in semantics.exe"));
    Box::leak(Box::new(found.layout.clone()))
}

/// **`semantics.exe` reads the way `exec.c` runs it.**
///
/// One assertion per form, each against the runtime's own branch on the same
/// operands. Built by makensis 3.10 from `semantics.nsi`.
#[test]
fn the_semantics_fixture_reads_as_the_runtime_runs_it() {
    let all = decoded(&parse_fixture("semantics.exe"));
    let write = ParamType::Variable(Access::Write);
    let may_write = ParamType::Variable(Access::MayWrite);

    // ExecWait with a variable: the exit code is slot 1, written only when the
    // process started. Without one (-1), and for Exec, nothing is written.
    let exec_wait = find(&all, "EW_EXECUTE", |o| o[2] != 0 && o[1] >= 0);
    assert_eq!(exec_wait.types[1], may_write);
    assert_eq!(
        find(&all, "EW_EXECUTE", |o| o[1] < 0).types[1],
        ParamType::Unused
    );
    assert_eq!(
        find(&all, "EW_EXECUTE", |o| o[2] == 0).types[1],
        ParamType::Unused
    );

    // SetOutPath writes $OUTDIR; File and ExecShell read it.
    let set_out_path = find(&all, "EW_CREATEDIR", |o| o[1] != 0);
    assert_eq!(set_out_path.effects.outdir, Some(Access::Write));
    assert_eq!(
        find(&all, "EW_CREATEDIR", |o| o[1] == 0).effects.outdir,
        None
    );
    let file = find(&all, "EW_EXTRACTFILE", |_| true);
    assert_eq!(file.effects.outdir, Some(Access::Read));
    assert_eq!(file.types[5], ParamType::String, "the error text");
    assert_eq!(file.effects.terminates, Termination::May);
    assert_eq!(
        find(&all, "EW_SHELLEXEC", |_| true).effects.outdir,
        Some(Access::Read)
    );

    // ClearErrors writes the error flag; IfErrors reads and clears it;
    // IfRebootFlag only reads its flag.
    let clear = find(&all, "EW_SETFLAG", |o| o[0] == 2);
    assert_eq!(clear.types[0], ParamType::Flag(Access::Write));
    let if_errors = find(&all, "EW_IFFLAG", |o| o[2] == 2);
    assert_eq!(if_errors.types[2], ParamType::Flag(Access::Update));
    let if_reboot = find(&all, "EW_IFFLAG", |o| o[2] == 4);
    assert_eq!(if_reboot.types[2], ParamType::Flag(Access::Read));

    // A failing FileOpen sets the error flag the IfErrors above tests; its
    // handle is written either way.
    let file_open = find(&all, "EW_FOPEN", |_| true);
    assert_eq!(file_open.types[0], write);
    assert!(
        file_open
            .effects
            .flags_written
            .contains(ExecFlag::ExecError)
    );
    // FileRead and FileSeek read their handle and may write their output;
    // FileSeek without an output writes nothing.
    let file_read = find(&all, "EW_FGETS", |_| true);
    assert_eq!(
        (file_read.types[0], file_read.types[1]),
        (ParamType::Variable(Access::Read), may_write)
    );
    assert_eq!(find(&all, "EW_FSEEK", |o| o[1] >= 0).types[1], may_write);
    assert_eq!(
        find(&all, "EW_FSEEK", |o| o[1] < 0).types[1],
        ParamType::Unused
    );

    // IntOp divides, which can set the error flag; adding cannot.
    let divide = find(&all, "EW_INTOP", |o| o[3] == 3);
    assert!(divide.effects.flags_written.contains(ExecFlag::ExecError));
    let add = find(&all, "EW_INTOP", |o| o[3] == 0);
    assert!(add.effects.flags_written.is_empty());
    assert_eq!(add.types[1], ParamType::Number);

    // SendMessage with and without an output; a `STR:` parameter is a string.
    assert_eq!(find(&all, "EW_SENDMESSAGE", |o| o[0] >= 0).types[0], write);
    let string_param = find(&all, "EW_SENDMESSAGE", |o| o[0] < 0);
    assert_eq!(string_param.types[0], ParamType::Unused);
    assert_eq!(string_param.types[4], ParamType::String);

    // LoadAndSetImage: by path into a window handle with its handle returned,
    // and by resource id into a dialog item without.
    let by_path = find(&all, "EW_LOADANDSETIMAGE", |o| o[0] >= 0);
    assert_eq!(
        by_path.types[..4],
        [write, ParamType::String, ParamType::Number, ParamType::Int]
    );
    let by_id = find(&all, "EW_LOADANDSETIMAGE", |o| o[0] < 0);
    assert_eq!(
        by_id.types[..4],
        [
            ParamType::Unused,
            ParamType::Int,
            ParamType::Int,
            ParamType::Int
        ]
    );

    // The registry data slot is what the value kind says.
    let data = |kind: i32| find(&all, "EW_WRITEREG", move |o| o[4] == kind).types[3];
    assert_eq!(data(1), ParamType::String);
    assert_eq!(data(4), ParamType::Number);
    assert_eq!(data(3), ParamType::DataOffset);
    // SHCTX reads SetShellVarContext; HKCU does not.
    let shctx = find(&all, "EW_WRITEREG", |o| o[0] == 0);
    assert!(shctx.effects.flags_read.contains(ExecFlag::AllUserVar));
    let hkcu = find(&all, "EW_WRITEREG", |o| o[0] < 0);
    assert!(!hkcu.effects.flags_read.contains(ExecFlag::AllUserVar));
    // DeleteRegKey names no value.
    assert_eq!(
        find(&all, "EW_DELREG", |o| o[4] != 0).types[3],
        ParamType::Unused
    );
    assert_eq!(
        find(&all, "EW_DELREG", |o| o[4] == 0).types[3],
        ParamType::String
    );

    // DeleteINISec writes no key and no value.
    let delete_section = find(&all, "EW_WRITEINI", |o| o[4] == 0);
    assert_eq!(
        (delete_section.types[1], delete_section.types[2]),
        (ParamType::Unused, ParamType::Unused)
    );

    // DetailPrint reads SetDetailsPrint's flag.
    let detail_print = find(&all, "EW_UPDATETEXT", |_| true);
    assert!(
        detail_print
            .effects
            .flags_read
            .contains(ExecFlag::StatusUpdate)
    );

    // Rename /REBOOTOK may raise the reboot flag.
    let rename = find(&all, "EW_RENAME", |o| o[2] != 0);
    assert!(rename.effects.flags_written.contains(ExecFlag::ExecReboot));

    // The plugin call hands over everything; the SetFlag before it saves the
    // status mode, and the one after restores it from the runtime's copy.
    let plugin = find(&all, "EW_REGISTERDLL", |o| o[2] == 0);
    assert!(plugin.effects.plugin);
    assert_eq!(plugin.effects.hidden, HiddenVariables::Any);
    let restore = find(&all, "EW_SETFLAG", |o| o[2] > 0);
    assert_eq!(restore.types[1], ParamType::Unused);

    // Push, Exch and Pop, from the Helper function.
    assert_eq!(
        find(&all, "EW_PUSHPOP", |o| o[1] == 0 && o[2] == 0)
            .effects
            .stack,
        StackEffect::Push
    );
    let exch = find(&all, "EW_PUSHPOP", |o| o[2] != 0);
    assert_eq!(exch.effects.stack, StackEffect::Exch);
    let pop = find(&all, "EW_PUSHPOP", |o| o[1] != 0);
    assert_eq!(
        (pop.types[0], pop.effects.stack),
        (may_write, StackEffect::Pop)
    );

    // GetKnownFolderPath writes slot 1 from the folder id in slot 2.
    let known_folder = find(&all, "EW_GETOSINFO", |o| o[3] == 0);
    assert_eq!(
        (known_folder.types[1], known_folder.types[2]),
        (write, ParamType::String)
    );

    // CreateShortcut's description is slot 5; WriteUninstaller's full path
    // slot 3.
    assert_eq!(
        find(&all, "EW_CREATESHORTCUT", |_| true).types[5],
        ParamType::String
    );
    assert_eq!(
        find(&all, "EW_WRITEUNINSTALLER", |_| true).types[3],
        ParamType::String
    );
}
