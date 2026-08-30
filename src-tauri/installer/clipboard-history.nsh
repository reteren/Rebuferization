; Rebuffer installer: "Disable Windows clipboard history (Win+V)" option.
;
; Wired up through `bundle.windows.nsis.installerHooks` in tauri.conf.json. Tauri
; !includes this file at the very top of its NSIS template (installer.nsi), before
; any page or section is declared, so everything here works through the supported
; hook macros and MUI's page custom-function defines -- no custom .nsi template is
; needed, which is what lets it survive a Tauri bundler upgrade.
;
; HOW IT WORKS
;   * The checkbox is attached to the installer's FIRST MUI page (the Welcome
;     page). MUI's MUI_PAGE_CUSTOMFUNCTION_SHOW / _LEAVE defines are one-shot:
;     the first page macro that calls MUI_PAGE_FUNCTION_CUSTOM consumes them,
;     and the Welcome page is first. The checkbox is created when that page
;     shows, its state is captured when the page is left.
;   * The state is applied in NSIS_HOOK_POSTINSTALL, which runs after the user
;     has committed to installing (files are already copied). In passive or
;     silent mode the Welcome page is skipped, so nothing is changed.
;   * The uninstaller offers to restore the setting in NSIS_HOOK_PREUNINSTALL,
;     and only when Rebuffer is the one who disabled it (recorded at install),
;     never when the user had already disabled it themselves.
;
; The setting is per-user, so the installer records what it did under the
; installing user's HKCU. The template defines MANUPRODUCTKEY only AFTER this
; file is included, so the key is spelled out here -- it must match the
; `publisher` / `productName` in tauri.conf.json.
!define REBUFFER_REGKEY "Software\reteren\Rebuffer"

; ---------------------------------------------------------------------------
; Variables
; ---------------------------------------------------------------------------
Var RebufferDisableClipboardCheckbox
Var RebufferDisableClipboard

; ---------------------------------------------------------------------------
; Install: checkbox on the Welcome page
; ---------------------------------------------------------------------------
!define /ifndef WS_EX_LAYOUTRTL 0x00400000
!define MUI_PAGE_CUSTOMFUNCTION_SHOW RebufferWelcomeShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE RebufferWelcomeLeave

Function RebufferWelcomeShow
  ; Same technique the Tauri template itself uses for the uninstaller's
  ; "Delete app data" checkbox: find the inner dialog and create the control
  ; directly, scaled by DPI. Unchecked by default.
  FindWindow $1 "#32770" "" $HWNDPARENT ; Find inner dialog
  System::Call "user32::GetDpiForWindow(p r1) i .r2"
  ${If} $(^RTL) = 1
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE} | ${WS_EX_LAYOUTRTL}"
    IntOp $4 30 * $2
  ${Else}
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE}"
    IntOp $4 120 * $2
  ${EndIf}
  IntOp $5 105 * $2
  IntOp $6 420 * $2
  IntOp $7 22 * $2
  IntOp $4 $4 / 96
  IntOp $5 $5 / 96
  IntOp $6 $6 / 96
  IntOp $7 $7 / 96
  System::Call 'user32::CreateWindowEx(i r3, w "${__NSD_CheckBox_CLASS}", w "Disable Windows clipboard history (Win+V) for the current user", i ${__NSD_CheckBox_STYLE}, i r4, i r5, i r6, i r7, p r1, i0, i0, i0) i .s'
  Pop $RebufferDisableClipboardCheckbox
  SendMessage $HWNDPARENT ${WM_GETFONT} 0 0 $1
  SendMessage $RebufferDisableClipboardCheckbox ${WM_SETFONT} $1 1
FunctionEnd

Function RebufferWelcomeLeave
  SendMessage $RebufferDisableClipboardCheckbox ${BM_GETCHECK} 0 0 $RebufferDisableClipboard
FunctionEnd

; ---------------------------------------------------------------------------
; Install: apply the choice once the user has committed
; ---------------------------------------------------------------------------
!macro NSIS_HOOK_POSTINSTALL
  ${If} $RebufferDisableClipboard = 1
    Call RebufferApplyClipboardHistory
  ${Else}
    ; Not requested this time. Clear any record a previous install left behind,
    ; so the uninstaller never restores a setting this install did not touch.
    WriteRegDWORD HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryDisabledByRebuffer" 0
  ${EndIf}
!macroend

Function RebufferApplyClipboardHistory
  ; HKCU here is the account that ran the (elevated, perMachine) installer.
  ; An absent value means enabled. It must be detected with the error flag:
  ; LogicLib's `=` is a NUMERIC comparison, so `$0 = ""` coerces the empty
  ; string to 0 and would also match an explicit 0 — which would record "was
  ; enabled" for a user who had deliberately turned it off, and the uninstaller
  ; would then switch it back on. That is the exact case this design forbids.
  ClearErrors
  ReadRegDWORD $0 HKCU "Software\Microsoft\Clipboard" "EnableClipboardHistory"
  ${If} ${Errors}
    StrCpy $0 1
  ${EndIf}
  ${If} $0 <> 0
    ; It was enabled (or unset, which means enabled). Disable it and record
    ; that Rebuffer is the one who did, so the uninstaller can restore it.
    WriteRegDWORD HKCU "Software\Microsoft\Clipboard" "EnableClipboardHistory" 0
    WriteRegDWORD HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryDisabledByRebuffer" 1
    WriteRegDWORD HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryWasEnabled" 1
  ${Else}
    ; Already disabled -- the user chose that themselves. Record that Rebuffer
    ; did NOT disable it, so the uninstaller leaves it alone.
    WriteRegDWORD HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryDisabledByRebuffer" 0
    WriteRegDWORD HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryWasEnabled" 0
  ${EndIf}
FunctionEnd

; ---------------------------------------------------------------------------
; Uninstall: offer to restore what the installer changed
; ---------------------------------------------------------------------------
!macro NSIS_HOOK_PREUNINSTALL
  ReadRegDWORD $0 HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryDisabledByRebuffer"
  ${If} $0 = 1
    ; Offer only while the setting is still off: if the user re-enabled it in
    ; the meantime, leave their choice alone.
    ReadRegDWORD $1 HKCU "Software\Microsoft\Clipboard" "EnableClipboardHistory"
    ${If} $1 = 0
      MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Rebuffer disabled Windows clipboard history (Win+V) during installation. Re-enable it now?" /SD IDNO IDYES rebuffer_restore_clipboard_history
      Goto rebuffer_restore_clipboard_history_done
      rebuffer_restore_clipboard_history:
      Call un.RebufferRestoreClipboardHistory
      rebuffer_restore_clipboard_history_done:
    ${EndIf}
  ${EndIf}
  ; Clean up the record either way; the template removes the app key itself if
  ; it becomes empty.
  DeleteRegValue HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryDisabledByRebuffer"
  DeleteRegValue HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryWasEnabled"
!macroend

; NSIS keeps installer and uninstaller code in separate scopes: a function
; called from an uninstall section must be declared with the `un.` prefix, or
; makensis refuses to build at all.
Function un.RebufferRestoreClipboardHistory
  ReadRegDWORD $0 HKCU "${REBUFFER_REGKEY}" "ClipboardHistoryWasEnabled"
  ${If} $0 = 1
    WriteRegDWORD HKCU "Software\Microsoft\Clipboard" "EnableClipboardHistory" 1
  ${Else}
    DeleteRegValue HKCU "Software\Microsoft\Clipboard" "EnableClipboardHistory"
  ${EndIf}
FunctionEnd
