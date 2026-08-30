# Rebuffer NSIS installer: "Disable Windows clipboard history (Win+V)"

The NSIS installer offers a checkbox — **"Disable Windows clipboard history
(Win+V) for the current user"** — which turns off Windows' own clipboard
history so it does not compete with Rebuffer (SPEC §4's one-click helper).
The checkbox defaults to **unticked**: the installer never changes a Windows
setting the user did not ask about.

## What it does

When ticked, the installer writes the per-user registry value:

```
HKCU\Software\Microsoft\Clipboard\EnableClipboardHistory = 0 (DWORD)
```

`0` disables Win+V history for that user; `1` (or the value being absent)
means enabled. Windows applies it without a reboot (a new explorer.exe or a
sign-out/sign-in guarantees it; the current session may keep the old
history until then).

## Which user it applies to

The value is **per-user (`HKCU`)**, but the installer runs elevated because
`bundle.windows.nsis.installMode` is `perMachine`. When a normal user installs
on their own machine, UAC elevation keeps the same Windows account, so `HKCU`
is that user's hive and the setting applies to the person who will use
Rebuffer. When the installer is run from a *different* account (an admin
deploying for someone else, `runas` with alternate credentials, or an SCCM /
silent deployment running as SYSTEM), `HKCU` is **that account's** hive — the
setting will not apply to the person who will actually use the app. That is
why the checkbox label says "for the current user".

## What happens on uninstall

The installer records what it did under the app's own registry key, so the
uninstaller can tell "Rebuffer disabled it" from "the user had already
disabled it themselves":

```
HKCU\Software\reteren\Rebuffer\ClipboardHistoryDisabledByRebuffer  (DWORD)
HKCU\Software\reteren\Rebuffer\ClipboardHistoryWasEnabled          (DWORD)
```

(This key matches the `publisher`/`productName` in `tauri.conf.json`; the
Tauri template's `MANUPRODUCTKEY` is only defined after the hooks file is
included, so the path is spelled out in the hook.)

- **Install, checkbox ticked, history was enabled (or unset):** the value is
  written to `0` and `ClipboardHistoryDisabledByRebuffer=1` (with
  `ClipboardHistoryWasEnabled=1`).
- **Install, checkbox ticked, history already disabled:** nothing is written;
  `ClipboardHistoryDisabledByRebuffer=0`. This is the "user chose it
  themselves" case.
- **Install, checkbox unticked:** `ClipboardHistoryDisabledByRebuffer=0`, so a
  record from a previous install cannot make the uninstaller restore anything.

On uninstall, `NSIS_HOOK_PREUNINSTALL` reads the record:

- Only when `ClipboardHistoryDisabledByRebuffer=1` **and** the setting is
  still `0` (the user did not re-enable it in the meantime) does the
  uninstaller offer: *"Rebuffer disabled Windows clipboard history (Win+V)
  during installation. Re-enable it now?"* Default button is **No**; in
  silent/passive uninstall the default is also No (nothing is restored).
- If the user confirms, `EnableClipboardHistory` is restored to `1` (the value
  recorded as having been there before install).
- If Rebuffer did **not** disable it (`ClipboardHistoryDisabledByRebuffer=0`),
  the uninstaller does not prompt and does not touch the setting — a user who
  disabled Win+V history on their own keeps it disabled.

The record values are deleted during uninstall; the Tauri template removes the
app registry key itself once it is empty.

## Undoing it by hand

The setting lives in the Windows Settings UI: **Settings → System →
Clipboard → Clipboard history** (turn it on), or set the value directly:

```powershell
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Clipboard" -Name "EnableClipboardHistory" -Value 1
```

To clear Rebuffer's install record:

```powershell
Remove-ItemProperty -Path "HKCU:\Software\reteren\Rebuffer" -Name "ClipboardHistoryDisabledByRebuffer"
Remove-ItemProperty -Path "HKCU:\Software\reteren\Rebuffer" -Name "ClipboardHistoryWasEnabled"
```

## How it is wired (for the next editor)

Everything lives in `src-tauri/installer/clipboard-history.nsh`, referenced
from `tauri.conf.json`:

```json
"bundle": {
  "windows": {
    "nsis": {
      "installMode": "perMachine",
      "installerHooks": "installer/clipboard-history.nsh"
    }
  }
}
```

The path is relative to `src-tauri/`. The hook file is `!include`d by Tauri's
`installer.nsi` at the very top (before any page), so it uses only supported
extension points — which is what lets it survive bundler upgrades without a
custom `.nsi` template:

- **`MUI_PAGE_CUSTOMFUNCTION_SHOW` / `MUI_PAGE_CUSTOMFUNCTION_LEAVE`** — MUI's
  page custom-function defines are *one-shot* (the first page macro that calls
  `MUI_PAGE_FUNCTION_CUSTOM` consumes them), so the checkbox lands on the
  installer's first MUI page, the Welcome page. The checkbox is created there
  with the same `CreateWindowEx`/DPI-scaling technique the Tauri template uses
  for its own uninstaller "Delete app data" checkbox, and its state is captured
  when the page is left.
- **`NSIS_HOOK_POSTINSTALL`** — applies the recorded choice after the user has
  committed to installing (files already copied).
- **`NSIS_HOOK_PREUNINSTALL`** — offers to restore, guarded by the recorded
  flag and the live setting.

If the checkbox position on the Welcome page looks off in a real install, only
the `IntOp $4/$5/$6/$7` coordinates in `RebufferWelcomeShow` need adjusting.

### Upgrade note

A future Tauri bundler bump should not require changes here: the hooks and
MUI defines are stable. If the template ever stops landing these defines on the
Welcome page (e.g. a new page inserted before it), the checkbox will simply
appear on whatever page is first — verify once after a `tauri` upgrade.