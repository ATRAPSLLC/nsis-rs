; Fixture: every opcode the Park fork numbers differently, compiled by its
; third release, which is built with logging (makensis /HDRINFO lists
; NSIS_CONFIG_LOG). Above the standard layout it stores, in order:
;   44 GetFontVersion, 45 GetFontName (fork only), then RegisterDLL at 46,
;   the UTF-16 file commands before FileSeek, the log instruction after
;   WriteUninstaller, and FindProc (fork only) last.
SetCompressor /FINAL zlib
Name "Park Opcodes Test"
OutFile "park3_opcodes.exe"
InstallDir "$PROGRAMFILES\ParkOpcodes"

Section "Main" SEC_MAIN
  SetOutPath $INSTDIR
  File "payload.txt"
  GetFontVersion "$FONTS\arial.ttf" $0
  GetFontName "$FONTS\arial.ttf" $1
  FileOpen $2 "$INSTDIR\wide.txt" w
  FileWriteUTF16LE $2 "wide text"
  FileSeek $2 0 END $3
  FileClose $2
  LogSet on
  LogText "installing"
  SectionGetText ${SEC_MAIN} $4
  LockWindow on
  FindProc $5 "notepad.exe"
SectionEnd
