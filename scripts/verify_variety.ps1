# Seeds fresh variety items (new ids 9501+) and verifies them in the popup:
# stops the app, seeds, restarts, waits for hotkey-ready, opens the popup,
# search-filters to each item, captures each result.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Vv {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint LD = 0x0002, LU = 0x0004;
    public static void AltV() { keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static bool IsFg(IntPtr h) { return GetForegroundWindow() == h; }
    public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(LD, 0, 0, 0, UIntPtr.Zero); mouse_event(LU, 0, 0, 0, UIntPtr.Zero); }
    public static void Type(string s) { foreach (char ch in s) { byte vk = (byte)char.ToUpperInvariant(ch); keybd_event(vk, 0, 0, UIntPtr.Zero); keybd_event(vk, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); } }
    public static void CtrlA() { keybd_event(0x11, 0, 0, UIntPtr.Zero); keybd_event(0x41, 0, 0, UIntPtr.Zero); keybd_event(0x41, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); keybd_event(0x11, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static void Backspace() { keybd_event(0x08, 0, 0, UIntPtr.Zero); keybd_event(0x08, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
}
'@

$storeRoot = Join-Path $env:APPDATA 'Rebuffer'
$db = Join-Path $storeRoot 'rebuffer.db'
$tool = Join-Path $PSScriptRoot 'seed-tool\target\release\seed-tool.exe'
$exe = Join-Path $PSScriptRoot '..\src-tauri\target\debug\rebuffer.exe'
$logFile = Join-Path $storeRoot 'logs\rebuffer.log.2026-08-30'

# ---- 1. stop app + seed --------------------------------------------------
Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 1000

$long = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. This paragraph is long enough that the seven-line preview panel must clamp it with an ellipsis."
$code = @'
{
  "schema": "rebuffer-seed",
  "version": 1,
  "items": [
    { "kind": "text", "note": "seed for the code sub-kind preview" },
    { "kind": "image", "note": "not actually stored, just structure" }
  ],
  "retentionDays": 30
}
'@
$link = 'https://github.com/anomalyco/opencode/issues'
$color = '#3D8BFD'
$plain = 'The quick brown fox jumps over the lazy dog while the sun sets over the harbour.'

$dataDir = Join-Path $PSScriptRoot 'seed-data'
$files = @{
    9501 = @{ kind = 'text'; sub = 'color'; ext = 'TXT'; body = $color }
    9502 = @{ kind = 'text'; sub = 'code'; ext = 'JSON'; body = $code }
    9503 = @{ kind = 'text'; sub = 'link'; ext = 'TXT'; body = $link }
    9504 = @{ kind = 'text'; sub = 'plain'; ext = 'TXT'; body = $long }
    9505 = @{ kind = 'text'; sub = 'plain'; ext = 'TXT'; body = $plain }
}
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$inserts = New-Object System.Collections.Generic.List[string]
foreach ($id in ($files.Keys | Sort-Object)) {
    $f = $files[$id]
    $path = Join-Path $dataDir "v$id.txt"
    [System.IO.File]::WriteAllText($path, $f.body, [System.Text.UTF8Encoding]::new($false))
    $hash = (& $tool 'text' $path).Trim()
    $rel = "$($hash.Substring(0,2))/$($hash.Substring(2,2))/$hash"
    $blobDir = Join-Path $storeRoot "blobs\$($hash.Substring(0,2))\$($hash.Substring(2,2))"
    New-Item -ItemType Directory -Path $blobDir -Force | Out-Null
    $blob = Join-Path $blobDir $hash
    if (-not (Test-Path $blob)) { [System.IO.File]::WriteAllBytes($blob, [System.IO.File]::ReadAllBytes($path)) }
    $preview = $f.body; if ($preview.Length -gt 200) { $preview = $preview.Substring(0, 200) }
    $preview = $preview.Replace("'", "''")
    $created = $now - (($id - 9500) * 60000)
    $inserts.Add(@"
INSERT INTO items (id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path, title, preview_text, ext, mime, byte_size, width, height, duration_ms, source_app, copy_count, created_at, first_seen_at, last_used_at, pinned)
VALUES ($id, '$($f.kind)', '$($f.sub)', '$hash', '$rel', NULL, 0, NULL, NULL, '$preview', '$($f.ext)', 'text/plain', $($f.body.Length), NULL, NULL, NULL, 'seed-tool', 1, $created, $created, NULL, 0)
ON CONFLICT(id) DO UPDATE SET hash='$hash', blob_path='$rel', preview_text='$preview', created_at=$created;
"@)
}
$sqlFile = Join-Path $PSScriptRoot 'seed_variety.sql.tmp'
[System.IO.File]::WriteAllText($sqlFile, ($inserts -join "`n"))
$null = & sqlite3 $db ".read `"$sqlFile`""
if ($LASTEXITCODE -ne 0) { throw "sqlite3 seed failed: $LASTEXITCODE" }
Write-Output "seeded variety items 9501-9505"

# ---- 2. start app and wait for hotkey-ready ------------------------------
Start-Process -FilePath $exe
Write-Output "app starting; waiting for hotkey-ready (can take a minute with a 10k-item store)..."
$ready = $false
for ($i = 0; $i -lt 90; $i++) {
    Start-Sleep -Seconds 2
    if (Test-Path $logFile) {
        $tail = Get-Content $logFile -Tail 6
        if ($tail -match 'hotkey Alt\+V registered') { $ready = $true; break }
    }
}
if (-not $ready) { throw 'app did not reach hotkey-ready in time' }
Write-Output 'app hotkey-ready'
Start-Sleep -Milliseconds 500

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)

# ---- 3. open popup --------------------------------------------------------
if (-not [Win32]::IsWindowVisible($hwnd)) { [Vv]::AltV() }
for ($i = 0; $i -lt 80 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
for ($i = 0; $i -lt 40 -and -not [Vv]::IsFg($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
Write-Output "popup open=$([Win32]::IsWindowVisible($hwnd)) fg=$([Vv]::IsFg($hwnd))"
Start-Sleep -Milliseconds 2500

$r = New-Object Win32+RECT
function Click-Search {
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    [Vv]::Click($r.Left + 150, $r.Top + 24)
    Start-Sleep -Milliseconds 500
    return [Vv]::IsFg($hwnd)
}
if (-not (Click-Search)) { throw 'search click did not hold focus' }

foreach ($q in @('3D8BFD', 'lorem', 'schema', 'github')) {
    if (-not [Vv]::IsFg($hwnd)) { Write-Output "SKIP $q"; continue }
    [Vv]::CtrlA(); [Vv]::Backspace()
    Start-Sleep -Milliseconds 250
    [Vv]::Type($q)
    Start-Sleep -Milliseconds 2500
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot ("var_{0}.png" -f $q)
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "captured $out"
}
if ([Vv]::IsFg($hwnd)) { [Vv]::CtrlA(); [Vv]::Backspace() }
Write-Output DONE