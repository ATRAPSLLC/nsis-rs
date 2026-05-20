//! Malware analysis iterators for NSIS installers.
//!
//! This module provides high-level, security-focused iterators that surface
//! the operations most relevant for malware analysis:
//!
//! - [`PluginCall`]: DLL plugin invocations (`System::Call`, etc.)
//! - [`ExecCommand`]: Process execution (`Exec`, `ExecWait`, `ShellExecute`)
//! - [`RegistryOp`]: Registry read/write/delete operations
//! - [`Shortcut`]: Desktop/Start Menu shortcut creation
//! - [`Uninstaller`]: Embedded uninstaller stubs
//!
//! Each type wraps an [`Entry`] with typed accessors that decode the raw
//! opcode parameters into meaningful fields. The iterators scan the full
//! entry block and yield only entries matching the relevant opcodes.
//!
//! # Example
//!
//! ```no_run
//! use nsis::NsisInstaller;
//!
//! let data = std::fs::read("installer.exe").unwrap();
//! let inst = NsisInstaller::from_bytes(&data).unwrap();
//!
//! // Check for suspicious plugin calls
//! for call in inst.plugin_calls() {
//!     let call = call.unwrap();
//!     let dll = call.dll().unwrap();
//!     let func = call.function().unwrap();
//!     println!("Plugin: {}::{}", dll, func);
//! }
//!
//! // Enumerate registry persistence
//! for op in inst.registry_ops() {
//!     if let Ok(nsis::RegistryOp::Write(w)) = op {
//!         println!("{}\\{}", w.root_name(), w.key().unwrap());
//!     }
//! }
//! ```

use core::fmt;

use crate::{
    decompress::{self, CompressionMode},
    error::Error,
    installer::NsisInstaller,
    nsis::entry::{Entry, EntryIter},
    opcode,
    strings::NsisString,
};

/// Resolves an HKEY root constant to its conventional name.
///
/// NSIS stores registry roots as the upper 32-bit `HKEY_*` handle values:
///
/// | Value | Name |
/// |-------|------|
/// | `0x80000000` | `HKCR` (HKEY_CLASSES_ROOT) |
/// | `0x80000001` | `HKCU` (HKEY_CURRENT_USER) |
/// | `0x80000002` | `HKLM` (HKEY_LOCAL_MACHINE) |
/// | `0x80000003` | `HKU` (HKEY_USERS) |
/// | `0x80000005` | `HKCC` (HKEY_CURRENT_CONFIG) |
pub fn hkey_name(root: i32) -> &'static str {
    match root as u32 {
        0x8000_0000 => "HKCR",
        0x8000_0001 => "HKCU",
        0x8000_0002 => "HKLM",
        0x8000_0003 => "HKU",
        0x8000_0005 => "HKCC",
        _ => "UNKNOWN_HKEY",
    }
}

/// Registry value type for [`RegWrite`] operations.
///
/// Corresponds to the Windows `REG_*` constants. The type is determined
/// from the `typelen` parameter (param 4) of the `EW_WRITEREG` instruction,
/// with additional disambiguation from param 5 for `ExpandStr` and `MultiStr`.
///
/// Source: 7-Zip `NsisIn.cpp` lines 4560-4618.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegValueType {
    /// `REG_SZ` — null-terminated string.
    Str,
    /// `REG_EXPAND_SZ` — string with `%ENVIRONMENT_VARIABLE%` expansion.
    ExpandStr,
    /// `REG_BINARY` — arbitrary binary data.
    Bin,
    /// `REG_DWORD` — 32-bit unsigned integer.
    Dword,
    /// `REG_MULTI_SZ` — sequence of null-terminated strings.
    MultiStr,
    /// Unknown or unrecognized registry value type.
    Unknown(i32),
}

impl fmt::Display for RegValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegValueType::Str => f.write_str("REG_SZ"),
            RegValueType::ExpandStr => f.write_str("REG_EXPAND_SZ"),
            RegValueType::Bin => f.write_str("REG_BINARY"),
            RegValueType::Dword => f.write_str("REG_DWORD"),
            RegValueType::MultiStr => f.write_str("REG_MULTI_SZ"),
            RegValueType::Unknown(n) => write!(f, "REG_UNKNOWN({n})"),
        }
    }
}

