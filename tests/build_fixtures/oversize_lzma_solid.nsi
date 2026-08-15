; Fixture: solid LZMA whose decompressed payload exceeds a 64 MiB budget.
Unicode true
SetCompressor /SOLID lzma
Name "Oversize Test"
OutFile "oversize_lzma_solid.exe"
InstallDir "$PROGRAMFILES\OversizeTest"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  File "big.bin"
  File "config.ini"
SectionEnd
