# nsis

A pure Rust parser for [NSIS (NullSoft Scriptable Install System)](https://nsis.sourceforge.io/)
installer binaries. Provides typed access to all internal structures — from
PE overlay detection through decompressed headers to individual bytecode
instructions and embedded files.

Built for **malware analysis** and **reverse engineering**.

## Features

- Parse PE overlay to locate NSIS data appended after PE sections
- Decompress header blocks (deflate, bzip2, LZMA) in solid and non-solid modes
- Iterate sections, pages, bytecode entries, language tables, and embedded files
- Decode NSIS string tables (ANSI, Unicode, Jim Park fork encoding) with variable and shell folder resolution
- Version-aware opcode lookup across every NSIS generation — see below
- High-level analysis iterators for security-relevant operations:
  plugin calls, process execution, registry modifications, shortcut creation, uninstaller stubs
- Extract and decompress embedded files
- Zero-copy view types — the only heap allocations are for decompressed data and decoded strings
- `#![deny(unsafe_code)]`

## Supported versions

NSIS changed its container format, its instruction numbering and its string
encoding several times. This crate reads all of them through one interface,
working out which applies from the file rather than being told:

| Generation | Notes |
|---|---|
| **NSIS 1.x** | A different container: no block table, 20-byte sections, 24-byte instructions, its own opcode numbering and one-byte variable references. Its bzip2 keeps a per-block flag NSIS 2.0 dropped, its first-header flags mean different things, and its uninstallers use a shorter header again. |
| **NSIS 2.x** | Including the variable-layout changes in 2.04 and 2.26, which move the built-in variables and so change what a reference decodes to. |
| **NSIS 3.x** | ANSI and Unicode targets. NSIS 3 moved the ANSI special codes from the top of the byte range to the bottom. |
| **Jim Park Unicode fork** | 2.46.1, 2.46.2 and 2.46.3, each inserting instructions at a different point. |
| **Logging builds** | A makensis compiled with logging carries an extra instruction, shifting everything above `WriteUninstaller`. Detected by which layout the entry stream is consistent with. |

Compression is deflate, bzip2 (both NSIS block layouts) or LZMA, solid or
non-solid, in any combination the compiler could produce.

Every one of these is covered by a fixture built with the compiler in question
and checked against 7-Zip's own listing of the same file, in
`tests/fixture_registry.rs`. The exception is NSIS 1.x, which 7-Zip cannot open
at all — those fixtures are checked against their build logs instead.

Builds of makensis with a non-default compile-time configuration are a known
limit: nearly every field in the header and nearly every instruction sits
behind an `#ifdef`, so a custom build numbers them differently. Files that do
not read consistently are rejected rather than decoded into plausible nonsense.

## Quick start

```rust
use nsis::NsisInstaller;

let data = std::fs::read("installer.exe").unwrap();
let installer = NsisInstaller::from_bytes(&data).unwrap();

println!("Version:     {:?}", installer.version());
println!("Compression: {:?} ({:?})", installer.compression(), installer.compression_mode());
println!("Encoding:    {:?}", installer.string_encoding());
println!("Sections:    {}", installer.section_count());
println!("Entries:     {}", installer.entry_count());
```

## Analysis iterators

The high-level API surfaces operations that are commonly relevant during
malware triage:

```rust
// Plugin DLL calls (System.dll, nsDialogs.dll, etc.)
for call in installer.plugin_calls() {
    let call = call.unwrap();
    println!("Plugin: {} -> {}", call.dll().unwrap(), call.function().unwrap());
}

// Process execution (Exec, ExecWait, ShellExec)
for cmd in installer.exec_commands() {
    println!("{:?}", cmd.unwrap());
}

// Registry operations (read, write, delete)
for op in installer.registry_ops() {
    println!("{:?}", op.unwrap());
}

// Shortcut creation and embedded uninstallers
for shortcut in installer.shortcuts() { /* ... */ }
for uninst in installer.uninstallers() { /* ... */ }
```

## File extraction

```rust
for file in installer.files() {
    let file = file.unwrap();
    let name = file.name().unwrap();
    println!("{}: {} bytes (compressed)", name, file.data().len());

    // Decompress the file data
    let content = file.decompress().unwrap();
    std::fs::write(format!("out/{name}"), &content).unwrap();
}
```

## Dump example

The included `dump` example prints a full analysis of an installer and
optionally extracts embedded files:

```bash
cargo run --example dump -- installer.exe
cargo run --example dump -- installer.exe --extract out/
```

## Minimum Rust version

1.85 (edition 2024)

## License

Copyright 2026 ATRAPS LLC. Licensed under the Apache License,
Version 2.0. See `LICENSE` and `NOTICE`.
