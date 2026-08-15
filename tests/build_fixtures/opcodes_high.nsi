; Fixture: instructions above the opcode-shift boundary.
; Stock (non-logging) makensis 3.x numbering:
;   63 EW_SECTIONSET, 64 EW_INSTTYPESET, 65 EW_GETOSINFO,
;   66 EW_RESERVEDOPCODE (not emitted by any instruction),
;   67 EW_LOCKWINDOW, 68 EW_FPUTWS, 69 EW_FGETWS
Unicode true
SetCompressor /FINAL zlib
Name "Opcodes High Test"
OutFile "opcodes_high.exe"
InstallDir "$PROGRAMFILES\OpcodesHigh"

Section "Main" SEC_MAIN
  SetOutPath $INSTDIR
  File "payload.txt"

  ; SectionSetText / SectionGetText    -> opcode 63
  SectionSetText ${SEC_MAIN} "Renamed Section"
  SectionGetText ${SEC_MAIN} $0

  ; InstTypeSetText                    -> opcode 64
  InstTypeSetText 0 "Typical"

  ; GetKnownFolderPath                 -> opcode 65 (NSIS 3.06+)
  ; {3EB685DB-65F9-4CF6-A03A-E3EF65729F3D} = FOLDERID_RoamingAppData
  GetKnownFolderPath $3 "{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}"

  ; LockWindow                         -> opcode 67
  LockWindow on
  LockWindow off

  ; FileWriteUTF16LE / FileReadUTF16LE -> opcodes 68 / 69
  FileOpen $1 "$INSTDIR\wide.txt" w
  FileWriteUTF16LE $1 "wide text"
  FileClose $1
  FileOpen $1 "$INSTDIR\wide.txt" r
  FileReadUTF16LE $1 $2
  FileClose $1
SectionEnd
