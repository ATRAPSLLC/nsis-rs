; Fixture: same instruction set as opcodes_high.nsi, but compiled with a
; logging-enabled makensis (NSIS_CONFIG_LOG=yes). The extra EW_LOG instruction
; at opcode 63 shifts everything above WriteUninstaller (62) up by one:
;   63 EW_LOG, 64 EW_SECTIONSET, 65 EW_INSTTYPESET, 66 EW_GETOSINFO,
;   67 EW_RESERVEDOPCODE, 68 EW_LOCKWINDOW, 69 EW_FPUTWS, 70 EW_FGETWS
; Only distinguishable from opcodes_high.exe by the compiler used - record it.
Unicode true
SetCompressor /FINAL zlib
Name "Opcodes Log Build Test"
OutFile "opcodes_logbuild.exe"
InstallDir "$PROGRAMFILES\OpcodesLogBuild"

Section "Main" SEC_MAIN
  SetOutPath $INSTDIR
  File "payload.txt"

  ; LogSet / LogText                   -> opcode 63 (logging builds only)
  LogSet on
  LogText "installing"

  ; SectionSetText / SectionGetText    -> opcode 64
  SectionSetText ${SEC_MAIN} "Renamed Section"
  SectionGetText ${SEC_MAIN} $0

  ; InstTypeSetText                    -> opcode 65
  InstTypeSetText 0 "Typical"

  ; GetKnownFolderPath                 -> opcode 66 (NSIS 3.06+)
  GetKnownFolderPath $3 "{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}"

  ; LockWindow                         -> opcode 68
  LockWindow on
  LockWindow off

  ; FileWriteUTF16LE / FileReadUTF16LE -> opcodes 69 / 70
  FileOpen $1 "$INSTDIR\wide.txt" w
  FileWriteUTF16LE $1 "wide text"
  FileClose $1
  FileOpen $1 "$INSTDIR\wide.txt" r
  FileReadUTF16LE $1 $2
  FileClose $1
SectionEnd
