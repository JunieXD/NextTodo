; The installer's hooks (docs/desktop.md#命令行).

; A running `nexttodo mcp` keeps the command line from being replaced, not from being renamed: the old
; one moves aside for the new, and those left by earlier installs go once nothing runs them.
!macro NSIS_HOOK_PREINSTALL
  Delete "$INSTDIR\bin\*.old"
  ${If} ${FileExists} "$INSTDIR\bin\nexttodo.exe"
    GetTempFileName $0 "$INSTDIR\bin"
    Delete $0
    Rename "$INSTDIR\bin\nexttodo.exe" "$0.old"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\bin\*.old"
  RMDir "$INSTDIR\bin"
  RMDir "$INSTDIR"
!macroend