impl RegValueType {
    fn from_params(type_param: i32, flags_param: i32) -> Self {
        match type_param {
            1 => {
                if flags_param == 2 {
                    RegValueType::ExpandStr
                } else {
                    RegValueType::Str
                }
            }
            2 => RegValueType::ExpandStr,
            3 => {
                if flags_param == 7 {
                    RegValueType::MultiStr
                } else {
                    RegValueType::Bin
                }
            }
            4 => RegValueType::Dword,
            other => RegValueType::Unknown(other),
        }
    }
}

/// A plugin DLL invocation (`EW_REGISTERDLL`, opcode 44).
///
/// This is the mechanism behind NSIS plugin calls. In NSIS script, a plugin
/// call like `System::Call "kernel32::VirtualAlloc(...)"` compiles to an
/// `EW_REGISTERDLL` instruction with:
/// - param 0: DLL path (e.g., `$PLUGINSDIR\System.dll`)
/// - param 1: function name (e.g., `Call`)
/// - param 2: 0 for plugin calls, non-zero for COM DLL registration
/// - param 3: `/NOUNLOAD` flag
///
/// Malware frequently abuses `System::Call` to invoke Win32 APIs directly:
/// `VirtualAlloc`, `VirtualProtect`, `CreateThread`, `NtCreateSection`, etc.
/// The actual API call string is typically pushed onto the NSIS stack before
/// the `CallInstDLL` instruction.
///
/// Source: `exec.c` case `EW_REGISTERDLL`, 7-Zip `NsisIn.cpp` lines 4381-4412.
pub struct PluginCall<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> PluginCall<'a> {
    /// Returns the DLL file path.
    ///
    /// Typically `$PLUGINSDIR\<name>.dll` — the plugin is extracted to the
    /// temp plugins directory and loaded from there.
    pub fn dll(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(0))
    }

    /// Returns the exported function name being called.
    ///
    /// Common values:
    /// - `"Call"` — `System::Call` (arbitrary Win32 API invocation)
    /// - `"Create"` — `nsDialogs::Create` (UI dialog creation)
    /// - `"DllRegisterServer"` — standard COM registration
    /// - `"DllUnregisterServer"` — standard COM unregistration
    pub fn function(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(1))
    }

    /// Returns `true` if this is a `CallInstDLL` (plugin call).
    ///
    /// When `false`, this is a `RegDLL` or `UnRegDLL` COM registration
    /// operation instead.
    pub fn is_plugin_call(&self) -> bool {
        self.entry.offset(2) == 0
    }

    /// Returns `true` if the `/NOUNLOAD` flag is set.
    ///
    /// When set, the DLL remains loaded in memory after the call returns.
    /// This is used by plugins that maintain state across multiple calls.
    pub fn no_unload(&self) -> bool {
        self.entry.offset(3) == 1
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }
}

/// An execution command found in the NSIS script.
///
/// Covers two NSIS instructions:
/// - [`ExecOp`]: `Exec` / `ExecWait` (`EW_EXECUTE`, opcode 41) — direct
///   process creation via `CreateProcess`.
/// - [`ShellExecOp`]: `ExecShell` (`EW_SHELLEXEC`, opcode 40) — shell-based
///   execution via `ShellExecuteEx`.
///
/// Both are used by malware to launch extracted payloads after decryption.
pub enum ExecCommand<'a> {
    /// `Exec` or `ExecWait` — direct process execution.
    Exec(ExecOp<'a>),
    /// `ExecShell` — shell-based file/URL execution.
    ShellExec(ShellExecOp<'a>),
}

/// An `Exec` or `ExecWait` command (`EW_EXECUTE`, opcode 41).
///
/// NSIS script equivalents:
/// - `Exec '"$INSTDIR\app.exe"'`
/// - `ExecWait '"$TEMP\setup.exe" /S' $0`
///
/// Parameters:
/// - param 0: command line (string)
/// - param 1: output variable for exit code (variable, only if `ExecWait`)
/// - param 2: wait flag (0 = `Exec`, non-zero = `ExecWait`)
///
/// Source: `exec.c` case `EW_EXECUTE`.
pub struct ExecOp<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> ExecOp<'a> {
    /// Returns the command line to execute.
    pub fn command_line(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(0))
    }

    /// Returns `true` if this is `ExecWait` (blocks until the process exits).
    pub fn is_wait(&self) -> bool {
        self.entry.offset(2) != 0
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }
}

