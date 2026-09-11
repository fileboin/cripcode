; CripCode NSIS installer hooks.
;
; Guards against the installer truncating the main executable to 0 bytes.
; NSIS's `File` instruction deletes the target and then creates a new file;
; if the write fails (e.g. the old instance's image lock has not been fully
; released right after the process kill, or an antivirus filter grabs the
; freshly created file), the result is an empty 0-byte executable and an
; aborted install. These hooks make that failure impossible:
;
;   PREINSTALL  - frees the target name before any copy happens:
;                 delete the old exe; if it is locked, rename it aside
;                 (a rename succeeds even while the executable is running).
;                 If the name cannot be freed at all, abort cleanly BEFORE
;                 NSIS can truncate anything.
;   POSTINSTALL - verifies the freshly written executable is non-empty and,
;                 if it is not, restores the previous version and aborts
;                 with a clear error instead of silently leaving a broken
;                 installation behind.

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Preparing replacement of ${MAINBINARYNAME}.exe..."
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ClearErrors
    Delete "$INSTDIR\${MAINBINARYNAME}.exe"
    ${If} ${Errors}
      ClearErrors
      Rename "$INSTDIR\${MAINBINARYNAME}.exe" "$INSTDIR\${MAINBINARYNAME}.exe.old"
      ${If} ${Errors}
        DetailPrint "${MAINBINARYNAME}.exe is locked by another process."
        Abort "${MAINBINARYNAME}.exe is locked by another process. Close all running instances and run the installer again."
      ${EndIf}
      DetailPrint "Old executable was locked and has been moved aside; it will be removed after install."
      Delete /REBOOTOK "$INSTDIR\${MAINBINARYNAME}.exe.old"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $R7
  Push $R8
  Push $R9
  ${GetSize} "$INSTDIR" "/M=${MAINBINARYNAME}.exe /S=0K /G=0" $R9 $R8 $R7
  ${If} $R9 == 0
  ${OrIf} $R9 == ""
    DetailPrint "Verification failed: ${MAINBINARYNAME}.exe is empty."
    ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe.old"
      Delete "$INSTDIR\${MAINBINARYNAME}.exe"
      ClearErrors
      Rename "$INSTDIR\${MAINBINARYNAME}.exe.old" "$INSTDIR\${MAINBINARYNAME}.exe"
      ${IfNot} ${Errors}
        DetailPrint "Previous version restored."
      ${EndIf}
    ${EndIf}
    Pop $R9
    Pop $R8
    Pop $R7
    Abort "Installation aborted: ${MAINBINARYNAME}.exe was not written correctly. The previous version was restored if it was available."
  ${EndIf}
  Delete "$INSTDIR\${MAINBINARYNAME}.exe.old"
  Pop $R9
  Pop $R8
  Pop $R7
!macroend
