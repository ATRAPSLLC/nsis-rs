//! NSIS installer dump and extraction tool.
//!
//! Usage:
//!   `cargo run --example dump -- <installer.exe>`             — print info
//!   `cargo run --example dump -- <installer.exe> --extract <outdir>` — extract files

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use std::{collections::HashSet, env, fs, path::Path, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: dump <installer.exe> [--extract <outdir>]");
        process::exit(1);
    }
    let path = &args[1];
    let extract_dir = if args.len() >= 4 && args[2] == "--extract" {
        Some(args[3].as_str())
    } else {
        None
    };

    let data = fs::read(path).unwrap_or_else(|e| {
        eprintln!("error reading {path}: {e}");
        process::exit(1);
    });

    let installer = nsis::NsisInstaller::from_bytes(&data).unwrap_or_else(|e| {
        eprintln!("error parsing NSIS installer: {e}");
        process::exit(1);
    });

    println!("NSIS Installer: {path}");
    println!("  Version:     {:?}", installer.version());
    println!(
        "  Compression: {:?} ({:?})",
        installer.compression(),
        installer.compression_mode()
    );
    println!("  Encoding:    {:?}", installer.string_encoding());
    println!("  Uninstaller: {}", installer.is_uninstaller());
    println!("  Legacy:      {}", installer.is_legacy());
    println!("  Sections:    {}", installer.section_count());
    println!("  Entries:     {}", installer.entry_count());
    println!("  Pages:       {}", installer.page_count());
    println!();

    println!("Sections:");
    let mut group_depth: usize = 0;
    for (i, section) in installer.sections().enumerate() {
        match section {
            Ok(s) => {
                // Close group before printing the end marker.
                if s.is_section_group_end() {
                    group_depth = group_depth.saturating_sub(1);
                }

                let name = s
                    .inline_name()
                    .unwrap_or_else(|| resolve_str(&installer, s.name_ptr()));
                let indent = "  ".repeat(group_depth);
                let mut flags_desc = Vec::new();
                if s.is_selected() {
                    flags_desc.push("selected");
                }
                if s.is_read_only() {
                    flags_desc.push("ro");
                }
                if s.is_bold() {
                    flags_desc.push("bold");
                }
                let flags_str = if flags_desc.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", flags_desc.join(", "))
                };

                if s.is_section_group() {
                    println!("  {indent}[{i:3}] {name}{flags_str}");
                    group_depth += 1;
                } else if s.is_section_group_end() {
                    // Don't print the end marker — the dedent is enough.
                } else if name.is_empty() && s.code_size() == 0 {
                    // Skip truly empty unnamed sections.
                } else {
                    let size = if s.size_kb() > 0 {
                        format!("  {}KB", s.size_kb())
                    } else {
                        String::new()
                    };
                    println!("  {indent}[{i:3}] {name}{flags_str}{size}");
                }
            }
            Err(e) => println!("  [{i:2}] <error: {e}>"),
        }
    }
    println!();

    println!("Pages:");
    for (i, page) in installer.pages().enumerate() {
        match page {
            Ok(p) => {
                let caption = resolve_str(&installer, p.caption());
                let ptype = format!("{:?}", p.page_type());
                print!("  [{i:2}] {ptype:<12}");
                if !caption.is_empty() {
                    print!(" {caption:?}");
                }

                let mut details = Vec::new();
                let flags = p.flags();
                if flags & nsis::nsis::page::PF_BACK_SHOW != 0 {
                    details.push("back");
                }
                if flags & nsis::nsis::page::PF_NEXT_ENABLE != 0 {
                    details.push("next");
                }
                if flags & nsis::nsis::page::PF_CANCEL_ENABLE != 0 {
                    details.push("cancel");
                }
                if flags & nsis::nsis::page::PF_LICENSE_FORCE_SELECTION != 0 {
                    details.push("license_must_accept");
                }
                if flags & nsis::nsis::page::PF_PAGE_EX != 0 {
                    details.push("PageEx");
                }
                if !details.is_empty() {
                    print!("  [{}]", details.join(", "));
                }

                // Show callbacks if defined
                let mut cbs = Vec::new();
                if p.prefunc() >= 0 {
                    cbs.push(format!("pre=>{}", p.prefunc()));
                }
                if p.showfunc() >= 0 {
                    cbs.push(format!("show=>{}", p.showfunc()));
                }
                if p.leavefunc() >= 0 {
                    cbs.push(format!("leave=>{}", p.leavefunc()));
                }
                if !cbs.is_empty() {
                    print!("  ({})", cbs.join(", "));
                }

                println!();
            }
            Err(e) => println!("  [{i:2}] <error: {e}>"),
        }
    }
    println!();

    println!("Files:");
    {
        let mut seen = HashSet::new();
        for file in installer.files() {
            match file {
                Ok(f) => {
                    if !seen.insert(f.data_block_offset()) {
                        continue;
                    }
                    let name = f
                        .name()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|_| "<error>".into());
                    let compressed = if f.is_compressed() {
                        "compressed"
                    } else {
                        "raw"
                    };
                    let data_len = f.data().len();
                    println!(
                        "  {name:40} offset=0x{:08X}  {data_len:>8} bytes ({compressed})",
                        f.data_block_offset()
                    );
                }
                Err(e) => println!("  <error: {e}>"),
            }
        }
    }
    println!();

    println!("Callbacks:");
    let callbacks: &[(&str, Option<usize>)] = &[
        (".onInit", installer.on_init()),
        (".onInstSuccess", installer.on_inst_success()),
        (".onInstFailed", installer.on_inst_failed()),
        (".onUserAbort", installer.on_user_abort()),
        (".onGUIInit", installer.on_gui_init()),
        (".onGUIEnd", installer.on_gui_end()),
        (".onMouseOverSection", installer.on_mouse_over_section()),
        (".onVerifyInstDir", installer.on_verify_inst_dir()),
        (".onSelChange", installer.on_sel_change()),
        (".onRebootFailed", installer.on_reboot_failed()),
    ];
    for (name, idx) in callbacks {
        if let Some(entry_idx) = idx {
            println!("  {name:25} entry {entry_idx}");
        }
    }
    println!();

    println!("Plugin Calls:");
    for c in installer.plugin_calls().flatten() {
        let dll = c.dll().map(|n| n.to_string()).unwrap_or_default();
        let func = c.function().map(|n| n.to_string()).unwrap_or_default();
        let kind = if c.is_plugin_call() { "call" } else { "reg" };
        println!("  [{kind}] {dll}::{func}");
    }
    println!();

    println!("Exec Commands:");
    for c in installer.exec_commands().flatten() {
        match c {
            nsis::ExecCommand::Exec(op) => {
                let wait = if op.is_wait() { "ExecWait" } else { "Exec" };
                let cmdline = op.command_line().map(|n| n.to_string()).unwrap_or_default();
                println!("  {wait}: {cmdline}");
            }
            nsis::ExecCommand::ShellExec(op) => {
                let verb = op.verb().map(|n| n.to_string()).unwrap_or_default();
                let file = op.file().map(|n| n.to_string()).unwrap_or_default();
                println!("  ShellExec: {verb} {file}");
            }
        }
    }
    println!();

    println!("Registry Operations:");
    for o in installer.registry_ops().flatten() {
        match o {
            nsis::RegistryOp::Write(w) => {
                let key = w.key().map(|n| n.to_string()).unwrap_or_default();
                let vname = w.value_name().map(|n| n.to_string()).unwrap_or_default();
                let data = w.data().map(|n| n.to_string()).unwrap_or_default();
                println!(
                    "  WRITE {}\\{} \"{}\" = {:?} ({:?})",
                    w.root_name(),
                    key,
                    vname,
                    data,
                    w.reg_type()
                );
            }
            nsis::RegistryOp::Delete(d) => {
                let key = d.key().map(|n| n.to_string()).unwrap_or_default();
                let vname = d.value_name().map(|n| n.to_string()).unwrap_or_default();
                println!("  DELETE {}\\{} \"{}\"", d.root_name(), key, vname);
            }
            nsis::RegistryOp::Read(r) => {
                let key = r.key().map(|n| n.to_string()).unwrap_or_default();
                let vname = r.value_name().map(|n| n.to_string()).unwrap_or_default();
                println!("  READ {}\\{} \"{}\"", r.root_name(), key, vname);
            }
        }
    }
    println!();

    println!("Shortcuts:");
    for s in installer.shortcuts().flatten() {
        let link = s.link_path().map(|n| n.to_string()).unwrap_or_default();
        let target = s.target().map(|n| n.to_string()).unwrap_or_default();
        println!("  {link} -> {target}");
    }
    println!();

    println!("Uninstallers:");
    for u in installer.uninstallers().flatten() {
        let path = u.path().map(|n| n.to_string()).unwrap_or_default();
        println!(
            "  {path} (offset={}, icon_size={})",
            u.data_offset(),
            u.icon_size()
        );
    }
    println!();

    println!("Script:");
    let script_analysis = installer.script_analysis().ok();
    for (i, entry) in installer.entries().enumerate() {
        match entry {
            Ok(e) => {
                let info = installer.resolve_opcode(e.which());
                let mnemonic = info.map(|o| o.mnemonic).unwrap_or("???");
                let detail = script_analysis
                    .as_ref()
                    .map(|analysis| installer.format_entry_params_with_analysis(&e, analysis))
                    .unwrap_or_else(|| installer.format_entry_params(&e));
                println!("  {i:5}: {mnemonic:<25} {detail}");
            }
            Err(e) => println!("  {i:5}: <error: {e}>"),
        }
    }

    if let Some(dir) = extract_dir {
        println!();
        extract_files(&installer, dir);
    }
}

