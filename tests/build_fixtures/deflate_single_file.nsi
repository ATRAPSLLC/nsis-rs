; Test fixture: deflate non-solid, a single File instruction.
;
; Previously named ansi_deflate.nsi on the assumption that omitting
; "Unicode true" yields an ANSI build. It does not: makensis 3.x defaults to
; Unicode, and the compiled fixture's string table is UTF-16LE. Real ANSI
; coverage needs an explicit "Unicode false" -- see ansi3_deflate_nonsolid.nsi.
Name "Deflate Single File Test"
OutFile "deflate_single_file.exe"
InstallDir "$TEMP\nsis_test"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
SectionEnd