/// A `ShellExecute` command (`EW_SHELLEXEC`, opcode 40).
///
/// NSIS script equivalent:
/// `ExecShell "open" "http://example.com"`
///
/// Parameters:
/// - param 0: shell verb (string, e.g., `"open"`)
/// - param 1: file or URL (string)
/// - param 2: parameters (string)
/// - param 3: `SW_*` show window constant (int)
/// - param 5: optional status text (string)
///
/// Source: `exec.c` case `EW_SHELLEXEC`.
pub struct ShellExecOp<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> ShellExecOp<'a> {
    /// Returns the shell verb (e.g., `"open"`, `"edit"`, `"print"`, `"runas"`).
    pub fn verb(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(0))
    }

    /// Returns the file path or URL to execute.
    pub fn file(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(1))
    }

    /// Returns the command-line parameters passed to the target.
    pub fn params(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(2))
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }
}

/// A Windows registry operation found in the NSIS script.
///
/// Covers three NSIS instructions:
/// - [`RegWrite`]: `WriteRegStr`, `WriteRegDWORD`, `WriteRegBin`, `WriteRegExpandStr`
///   (`EW_WRITEREG`, opcode 51)
/// - [`RegDelete`]: `DeleteRegKey`, `DeleteRegValue` (`EW_DELREG`, opcode 50)
/// - [`RegRead`]: `ReadRegStr`, `ReadRegDWORD` (`EW_READREGSTR`, opcode 52)
///
/// Registry operations are critical for persistence analysis. Malware commonly
/// writes to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` or similar
/// autostart keys.
pub enum RegistryOp<'a> {
    /// `WriteRegStr` / `WriteRegDWORD` / `WriteRegBin` / `WriteRegExpandStr`.
    Write(RegWrite<'a>),
    /// `DeleteRegKey` / `DeleteRegValue`.
    Delete(RegDelete<'a>),
    /// `ReadRegStr` / `ReadRegDWORD`.
    Read(RegRead<'a>),
}

/// A registry write operation (`EW_WRITEREG`, opcode 51).
///
/// NSIS script equivalents:
/// - `WriteRegStr HKLM "Software\MyApp" "Version" "1.0"`
/// - `WriteRegDWORD HKCU "Software\MyApp" "InstallCount" 1`
///
/// Parameters:
/// - param 0: registry root (int, `HKEY_*` constant)
/// - param 1: key path (string)
/// - param 2: value name (string)
/// - param 3: value data (string for Str/ExpandStr/MultiStr, offset for Bin)
/// - param 4: type (1=Str, 2=ExpandStr, 3=Bin, 4=DWORD)
/// - param 5: additional flags (used to disambiguate ExpandStr and MultiStr)
///
/// Source: `exec.c` case `EW_WRITEREG`, 7-Zip `NsisIn.cpp` lines 4560-4618.
pub struct RegWrite<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> RegWrite<'a> {
    /// Returns the registry root as a raw `HKEY_*` constant.
    pub fn root(&self) -> i32 {
        self.entry.offset(0)
    }

    /// Returns the registry root name (e.g., `"HKLM"`, `"HKCU"`).
    pub fn root_name(&self) -> &'static str {
        hkey_name(self.entry.offset(0))
    }

    /// Returns the registry key path (e.g., `"Software\\MyApp"`).
    pub fn key(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(1))
    }

    /// Returns the registry value name (e.g., `"Version"`).
    pub fn value_name(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(2))
    }

    /// Returns the value data as a string.
    ///
    /// For [`RegValueType::Str`], [`RegValueType::ExpandStr`], and
    /// [`RegValueType::MultiStr`], this is the string data. For
    /// [`RegValueType::Dword`], the string contains the numeric value.
    /// For [`RegValueType::Bin`], this is an offset into the data block.
    pub fn data(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(3))
    }

    /// Returns the registry value type.
    pub fn reg_type(&self) -> RegValueType {
        RegValueType::from_params(self.entry.offset(4), self.entry.offset(5))
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }
}

