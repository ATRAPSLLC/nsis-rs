; Fixture: NSIS 3.x ANSI with Latin-1 high bytes (0xFC-0xFF) in paths.
; SAVE THIS FILE AS WINDOWS-1252.
Unicode false
SetCompressor /FINAL zlib
Name "ANSI3 Latin1 Test"
OutFile "ansi3_latin1.exe"
InstallDir "$PROGRAMFILES\Ansi3Latin1"

Section "Sektion für Übersetzungen"
  SetOutPath "$INSTDIR\Sprachen"
  File /oname=grüße.txt "payload.txt"
  File /oname=þýÿ.ini "config.ini"
SectionEnd
