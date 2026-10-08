; Test fixture: three language tables, one of them right-to-left, and a
; custom LangString the script refers to from an instruction.
SetCompressor /SOLID lzma
Unicode true
Name "Language Test"
OutFile "langstrings.exe"
InstallDir "$TEMP\nsis_test"

LoadLanguageFile "${NSISDIR}\Contrib\Language files\English.nlf"
LoadLanguageFile "${NSISDIR}\Contrib\Language files\German.nlf"
LoadLanguageFile "${NSISDIR}\Contrib\Language files\Hebrew.nlf"

LangString Greeting ${LANG_ENGLISH} "Hello from $(^Name)"
LangString Greeting ${LANG_GERMAN} "Hallo von $(^Name)"
LangString Greeting ${LANG_HEBREW} "Shalom from $(^Name)"

Section "Main"
  SetOutPath $INSTDIR
  DetailPrint "$(Greeting)"
  File "payload.txt"
SectionEnd