/// A registry delete operation (`EW_DELREG`, opcode 50).
///
/// NSIS script equivalents:
/// - `DeleteRegKey HKLM "Software\\MyApp"` (deletes entire key)
/// - `DeleteRegValue HKCU "Software\\MyApp" "Setting"` (deletes single value)
///
/// Parameters:
/// - param 1: registry root (int, `HKEY_*` constant)
/// - param 2: key path (string)
/// - param 3: value name (string, empty = delete entire key)
/// - param 4: flags (int)
///
/// Source: `exec.c` case `EW_DELREG`.
pub struct RegDelete<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> RegDelete<'a> {
    /// Returns the registry root as a raw `HKEY_*` constant.
    pub fn root(&self) -> i32 {
        self.entry.offset(1)
    }

    /// Returns the registry root name (e.g., `"HKLM"`, `"HKCU"`).
    pub fn root_name(&self) -> &'static str {
        hkey_name(self.entry.offset(1))
    }

    /// Returns the registry key path.
    pub fn key(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(2))
    }

    /// Returns the value name to delete.
    ///
    /// An empty string means the entire key is deleted (`DeleteRegKey`).
    pub fn value_name(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(3))
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }
}

/// A registry read operation (`EW_READREGSTR`, opcode 52).
///
/// NSIS script equivalents:
/// - `ReadRegStr $0 HKLM "Software\\MyApp" "InstallDir"`
/// - `ReadRegDWORD $1 HKCU "Software\\MyApp" "Count"`
///
/// Parameters:
/// - param 0: output variable (variable index)
/// - param 1: registry root (int, `HKEY_*` constant)
/// - param 2: key path (string)
/// - param 3: value name (string)
/// - param 4: type flag (int, determines Str vs DWORD reading)
///
/// Source: `exec.c` case `EW_READREGSTR`.
pub struct RegRead<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> RegRead<'a> {
    /// Returns the registry root as a raw `HKEY_*` constant.
    pub fn root(&self) -> i32 {
        self.entry.offset(1)
    }

    /// Returns the registry root name (e.g., `"HKLM"`, `"HKCU"`).
    pub fn root_name(&self) -> &'static str {
        hkey_name(self.entry.offset(1))
    }

    /// Returns the registry key path.
    pub fn key(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(2))
    }

    /// Returns the value name to read.
    pub fn value_name(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(3))
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }
}

/// A shortcut creation operation (`EW_CREATESHORTCUT`, opcode 45).
///
/// NSIS script equivalent:
/// `CreateShortcut "$DESKTOP\\MyApp.lnk" "$INSTDIR\\app.exe" "" "$INSTDIR\\icon.ico"`
///
/// Parameters:
/// - param 0: `.lnk` file path (string)
/// - param 1: target executable (string)
/// - param 2: command-line parameters (string)
/// - param 3: icon file (string)
/// - param 4: packed shortcut flags and hotkey (int)
///
/// Source: `exec.c` case `EW_CREATESHORTCUT`.
pub struct Shortcut<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> Shortcut<'a> {
    /// Returns the path of the `.lnk` shortcut file.
    ///
    /// Typically in `$DESKTOP`, `$SMPROGRAMS`, or `$SMSTARTUP`.
    pub fn link_path(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(0))
    }

    /// Returns the shortcut target executable path.
    pub fn target(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(1))
    }

    /// Returns the command-line parameters for the target.
    pub fn parameters(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(2))
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }
}

