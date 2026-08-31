# W37 — Installer verification: `Rebuffer_0.1.0_x64-setup.exe`

Date: 31 Aug 2026. Artifact: `src-tauri\target\release\bundle\nsis\Rebuffer_0.1.0_x64-setup.exe`
(3,107,652 bytes, built 14:55 today). Everything below is **static** — nothing was
executed, nothing was installed, no registry value was touched. The user is on the
machine; per the brief I went as far as the safe static limit and stopped.

## 1. Static extraction of the compiled installer — the hook IS in it

The installer was read with 7-Zip (`Type = Nsis, LZMA:23, solid`) and the solid
LZMA block was decompressed (`scripts/installer/decompress_nsis.py`), which yields
the compiled NSIS script data plus the embedded uninstaller executable (its own
NSIS block decompressed the same way). All target strings are present in the
**compiled** binary as UTF-16:

| String | Found in compiled data |
|---|---|
| `Disable Windows clipboard history (Win+V) for the current user` (checkbox label) | **yes** (installer script block) |
| `Software\Microsoft\Clipboard` (registry path) | **yes** |
| `EnableClipboardHistory` (registry value) | **yes** |
| `Software\reteren\Rebuffer` (the app's own record key) | **yes** |
| `ClipboardHistoryDisabledByRebuffer` / `ClipboardHistoryWasEnabled` | **yes** |
| `Rebuffer disabled Windows clipboard history (Win+V) during installation. Re-enable it now?` (uninstall prompt) | **yes** (embedded uninstaller block) |

So the "did the hook actually make it in" risk is answered: **the checkbox, the
registry path and the uninstall prompt are all compiled into the installer.** The
hook is not silently dropped by a wrong config key or macro name.

**Icon — NOT the Rebuffer artwork.** The installer's icon resource was extracted
and compared against the app's own icons. The setup.exe icon is **not** the app
artwork:

- setup.exe icon vs `rebuffer.exe` icon (the app binary, which carries the new
  artwork): **934 of 1024 pixels differ** at 32×32.
- setup.exe icon vs the app `32x32.png` / `icon.ico` 32×32: ~940 of 1024 differ.
- The setup.exe icon's dominant palette is black + light gray + **teal/cyan**
  (`160,192,192`) with no warm accent; the app artwork is black + gray + a warm
  red-brown accent (`96,64,64`).
- Decisively: an unrelated app's installer on the same machine
  (`Downloads\CMR Quality Method_1.1.0_x64-setup.exe`) has an **identical** icon
  histogram (41 / 25 / 24 in the same buckets) — a shared template icon.

Conclusion: **the installer is carrying the Tauri/NSIS default icon, not the
Rebuffer artwork.** If the intent was the app icon, the post-build icon
replacement did not take effect for this build. The app's own binary
(`rebuffer.exe`) does carry the new artwork, so this is installer-only.

## 2. Inspecting the generated script — the hook is wired at every point

`src-tauri\target\release\nsis\x64\installer.nsi`:

- **Line 31**: `!include "C:\Rebuferization\src-tauri\installer\clipboard-history.nsh"`
  — the hook file is included before any page/section is declared.
- **Welcome page wiring** (the checkbox's home):
  - installer.nsi **lines 164–165**: `!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive`
    + `!insertmacro MUI_PAGE_WELCOME`.
  - clipboard-history.nsh **lines 38–39**: `!define MUI_PAGE_CUSTOMFUNCTION_SHOW
    RebufferWelcomeShow` and `!define MUI_PAGE_CUSTOMFUNCTION_LEAVE
    RebufferWelcomeLeave`. These one-shot defines are consumed by the **first** MUI
    page — the Welcome page — so the checkbox is created on show
    (hook line 61 creates the control with the exact label) and read on leave
    (hook line 68).
- **`NSIS_HOOK_POSTINSTALL`** — defined in the hook (lines 74–82), inserted in the
  generated script at **installer.nsi lines 704–706** (inside `Section Install`,
  after the files are copied).
- **`NSIS_HOOK_PREUNINSTALL`** — defined in the hook (lines 113–131), inserted at
  **installer.nsi lines 749–751** (inside `Section Uninstall`).
- `NSIS_HOOK_PREINSTALL` / `NSIS_HOOK_POSTUNINSTALL` insertion points also exist
  (lines 632–634, 837–839) but the hook does not define those macros, which is fine.

The compiled-script strings in §1 corroborate that these macros really expanded
(the label, keys and prompt are in the binary).

## 3. Silent-mode safety — a silent install changes nothing

Reading, not running:

- The checkbox is created only inside `RebufferWelcomeShow` (the Welcome page's
  show function, hook line 41–65). `/S` silent installs skip **all** MUI pages, and
  `/P` passive mode additionally aborts the Welcome page via
  `MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive` (installer.nsi line 164). In both
  modes the show function never runs, so `$RebufferDisableClipboard` stays unset.
- `NSIS_HOOK_POSTINSTALL` (hook lines 74–82) gates on
  `${If} $RebufferDisableClipboard = 1`. An unset NSIS variable is empty → coerces
  to 0 ≠ 1 → the **${Else}** branch runs, which only writes
  `ClipboardHistoryDisabledByRebuffer = 0` to the app's own key (so a later
  uninstaller never restores what this install didn't touch). It does **not** write
  to `Software\Microsoft\Clipboard`.
- Even the checkbox-enabled path (`RebufferApplyClipboardHistory`, hook lines
  84–108) only ever sets the value to 0 (off) and records the prior state — it
  never turns Win+V on.

**Conclusion: an unattended (`/S` or `/P`) deployment cannot change a user's
Win+V setting, and cannot mis-default it either — the branch is provably
else-skewed.** This is a reading-task and it passes.

## 4. Live install in a disposable environment — NOT verified

No disposable environment exists on this machine: Windows Sandbox is not present
and its feature is `Disabled`, Hyper-V is `Disabled` (enabling requires a reboot),
and no VMware/VirtualBox/qemu is installed. Per the brief I did **not** improvise
one on the live machine and did **not** run the installer against it, so:

**"not verified, no disposable environment available."** The registry-value write
on tick, and the uninstall restore path, remain unverified-by-execution. The static
evidence (§1–§3) confirms the strings, the wiring, and the silent-mode safety, but
a run against a throwaway VM/sandbox is still needed to prove the ticked checkbox
actually flips `EnableClipboardHistory` to 0 and that uninstall restores it only
when Rebuffer was the one who disabled it.

## What I decided not to do (safety)

- Did not run the setup, install to the real system, or modify
  `HKCU\Software\Microsoft\Clipboard` in any way — the user is actively using this
  machine.
- Did not enable Windows Sandbox or Hyper-V (needs a reboot) to create a test
  environment.

## Repro / artifacts

- `scripts/installer/decompress_nsis.py` — decompresses the installer's solid LZMA
  block and the embedded uninstaller, searches for the target strings.
- `scripts/installer/pe_icons.py` — PE icon-resource extraction (used to locate the
  embedded uninstaller's NSIS signature; the icon comparison itself was done via
  System.Drawing).
- `scripts/installer/installer_icon.png` — the 32×32 icon extracted from the setup
  exe (for the record).