; Fixture: NSIS 1.x installer - exercises NsisVersion::V1 and the legacy
; "nsisinstall" firstheader signature path. Deliberately minimal: only syntax
; that the 1.x compiler accepts (no SetCompressor, no Unicode, no defines).
Name "Nsis1x Test"
OutFile "nsis1x.exe"
InstallDir "$PROGRAMFILES\Nsis1xTest"

Section "Main"
  SetOutPath $INSTDIR
  File payload.txt
SectionEnd