/// An embedded uninstaller stub (`EW_WRITEUNINSTALLER`, opcode 62).
///
/// NSIS script equivalent:
/// `WriteUninstaller "$INSTDIR\\Uninstall.exe"`
///
/// The uninstaller is a complete NSIS installer binary (PE + overlay) that
/// is embedded within the main installer's data block. Use [`decompress`](Self::decompress)
/// to extract the raw PE bytes, then parse recursively with [`NsisInstaller::from_bytes`]:
///
/// ```no_run
/// # let data = std::fs::read("installer.exe").unwrap();
/// # let inst = nsis::NsisInstaller::from_bytes(&data).unwrap();
/// for u in inst.uninstallers() {
///     let u = u.unwrap();
///     let uninst_bytes = u.decompress().unwrap();
///     let uninst = nsis::NsisInstaller::from_bytes(&uninst_bytes).unwrap();
///     assert!(uninst.is_uninstaller());
///     println!("Uninstaller has {} entries", uninst.entry_count());
/// }
/// ```
///
/// Parameters:
/// - param 0: output path (string, e.g., `$INSTDIR\Uninstall.exe`)
/// - param 1: byte offset within the data block (int)
/// - param 2: icon/patch size (int)
///
/// Source: `exec.c` case `EW_WRITEUNINSTALLER`, 7-Zip `NsisIn.cpp` lines 3599-3678.
pub struct Uninstaller<'a> {
    installer: &'a NsisInstaller<'a>,
    entry: Entry<'a>,
}

impl<'a> Uninstaller<'a> {
    /// Returns the path where the uninstaller will be written.
    pub fn path(&self) -> Result<NsisString, Error> {
        self.installer.read_string(self.entry.offset(0))
    }

    /// Returns the byte offset of the uninstaller stub within the data block.
    pub fn data_offset(&self) -> i32 {
        self.entry.offset(1)
    }

    /// Returns the icon/patch size in bytes.
    pub fn icon_size(&self) -> i32 {
        self.entry.offset(2)
    }

    /// Returns the raw payload bytes of the uninstaller NSIS overlay data.
    ///
    /// The data block at [`data_offset`](Self::data_offset) contains:
    /// 1. Icon/patch data (size = [`icon_size`](Self::icon_size)), with a
    ///    4-byte NSIS length prefix.
    /// 2. The NSIS overlay data, with its own 4-byte length prefix.
    ///
    /// This method skips the icon data and returns the overlay bytes.
    pub fn data(&self) -> &[u8] {
        let source = self.data_source();
        let Some(overlay_offset) = self.overlay_offset() else {
            return &[];
        };
        let Some(slice) = source.get(overlay_offset..) else {
            return &[];
        };
        if slice.len() < 4 {
            return &[];
        }
        let Ok((_, size)) = decompress::read_length_prefix(slice) else {
            return &[];
        };
        let Some(start) = overlay_offset.checked_add(4) else {
            return &[];
        };
        let Some(end) = start.checked_add(size as usize) else {
            return &[];
        };
        source.get(start..end).unwrap_or(&[])
    }

