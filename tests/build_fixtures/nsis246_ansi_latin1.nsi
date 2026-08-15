; Fixture: NSIS 2.46 ANSI with Latin-1 high bytes (0xFC-0xFF) in paths.
; SAVE THIS FILE AS WINDOWS-1252.
SetCompressor /FINAL zlib
Name "NSIS246 Latin1 Test"
OutFile "nsis246_ansi_latin1.exe"
InstallDir "$PROGRAMFILES\Nsis246Latin1"

Section "Sektion für Übersetzungen"
  SetOutPath "$INSTDIR\Sprachen"
  File /oname=grüße.txt "payload.txt"
  File /oname=þýÿ.ini "config.ini"
SectionEnd
