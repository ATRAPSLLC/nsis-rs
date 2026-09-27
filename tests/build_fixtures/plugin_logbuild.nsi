; Fixture: a logging-enabled build (NSIS_CONFIG_LOG=yes) that calls a plugin.
;
; A plugin call compiles to EW_SETFLAG with four operands - the fourth marks
; the saved-status restore - so a table that gives EW_SETFLAG three reads
; every such entry as malformed under both opcode layouts, and cannot tell a
; logging build from a standard one. opcodes_logbuild.nsi calls no plugin and
; never reaches that.
Unicode true
SetCompressor /FINAL zlib
Name "Plugin Log Build Test"
OutFile "plugin_logbuild.exe"
InstallDir "$PROGRAMFILES\PluginLogBuild"

Section "Main" SEC_MAIN
  SetOutPath $INSTDIR
  File "payload.txt"
  LogSet on
  System::Call "kernel32::GetTickCount() i .r0"
  ; Above the shifted boundary, so a wrong layout misnames it.
  SectionGetText ${SEC_MAIN} $1
SectionEnd
