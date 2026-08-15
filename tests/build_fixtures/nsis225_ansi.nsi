; Fixture: NSIS 2.25, ANSI, deflate.
SetCompressor /FINAL zlib
Name "NSIS225 Test"
OutFile "nsis225_ansi.exe"
InstallDir "$PROGRAMFILES\Nsis225Test"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  SetOutPath "$INSTDIR\docs"
  File "config.ini"
SectionEnd
