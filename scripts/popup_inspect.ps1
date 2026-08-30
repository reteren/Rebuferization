# Full popup inspection: opens the popup, then measures rendering, text
# overflow, thumbnails, zoom, arrow focus movement and Enter-copy via a mix
# of CDP evaluation and physical input injection.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Import-Module (Join-Path $PSScriptRoot 'cdp.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Insp {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint MOUSEEVENTF_WHEEL = 0x0800;
    public static void AltV() {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
        keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
    public static void Key(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); keybd_event(vk, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static void CtrlWheel(int delta) {
        keybd_event(0x11, 0, 0, UIntPtr.Zero);
        mouse_event(MOUSEEVENTF_WHEEL, 0, 0, (uint)delta, UIntPtr.Zero);
        keybd_event(0x11, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }

function Wait-Vis([int]$ms) { for ($i = 0; $i -lt $ms / 50; $i++) { if ([Win32]::IsWindowVisible($hwnd)) { return $true }; Start-Sleep -Milliseconds 50 }; return [Win32]::IsWindowVisible($hwnd) }
function Wait-Hid([int]$ms) { for ($i = 0; $i -lt $ms / 50; $i++) { if (-not [Win32]::IsWindowVisible($hwnd)) { return $true }; Start-Sleep -Milliseconds 50 }; return -not [Win32]::IsWindowVisible($hwnd) }

# --- open ---------------------------------------------------------------
if (-not [Win32]::IsWindowVisible($hwnd)) { [Insp]::AltV() }
$opened = Wait-Vis 4000
Write-Output "opened=$opened"
if (-not $opened) { throw 'popup did not open' }
Start-Sleep -Milliseconds 1800

$c = New-Cdp 'index.html'
Write-Output 'cdp connected'

# --- 1. rendering inventory ---------------------------------------------
$inv = $c.Eval(@'
(() => {
  const out = {};
  out.tabs = [...document.querySelectorAll('.tabs button, [role=tab]')].map(b => b.textContent.trim());
  out.groups = [...document.querySelectorAll('.group-header')].map(g => g.textContent.trim());
  out.cards = document.querySelectorAll('.card').length;
  out.cardKinds = [...document.querySelectorAll('.card')].map(card => {
    const id = card.getAttribute('data-id');
    const hasImg = !!card.querySelector('img.thumb');
    const hasText = !!card.querySelector('.text-panel');
    const hasLink = !!card.querySelector('.link-preview');
    const hasColor = !!card.querySelector('.color-preview');
    const hasFile = !!card.querySelector('.file-preview');
    return { id, hasImg, hasText, hasLink, hasColor, hasFile,
             age: card.querySelector('.badge.age')?.textContent.trim() ?? null,
             fmt: card.querySelector('.badge.fmt')?.textContent.trim() ?? null };
  });
  const status = document.querySelector('.statusbar, .status-bar, footer .status');
  out.statusBar = status ? status.textContent.trim() : null;
  const dial = document.querySelector('.zoom-dial, [aria-label*="zoom" i], [aria-label*="Zoom" i]');
  out.zoomDial = dial ? (dial.getAttribute('aria-valuenow') ?? dial.textContent.trim()) : null;
  out.emptyState = !!document.querySelector('.empty');
  return JSON.stringify(out);
})()
'@)
Write-Output "INVENTORY: $inv"

# --- 2. text overflow metrics -------------------------------------------
$ovf = $c.Eval(@'
(() => {
  const rows = [];
  for (const panel of document.querySelectorAll('.text-panel')) {
    const card = panel.closest('.card');
    const cs = getComputedStyle(panel);
    const cardCs = getComputedStyle(card);
    const rect = panel.getBoundingClientRect();
    rows.push({
      id: card.getAttribute('data-id'),
      scrollW: panel.scrollWidth, clientW: panel.clientWidth,
      scrollH: panel.scrollHeight, clientH: panel.clientHeight,
      hOverflow: panel.scrollWidth - panel.clientWidth,
      vOverflow: panel.scrollHeight - panel.clientHeight,
      clip: cs.webkitLineClamp || cs.lineClamp,
      panelOverflow: cs.overflow,
      cardOverflow: cardCs.overflow,
      fs: cs.fontSize, lh: cs.lineHeight,
      rectW: Math.round(rect.width), rectH: Math.round(rect.height)
    });
  }
  return JSON.stringify(rows);
})()
'@)
Write-Output "OVERFLOW: $ovf"

# --- 3. thumbnails --------------------------------------------------------
$th = $c.Eval(@'
(() => {
  const rows = [];
  for (const img of document.querySelectorAll('img.thumb')) {
    const card = img.closest('.card');
    rows.push({
      id: card.getAttribute('data-id'),
      src: img.currentSrc.slice(0, 90),
      naturalW: img.naturalWidth, naturalH: img.naturalHeight,
      complete: img.complete,
      clientW: Math.round(img.clientWidth), clientH: Math.round(img.clientHeight)
    });
  }
  return JSON.stringify(rows);
})()
'@)
Write-Output "THUMBS: $th"

# --- 4. zoom changes tile size --------------------------------------------
$w1 = $c.Eval('JSON.stringify([...document.querySelectorAll(".card")].map(c => c.getBoundingClientRect().width))')
$r = Get-Rect 2>$null
$rect = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$rect)
$cx = [int](($rect.Left + $rect.Right) / 2)
$cy = [int]($rect.Top + ($rect.Bottom - $rect.Top) * 0.4)
[Insp]::SetCursorPos($cx, $cy)
Start-Sleep -Milliseconds 200
[Insp]::CtrlWheel(-120); [Insp]::CtrlWheel(-120); [Insp]::CtrlWheel(-120)
Start-Sleep -Milliseconds 900
$w2 = $c.Eval('JSON.stringify([...document.querySelectorAll(".card")].map(c => c.getBoundingClientRect().width))')
$z = $c.Eval('JSON.stringify({dial: document.querySelector("[aria-label*=Zoom i]")?.getAttribute("aria-valuenow") ?? null, first: document.querySelector(".card")?.getBoundingClientRect().width ?? null})')
Write-Output "ZOOM before: $w1"
Write-Output "ZOOM after:  $w2"
Write-Output "ZOOM dial:   $z"

# --- 5. arrows move focus ---------------------------------------------------
[Insp]::Key(0x27); [Insp]::Key(0x27); [Insp]::Key(0x27)   # Right Right Right
Start-Sleep -Milliseconds 500
$focus = $c.Eval(@'
(() => {
  const f = document.querySelector('.card.focused');
  if (!f) return JSON.stringify({ focused: false });
  const box = f.getBoundingClientRect();
  return JSON.stringify({ focused: true, id: f.getAttribute('data-id'),
    x: Math.round(box.x), y: Math.round(box.y), w: Math.round(box.width),
    fmt: f.querySelector('.badge.fmt')?.textContent.trim() });
})()
'@)
Write-Output "FOCUS: $focus"

# --- 6. Enter copies and closes ---------------------------------------------
$before = (Get-Clipboard -Raw -ErrorAction SilentlyContinue) ?? ''
[Insp]::Key(0x0D)   # Enter
$closed = Wait-Hid 3000
Start-Sleep -Milliseconds 300
$after = (Get-Clipboard -Raw -ErrorAction SilentlyContinue) ?? ''
Write-Output "ENTER: popupClosed=$closed copied=$($after.Length) chars"
Write-Output ("ENTER: before='{0}' after='{1}'" -f $before.Substring(0, [Math]::Min(60, $before.Length)), $after.Substring(0, [Math]::Min(60, $after.Length)))

$c.Dispose()