fn extract_files(installer: &nsis::NsisInstaller<'_>, outdir: &str) {
    let base = Path::new(outdir);
    fs::create_dir_all(base).unwrap_or_else(|e| {
        eprintln!("error creating output directory: {e}");
        process::exit(1);
    });

    let mut extracted = 0;
    let mut errors = 0;
    let mut seen_offsets = HashSet::new();

    for file in installer.files() {
        let file = match file {
            Ok(f) => f,
            Err(e) => {
                eprintln!("  error iterating files: {e}");
                errors += 1;
                continue;
            }
        };

        // Deduplicate: skip files at already-seen data offsets.
        if !seen_offsets.insert(file.data_block_offset()) {
            continue;
        }

        let path = match file.name() {
            Ok(n) => n.to_path(),
            Err(e) => {
                eprintln!("  error reading filename: {e}");
                errors += 1;
                continue;
            }
        };

        if path.is_empty() {
            continue;
        }

        let dest = base.join(&path);

        if let Some(parent) = dest.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            eprintln!("  error creating directory for {path}: {e}");
            errors += 1;
            continue;
        }

        match file.decompress() {
            Ok(content) => {
                if let Err(e) = fs::write(&dest, &content) {
                    eprintln!("  error writing {path}: {e}");
                    errors += 1;
                } else {
                    println!("  {path} ({} bytes)", content.len());
                    extracted += 1;
                }
            }
            Err(_) => {
                // Decompression failed (e.g., solid mode) — write raw data if available.
                let raw = file.data();
                if !raw.is_empty() {
                    if let Err(e) = fs::write(&dest, raw) {
                        eprintln!("  error writing {path}: {e}");
                        errors += 1;
                    } else {
                        println!("  {path} ({} bytes, raw)", raw.len());
                        extracted += 1;
                    }
                }
            }
        }
    }

    println!();
    println!("Extraction: {extracted} files, {errors} errors");
}

/// Resolve a string table offset, returning an empty string on error.
fn resolve_str(installer: &nsis::NsisInstaller<'_>, offset: i32) -> String {
    if offset <= 0 {
        return String::new();
    }
    installer
        .read_string(offset)
        .map(|s| s.to_string())
        .unwrap_or_default()
}
