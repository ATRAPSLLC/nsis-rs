; Fixture: directory-tracking cases for destination-path reconstruction (NSIS 2.46, ANSI).
SetCompressor /SOLID lzma
Name "Dirs Test"
OutFile "dirs_nsis246_ansi_solid.exe"
InstallDir "$PROGRAMFILES\DirsTest"

Section "Main"
  ; 1. plain install root
  SetOutPath $INSTDIR
  File "payload.txt"

  ; 2. nested subdirectory
  SetOutPath "$INSTDIR\Lang"
  File /oname=de_DE.ini "config.ini"
  File /oname=en_US.ini "config.ini"

  ; 3. deeper nesting
  SetOutPath "$INSTDIR\Lang\regional"
  File /oname=de_AT.ini "config.ini"

  ; 4. $OUTDIR-relative SetOutPath (must resolve against the current prefix)
  SetOutPath "$OUTDIR\extra"
  File /oname=nested.ini "config.ini"

  ; 5. plain CreateDirectory — must NOT become a prefix for later files
  CreateDirectory "$SMPROGRAMS\DirsTest"
  SetOutPath "$INSTDIR\after"
  File /oname=after.txt "payload.txt"

  ; 6. absolute name: keeps its own directory, ignores the current prefix
  SetOutPath $PLUGINSDIR
  File /oname=app-64.7z "payload.txt"

  ; 7. back to the root
  SetOutPath $INSTDIR
  File /oname=last.txt "payload.txt"
SectionEnd