    /// Decompresses the uninstaller stub and returns its content.
    ///
    /// The data block at [`data_offset`](Self::data_offset) contains the
    /// icon/patch data followed by the NSIS overlay. This method skips the
    /// icon data and decompresses the overlay, then prepends the PE stub
    /// from the original installer to produce a complete uninstaller PE.
    pub fn decompress(&self) -> Result<Vec<u8>, Error> {
        let source = self.data_source();
        let overlay_offset = self.overlay_offset().ok_or(Error::TooShort {
            expected: 4,
            actual: 0,
            context: "uninstaller icon data length prefix",
        })?;

        let slice = source.get(overlay_offset..).ok_or(Error::TooShort {
            expected: overlay_offset.saturating_add(4),
            actual: source.len(),
            context: "uninstaller overlay length prefix",
        })?;
        if slice.len() < 4 {
            return Err(Error::TooShort {
                expected: overlay_offset.saturating_add(4),
                actual: source.len(),
                context: "uninstaller overlay length prefix",
            });
        }
        let (is_compressed, size) =
            decompress::read_length_prefix(slice).map_err(|_| Error::TooShort {
                expected: 4,
                actual: 0,
                context: "uninstaller overlay length prefix",
            })?;

        let start = overlay_offset.checked_add(4).ok_or(Error::TooShort {
            expected: usize::MAX,
            actual: source.len(),
            context: "uninstaller overlay overflow",
        })?;
        let end = start.checked_add(size as usize).ok_or(Error::TooShort {
            expected: usize::MAX,
            actual: source.len(),
            context: "uninstaller overlay overflow",
        })?;
        let payload = source.get(start..end).ok_or(Error::TooShort {
            expected: end,
            actual: source.len(),
            context: "uninstaller overlay payload",
        })?;

        let overlay_data = if !is_compressed {
            payload.to_vec()
        } else {
            let max_output = (size as usize).saturating_mul(10).max(64 * 1024 * 1024);
            decompress::decompress_block(
                payload,
                self.installer.compression(),
                max_output,
                Some(max_output),
            )?
        };

        // Prepend the PE stub from the original file.
        let pe_stub_size = self.installer.first_header_file_offset();
        let file_data = self.installer.file_data();
        let pe_stub = file_data
            .get(..pe_stub_size.min(file_data.len()))
            .unwrap_or(&[]);

        let mut result = Vec::with_capacity(pe_stub.len().saturating_add(overlay_data.len()));
        result.extend_from_slice(pe_stub);
        result.extend_from_slice(&overlay_data);
        Ok(result)
    }

    /// Returns the underlying [`Entry`].
    pub fn entry(&self) -> &Entry<'a> {
        &self.entry
    }

    /// Returns the byte offset within the data source where the NSIS overlay
    /// starts (after skipping the icon/patch data entry).
    fn overlay_offset(&self) -> Option<usize> {
        let source = self.data_source();
        let offset = self.source_offset();
        let slice = source.get(offset..)?;
        if slice.len() < 4 {
            return None;
        }
        let (_, icon_size) = decompress::read_length_prefix(slice).ok()?;
        // Skip: 4-byte prefix + icon data.
        let after_icon = offset.checked_add(4)?.checked_add(icon_size as usize)?;
        let after_with_prefix = after_icon.checked_add(4)?;
        if after_with_prefix <= source.len() {
            Some(after_icon)
        } else {
            None
        }
    }

    fn data_source(&self) -> &[u8] {
        if self.installer.compression_mode() == CompressionMode::Solid {
            self.installer.solid_data()
        } else {
            self.installer.file_data()
        }
    }

    fn source_offset(&self) -> usize {
        if self.installer.compression_mode() == CompressionMode::Solid {
            self.data_offset().max(0) as usize
        } else {
            self.installer
                .data_block_offset()
                .saturating_add(self.data_offset().max(0) as usize)
        }
    }
}

/// Iterator over plugin DLL calls (`EW_REGISTERDLL` entries).
///
/// Created by [`NsisInstaller::plugin_calls`].
pub struct PluginCallIter<'a> {
    installer: &'a NsisInstaller<'a>,
    entries: EntryIter<'a>,
}

impl<'a> PluginCallIter<'a> {
    pub(crate) fn new(installer: &'a NsisInstaller<'a>, entries: EntryIter<'a>) -> Self {
        Self { installer, entries }
    }
}

impl<'a> Iterator for PluginCallIter<'a> {
    type Item = Result<PluginCall<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.entries.next()? {
                Ok(e) => e,
                Err(e) => return Some(Err(e)),
            };
            if self.installer.normalize_opcode(entry.which()) == opcode::EW_REGISTERDLL {
                return Some(Ok(PluginCall {
                    installer: self.installer,
                    entry,
                }));
            }
        }
    }
}

/// Iterator over execution commands (`EW_EXECUTE` and `EW_SHELLEXEC` entries).
///
/// Created by [`NsisInstaller::exec_commands`].
pub struct ExecIter<'a> {
    installer: &'a NsisInstaller<'a>,
    entries: EntryIter<'a>,
}

