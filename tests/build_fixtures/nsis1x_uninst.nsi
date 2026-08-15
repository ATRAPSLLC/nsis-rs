; Fixture: an NSIS 1.x installer with an uninstaller and optional sections.
; Compile with makensis.exe (zlib) - B3-1 covers bzip2. See FIXURES.md B3-2.
; Covers the 1.x header-level uninstdata_offset, a multi-row section table,
; DFS_SET/DFS_RO default_state bits, and section names in the string table.
Name "Nsis1x Uninst Test"
OutFile "nsis1x_uninst.exe"
InstallDir "$PROGRAMFILES\Nsis1xUninst"
ComponentText "Choose components:"

Section "Core Files"
  SetOutPath $INSTDIR
  File "payload.txt"
  WriteUninstaller "$INSTDIR\uninst.exe"
SectionEnd

Section "Optional Docs"
  SetOutPath $INSTDIR\docs
  File "config.ini"
SectionEnd

UninstallText "This will uninstall Nsis1x Uninst Test."

Section "Uninstall"
  Delete "$INSTDIR\payload.txt"
  Delete "$INSTDIR\docs\config.ini"
  Delete "$INSTDIR\uninst.exe"
  RMDir "$INSTDIR\docs"
  RMDir "$INSTDIR"
SectionEnd
