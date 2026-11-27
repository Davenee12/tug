; tug's NSIS installer hooks (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; On a real uninstall (not the silent uninstall an update runs) remove what tug itself added to
; Windows outside its folders:
;   - the "Start with Windows" entry (HKCU Run "tug", plus its Task Manager startup flag; Tauri's
;     uninstaller removes the Run value too, this keeps it with its flag),
;   - tug's own folder on the user's PATH (Settings > Developer tools > Add tug to PATH): only the
;     exact "<install folder>\bin" entry, every other entry left as written. tug.exe does this
;     itself before its files go ("tug.exe --remove-from-path", any PATH length); the NSIS
;     fallback below only handles a PATH NSIS can hold, and finds nothing left to do normally,
;   - the Spotify sign-in token in Credential Manager (tug/spotify-refresh-token).
;   - the browser extension's native messaging host registration (Chrome and Edge).
; User data (history, settings) is only removed when the user ticks Tauri's own "Delete the
; application data" box; that stays Tauri's job.
;
; On install (a first install, a reinstall, or an update), make sure the tug command,
; bin\tug.exe, really is the new one. It's often running: AI tools keep `tug mcp` open as their MCP
; server, and the browser extension starts it as its native messaging host. A running exe can't be
; overwritten, and NSIS's File then skips it, without a word under /S, so the app ended up new and
; the command old while the installer still reported success. So:
;   - before the copy, stop every process running this install's own bin\tug.exe (found the way
;     Tauri's template finds the running app, then only those whose exe is that very file), and
;     wait until the file can be written; if something else still holds it, say so and stop
;     before the copy,
;   - after the copy, run "bin\tug.exe --version" and fail the install unless it's this version.
; A failure shows a message (on the console under /S) and aborts, so the installer exits with 2.
;
; This file is included before the template declares ${VERSION} and $PassiveMode, so they're only
; used inside the macros, which expand in the template's Install section.

!include LogicLib.nsh

!define TUG_COMMAND "$INSTDIR\bin\tug.exe"

; Say why the install stopped, then stop it (exit code 2). The same as the template's own "app is
; running" failure: a message box, or under /S red text on the console that started the installer.
!macro TugFail message
  ${If} ${Silent}
    System::Call 'kernel32::AttachConsole(i -1) i .r0'
    ${If} $0 <> 0
      System::Call 'kernel32::GetStdHandle(i -11) p .r0'
      System::Call 'kernel32::SetConsoleTextAttribute(p r0, i 0x0004)'
      FileWrite $0 "${message}$\r$\n"
    ${EndIf}
  ${Else}
    MessageBox MB_ICONSTOP|MB_OK "${message}"
  ${EndIf}
  DetailPrint "${message}"
  SetErrorLevel 2
  Abort "${message}"
!macroend

!macro NSIS_HOOK_PREINSTALL
  ${If} ${FileExists} "${TUG_COMMAND}"
    StrCpy $TugStop 0
    Call TugCommandProcesses
    ${If} $TugFound > 0
    ${AndIfNot} ${Silent}
    ${AndIf} $PassiveMode <> 1
      MessageBox MB_OKCANCEL|MB_ICONINFORMATION "tug's command is running (an AI tool's tug MCP server, or the browser extension). Click OK to close it and continue. AI tools may need restarting afterwards to use tug again." IDOK tug_stop_ok
        SetErrorLevel 1
        Abort "tug's command is running, so tug wasn't updated."
      tug_stop_ok:
    ${EndIf}
    StrCpy $TugStop 1
    tug_stop_retry:
    Call TugStopCommand
    ${If} $TugFound <> 0
      ${If} ${Silent}
      ${OrIf} $PassiveMode = 1
        !insertmacro TugFail "tug's command (${TUG_COMMAND}) is in use by another program, so it can't be updated. Close that program and run the installer again."
      ${EndIf}
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "tug's command (${TUG_COMMAND}) is in use by another program, so it can't be updated. Close that program, then click Retry." IDRETRY tug_stop_retry
      SetErrorLevel 2
      Abort "tug's command is in use, so tug wasn't updated."
    ${EndIf}
  ${EndIf}
!macroend

; tug's browser extension reaches tug through bin\tug.exe as its native messaging host. Chrome and
; Edge find the host's manifest (bin\browser-host.json: which exe, and which extension may start
; it) through these per-user keys. Written on every install and update, so they follow the
; install folder; they start nothing until the extension asks, and tug answers only while
; Settings > Developer tools is on.
!define TUG_BROWSER_HOST "dev.davejames.tug.browser"

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Google\Chrome\NativeMessagingHosts\${TUG_BROWSER_HOST}" "" "$INSTDIR\bin\browser-host.json"
  WriteRegStr HKCU "Software\Microsoft\Edge\NativeMessagingHosts\${TUG_BROWSER_HOST}" "" "$INSTDIR\bin\browser-host.json"

  ; Then check that bin\tug.exe really is this version. The host's keys come first, so the
  ; extension stays registered even when this check fails and aborts the install.
  ; "tug 0.5.25", as tug-cli prints it; its crate's version is checked against this one at release
  ; (scripts/release-assets.mjs readVersions).
  nsExec::ExecToStack /TIMEOUT=15000 '"${TUG_COMMAND}" --version'
  Pop $0
  Pop $1
  Push $1
  Call TugTrimLineEnds
  Pop $1
  ${If} $0 != 0
  ${OrIf} $1 != "tug ${VERSION}"
    DetailPrint "${TUG_COMMAND} --version: exit $0, $\"$1$\""
    !insertmacro TugFail "tug's command (${TUG_COMMAND}) couldn't be updated to ${VERSION}: something kept it open. Close your AI tools and browser, then run the installer again."
  ${EndIf}
