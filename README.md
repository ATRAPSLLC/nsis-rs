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
- Version-aware opcode lookup across NSIS 1.x, 2.x, 3.x, and the Park Unicode fork
- High-level analysis iterators for security-relevant operations:
  plugin calls, process execution, registry modifications, shortcut creation, uninstaller stubs
- Extract and decompress embedded files
- Zero-copy view types — the only heap allocations are for decompressed data and decoded strings
- `#![deny(unsafe_code)]`

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
