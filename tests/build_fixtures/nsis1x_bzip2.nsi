; Fixture: an NSIS 1.x installer compressed with bzip2.
; Compile with makensis-bz2.exe, NOT makensis.exe - 1.98 picks its compressor
; when makensis is built, not per script. See FIXURES.md B3-1.
Name "Nsis1x BZip2 Test"
OutFile "nsis1x_bzip2.exe"
InstallDir "$PROGRAMFILES\Nsis1xBZip2"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
SectionEnd
