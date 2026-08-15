; Fixture: NSIS 2.46, ANSI, solid LZMA.
SetCompressor /SOLID lzma
Name "NSIS246 Test"
OutFile "nsis246_ansi_solid.exe"
InstallDir "$PROGRAMFILES\Nsis246Test"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  SetOutPath "$INSTDIR\docs"
  File "config.ini"
SectionEnd
