; Fixture: solid zlib whose decompressed payload exceeds a 64 MiB budget.
Unicode true
SetCompressor /SOLID zlib
Name "Oversize Test"
OutFile "oversize_zlib_solid.exe"
InstallDir "$PROGRAMFILES\OversizeTest"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  File "big.bin"
  File "config.ini"
SectionEnd
