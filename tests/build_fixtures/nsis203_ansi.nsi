; Fixture: NSIS 2.03 (or earlier), ANSI, deflate.
SetCompressor /FINAL zlib
Name "NSIS203 Test"
OutFile "nsis203_ansi.exe"
InstallDir "$PROGRAMFILES\Nsis203Test"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  SetOutPath "$INSTDIR\docs"
  File "config.ini"
SectionEnd
