; Fixture: one instance of each operand form whose meaning depends on the
; instruction, for the tests that check what every slot of an instruction
; reads and writes against what the NSIS runtime (exec.c) does with it.
;
; Each command is placed so its instruction can be found by opcode and by the
; operands that pick its form. The script is never run; it only has to
; compile to the forms the tests name.
Unicode true
SetCompressor /FINAL zlib
Name "Semantics Test"
OutFile "semantics.exe"
InstallDir "$PROGRAMFILES\SemanticsTest"
RequestExecutionLevel user

Var Handle
Var Line

Function Helper
  ; The string stack: Push, Exch (Push/Exch/Pop), Pop.
  Push "$INSTDIR"
  Exch $0
  Pop $1
FunctionEnd

Section "Main" SEC_MAIN
  ; SetOutPath writes $OUTDIR; File reads it.
  SetOutPath "$INSTDIR"
  File "payload.txt"
  CreateDirectory "$INSTDIR\sub"

  ; ExecWait with an exit-code variable, ExecWait without, Exec.
  ExecWait '"$INSTDIR\tool.exe" /a' $2
  ExecWait '"$INSTDIR\tool.exe" /b'
  Exec '"$INSTDIR\tool.exe" /c'
  ExecShellWait "open" "$INSTDIR\payload.txt"

  ; Error flag: cleared, set by a failing command, tested.
  ClearErrors
  FileOpen $Handle "$INSTDIR\payload.txt" r
  IfErrors done_file
  FileRead $Handle $Line
  FileSeek $Handle 0 END $3
  FileSeek $Handle 0 SET
  FileClose $Handle
  done_file:

  ; Integer and string compares, arithmetic with a division.
  IntOp $4 $3 / 2
  IntOp $5 $4 + 1
  IntCmp $4 $5 equal less more
  equal:
  less:
  more:
  StrCmp $Line "x" 0 +2
  StrCpy $6 $Line 3 1

  ; Window messages with and without an output variable.
  FindWindow $7 "Shell_TrayWnd"
  SendMessage $7 0x10 0 0 $8
  SendMessage $7 0x10 0 "STR:text"
  ; An image by path into a window handle, with its handle returned; and
  ; one by resource id into a dialog control, without.
  LoadAndSetImage /STRINGID /RESIZETOFIT $7 0 0x10 "$INSTDIR\a.bmp" $R1
  LoadAndSetImage /GETDLGITEM 1200 0 0 103

  ; Registry: string, dword, binary, delete value, delete key, read.
  WriteRegStr HKCU "Software\SemanticsTest" "Str" "$INSTDIR"
  WriteRegDWORD HKCU "Software\SemanticsTest" "Dword" $4
  WriteRegBin HKCU "Software\SemanticsTest" "Bin" 0102030405
  ReadRegStr $9 HKCU "Software\SemanticsTest" "Str"
  DeleteRegValue HKCU "Software\SemanticsTest" "Str"
  DeleteRegKey HKCU "Software\SemanticsTest"
  SetShellVarContext all
  WriteRegStr SHCTX "Software\SemanticsTest" "Ctx" "1"

  ; INI: write a value, delete a section.
  WriteINIStr "$INSTDIR\a.ini" "Sec" "Key" "Value"
  DeleteINISec "$INSTDIR\a.ini" "Sec"

  ; Status output, reboot flag, a plugin call, a function call.
  SetDetailsPrint none
  DetailPrint "quiet"
  SetDetailsPrint both
  IfRebootFlag 0 +2
  Rename /REBOOTOK "$INSTDIR\a.ini" "$INSTDIR\b.ini"
  System::Call "kernel32::GetTickCount() i .r0"
  Call Helper
  GetKnownFolderPath $R0 "{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}"
  CreateShortcut "$INSTDIR\a.lnk" "$INSTDIR\tool.exe" "" "" 0 SW_SHOWNORMAL "" "Description"
  WriteUninstaller "uninstall.exe"
SectionEnd

Section "un.Main"
  Delete "$INSTDIR\uninstall.exe"
SectionEnd