impl<'a> ExecIter<'a> {
    pub(crate) fn new(installer: &'a NsisInstaller<'a>, entries: EntryIter<'a>) -> Self {
        Self { installer, entries }
    }
}

impl<'a> Iterator for ExecIter<'a> {
    type Item = Result<ExecCommand<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.entries.next()? {
                Ok(e) => e,
                Err(e) => return Some(Err(e)),
            };
            match self.installer.normalize_opcode(entry.which()) {
                opcode::EW_EXECUTE => {
                    return Some(Ok(ExecCommand::Exec(ExecOp {
                        installer: self.installer,
                        entry,
                    })));
                }
                opcode::EW_SHELLEXEC => {
                    return Some(Ok(ExecCommand::ShellExec(ShellExecOp {
                        installer: self.installer,
                        entry,
                    })));
                }
                _ => continue,
            }
        }
    }
}

/// Iterator over registry operations (`EW_WRITEREG`, `EW_DELREG`, `EW_READREGSTR` entries).
///
/// Created by [`NsisInstaller::registry_ops`].
pub struct RegistryIter<'a> {
    installer: &'a NsisInstaller<'a>,
    entries: EntryIter<'a>,
}

impl<'a> RegistryIter<'a> {
    pub(crate) fn new(installer: &'a NsisInstaller<'a>, entries: EntryIter<'a>) -> Self {
        Self { installer, entries }
    }
}

impl<'a> Iterator for RegistryIter<'a> {
    type Item = Result<RegistryOp<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.entries.next()? {
                Ok(e) => e,
                Err(e) => return Some(Err(e)),
            };
            match self.installer.normalize_opcode(entry.which()) {
                opcode::EW_WRITEREG => {
                    return Some(Ok(RegistryOp::Write(RegWrite {
                        installer: self.installer,
                        entry,
                    })));
                }
                opcode::EW_DELREG => {
                    return Some(Ok(RegistryOp::Delete(RegDelete {
                        installer: self.installer,
                        entry,
                    })));
                }
                opcode::EW_READREGSTR => {
                    return Some(Ok(RegistryOp::Read(RegRead {
                        installer: self.installer,
                        entry,
                    })));
                }
                _ => continue,
            }
        }
    }
}

/// Iterator over shortcut creation operations (`EW_CREATESHORTCUT` entries).
///
/// Created by [`NsisInstaller::shortcuts`].
pub struct ShortcutIter<'a> {
    installer: &'a NsisInstaller<'a>,
    entries: EntryIter<'a>,
}

impl<'a> ShortcutIter<'a> {
    pub(crate) fn new(installer: &'a NsisInstaller<'a>, entries: EntryIter<'a>) -> Self {
        Self { installer, entries }
    }
}

impl<'a> Iterator for ShortcutIter<'a> {
    type Item = Result<Shortcut<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.entries.next()? {
                Ok(e) => e,
                Err(e) => return Some(Err(e)),
            };
            if self.installer.normalize_opcode(entry.which()) == opcode::EW_CREATESHORTCUT {
                return Some(Ok(Shortcut {
                    installer: self.installer,
                    entry,
                }));
            }
        }
    }
}

/// Iterator over embedded uninstaller stubs (`EW_WRITEUNINSTALLER` entries).
///
/// Created by [`NsisInstaller::uninstallers`].
pub struct UninstallerIter<'a> {
    installer: &'a NsisInstaller<'a>,
    entries: EntryIter<'a>,
}

impl<'a> UninstallerIter<'a> {
    pub(crate) fn new(installer: &'a NsisInstaller<'a>, entries: EntryIter<'a>) -> Self {
        Self { installer, entries }
    }
}

impl<'a> Iterator for UninstallerIter<'a> {
    type Item = Result<Uninstaller<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.entries.next()? {
                Ok(e) => e,
                Err(e) => return Some(Err(e)),
            };
            if self.installer.normalize_opcode(entry.which()) == opcode::EW_WRITEUNINSTALLER {
                return Some(Ok(Uninstaller {
                    installer: self.installer,
                    entry,
                }));
            }
        }
    }
}
