; Fixture: every opcode the Park fork numbers differently, compiled by its
; second release, which has GetFontVersion but neither GetFontName nor logging
; (makensis /HDRINFO). Above the standard layout it stores, in order:
;   44 GetFontVersion (fork only), then RegisterDLL at 45,
;   the UTF-16 file commands before FileSeek, and FindProc (fork only) last.
SetCompressor /FINAL zlib
Name "Park Opcodes Test"
OutFile "park2_opcodes.exe"
InstallDir "$PROGRAMFILES\ParkOpcodes"

Section "Main" SEC_MAIN
  SetOutPath $INSTDIR
  File "payload.txt"
  GetFontVersion "$FONTS\arial.ttf" $0
  FileOpen $2 "$INSTDIR\wide.txt" w
  FileWriteUTF16LE $2 "wide text"
  FileSeek $2 0 END $3
  FileClose $2
  SectionGetText ${SEC_MAIN} $4
  LockWindow on
  FindProc $5 "notepad.exe"
SectionEnd
