; Fixture: Jim Park Unicode fork.
SetCompressor /SOLID lzma
Name "Park Test"
OutFile "park1_unicode.exe"
InstallDir "$PROGRAMFILES\ParkTest"

Section "Main"
  SetOutPath $INSTDIR
  File "payload.txt"
  SetOutPath "$INSTDIR\docs"
  File "config.ini"
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\docs\config.ini"
  RMDir "$INSTDIR\docs"
  Delete "$INSTDIR\payload.txt"
  RMDir "$INSTDIR"
SectionEnd