!macroend

Var TugStop    ; 1: TugCommandProcesses stops what it finds; 0: only counts
Var TugFound   ; how many processes TugCommandProcesses found running ${TUG_COMMAND}

; Stop what runs ${TUG_COMMAND}, then wait (up to about 5 seconds) until the file can be written:
; a process that was stopping may still hold it a moment, and an AI tool or the browser may start
; it again. TugFound ends as 0 once the file is free, 1 if it's still held.
Function TugStopCommand
  Push $0
  Push $1
  StrCpy $1 0
  ${Do}
    Call TugCommandProcesses
    ClearErrors
    FileOpen $0 "${TUG_COMMAND}" a
    ${IfNot} ${Errors}
      FileClose $0
      StrCpy $TugFound 0
      ${Break}
    ${EndIf}
    StrCpy $TugFound 1
    IntOp $1 $1 + 1
    ${IfThen} $1 >= 10 ${|} ${Break} ${|}
    Sleep 500
  ${Loop}
  Pop $1
  Pop $0
FunctionEnd

; Count (and with TugStop 1, stop) the processes whose exe is ${TUG_COMMAND}, into TugFound.
; Windows Restart Manager lists what has the file in use, as the template's CheckIfAppIsRunning
; does for tug.exe; then only a process whose own exe is exactly this file counts. Anything else
; using it (an editor, a virus scan, a tug.exe in another folder) is never touched: if it keeps the
; file busy, TugStopCommand reports it instead.
!define TUG_RM_PROCESS_INFO_SIZE 668 ; sizeof(RM_PROCESS_INFO): the pid is its first DWORD
Function TugCommandProcesses
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8
  Push $R0
  Push $R1

  StrCpy $TugFound 0
  StrCpy $7 "${TUG_COMMAND}"
  System::Call 'RSTRTMGR::RmStartSession(*i .r1, i 0, w) i .r0'
  ${If} $0 = 0
    System::Call 'RSTRTMGR::RmRegisterResources(i r1, i 1, *w r7, i 0, p 0, i 0, p 0) i .r0'
    ${If} $0 = 0
      ; How many, then the list (with room for a few that start in between).
      System::Call 'RSTRTMGR::RmGetList(i r1, *i .r4, *i 0 r5, p 0, *i .r6) i .r0'
      ${If} $0 = 234 ; ERROR_MORE_DATA: something has it in use
        IntOp $5 $4 + 8
        IntOp $6 $5 * ${TUG_RM_PROCESS_INFO_SIZE}
        System::Alloc $6
        Pop $8
        System::Call 'RSTRTMGR::RmGetList(i r1, *i .r4, *i r5 r5, p r8, *i .r6) i .r0'
        ${If} $0 = 0
          StrCpy $4 0
          ${While} $4 < $5
            IntOp $6 $4 * ${TUG_RM_PROCESS_INFO_SIZE}
            IntPtrOp $6 $8 + $6
            System::Call '*$6(i .r2)'
            ; SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE
            System::Call 'kernel32::OpenProcess(i 0x101001, i 0, i r2) p .r3'
            ${If} $3 P<> 0
              StrCpy $R1 ${NSIS_MAX_STRLEN}
              System::Call 'kernel32::QueryFullProcessImageNameW(p r3, i 0, w .R0, *i R1 R1) i .r0'
              ; StrCmp (==) ignores case, as Windows paths do.
              ${If} $0 <> 0
              ${AndIf} $R0 == $7
                IntOp $TugFound $TugFound + 1
                ${If} $TugStop = 1
                  DetailPrint "Stopping $R0 (process $2)"
                  System::Call 'kernel32::TerminateProcess(p r3, i 1)'
                  System::Call 'kernel32::WaitForSingleObject(p r3, i 5000)'
                ${EndIf}
              ${EndIf}
              System::Call 'kernel32::CloseHandle(p r3)'
            ${EndIf}
            IntOp $4 $4 + 1
          ${EndWhile}
        ${EndIf}
        System::Free $8
      ${EndIf}
    ${EndIf}
    System::Call 'RSTRTMGR::RmEndSession(i r1)'
  ${EndIf}

  Pop $R1
  Pop $R0
  Pop $8
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; Drop trailing CR/LF from the string on the stack.
Function TugTrimLineEnds
  Exch $0
  Push $1
  ${Do}
    StrCpy $1 $0 1 -1
    ${If} $1 == "$\r"
    ${OrIf} $1 == "$\n"
      StrCpy $0 $0 -1
    ${Else}
      ${Break}
    ${EndIf}
  ${Loop}
  Pop $1
  Exch $0
FunctionEnd

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ; tug.exe is still here: it takes its own bin folder off the user's PATH and exits at once
    ; (no window; it does nothing if the folder isn't on PATH).
    nsExec::Exec '"$INSTDIR\tug.exe" --remove-from-path'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "tug"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "tug"
    Call un.TugRemoveBinFromPath
    DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\${TUG_BROWSER_HOST}"
    DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\${TUG_BROWSER_HOST}"
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
