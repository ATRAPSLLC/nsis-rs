; Fixture: NSIS 3.x compiled for ANSI. Verify "Target: x86-ansi" in the log.
Unicode false
SetCompressor /FINAL zlib
Name "ANSI3 Deflate Test"
OutFile "ansi3_deflate_nonsolid.exe"
InstallDir "$PROGRAMFILES\Ansi3Test"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  File "config.ini"
SectionEnd
