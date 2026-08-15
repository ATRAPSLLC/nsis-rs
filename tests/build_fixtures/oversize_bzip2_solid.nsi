; Fixture: solid bzip2 whose decompressed payload exceeds a 64 MiB budget.
Unicode true
SetCompressor /SOLID bzip2
Name "Oversize Test"
OutFile "oversize_bzip2_solid.exe"
InstallDir "$PROGRAMFILES\OversizeTest"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  File "big.bin"
  File "config.ini"
SectionEnd
