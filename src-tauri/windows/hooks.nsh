; tug's NSIS installer hooks (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; On a real uninstall (not the silent uninstall an update runs) remove what tug itself added to
; Windows outside its folders:
;   - the "Start with Windows" entry (HKCU Run "tug", plus its Task Manager startup flag; Tauri's
;     uninstaller removes the Run value too, this keeps it with its flag),
;   - tug's own folder on the user's PATH (Settings > Developer tools > Add tug to PATH): only the
;     exact "<install folder>\bin" entry, every other entry left as written,
;   - the Spotify sign-in token in Credential Manager (tug/spotify-refresh-token).
; User data (history, settings) is only removed when the user ticks Tauri's own "Delete the
; application data" box; that stays Tauri's job.

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "tug"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "tug"
    Call un.TugRemoveBinFromPath
    ; cmdkey ships with Windows; no window, and a missing entry is fine.
    nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:tug/spotify-refresh-token'
    Pop $0
  ${EndIf}
!macroend

; Remove "$INSTDIR\bin" (with or without a trailing backslash, any case) from the user's PATH
; (HKCU\Environment\Path), keeping every other entry. Leaves PATH untouched when it's longer than
; NSIS can hold, rather than risk writing back a truncated PATH.
Function un.TugRemoveBinFromPath
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7

  ReadRegStr $0 HKCU "Environment" "Path"
  StrCmp $0 "" tug_path_done
  StrLen $1 $0
  IntOp $2 ${NSIS_MAX_STRLEN} - 1
  IntCmp $1 $2 tug_path_done 0 tug_path_done

  StrCpy $1 ""   ; PATH being rebuilt
  StrCpy $2 $0   ; what's left to look at
  StrCpy $5 0    ; 1 once tug's entry was dropped

  tug_path_next:
    StrCmp $2 "" tug_path_rebuilt
    StrCpy $3 0
  tug_path_scan:
    StrCpy $4 $2 1 $3
    StrCmp $4 "" tug_path_cut
    StrCmp $4 ";" tug_path_cut
    IntOp $3 $3 + 1
    Goto tug_path_scan
  tug_path_cut:
    StrCpy $6 $2 $3          ; this entry
    IntOp $7 $3 + 1
    StrCpy $2 $2 "" $7       ; the rest, after the ';'
    StrCmp $6 "$INSTDIR\bin" tug_path_drop
    StrCmp $6 "$INSTDIR\bin\" tug_path_drop
    StrCmp $1 "" 0 tug_path_append
      StrCpy $1 $6
      Goto tug_path_next
  tug_path_append:
    StrCpy $1 "$1;$6"
    Goto tug_path_next
  tug_path_drop:
    StrCpy $5 1
    Goto tug_path_next

  tug_path_rebuilt:
    StrCmp $5 1 0 tug_path_done
    StrCmp $1 "" 0 tug_path_write
      DeleteRegValue HKCU "Environment" "Path"
      Goto tug_path_notify
  tug_path_write:
    WriteRegExpandStr HKCU "Environment" "Path" $1
  tug_path_notify:
    ; WM_SETTINGCHANGE "Environment" to all windows (HWND_BROADCAST), so new terminals see it.
    SendMessage 0xFFFF 0x1A 0 "STR:Environment" /TIMEOUT=2000

  tug_path_done:
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd
