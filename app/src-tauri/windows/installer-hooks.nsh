; Additions to Tauri's Windows installer (NSIS).

; Records the install date in the "Installed apps" entry. Without it,
; Windows cannot place EraseCord among recently installed programs, and a
; list sorted by date shows it at the very end.
!macro NSIS_HOOK_POSTINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  ${GetTime} "" "L" $0 $1 $2 $3 $4 $5 $6
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallDate" "$2$1$0"
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
