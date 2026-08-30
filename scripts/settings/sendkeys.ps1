# Sends a REAL chord through the OS input queue (SendInput), e.g.:
#   powershell -File sendkeys.ps1 -Chord "Alt+V"
#   powershell -File sendkeys.ps1 -Chord "Ctrl+Alt+Shift+Z"
# RegisterHotKey and low-level hooks both observe SendInput keystrokes.
param(
  [string]$Chord = "Alt+V"
)

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class Keys {
  [StructLayout(LayoutKind.Sequential)]
  public struct INPUT {
    public uint type;
    public InputUnion U;
  }
  [StructLayout(LayoutKind.Explicit)]
  public struct InputUnion {
    [FieldOffset(0)] public KEYBDINPUT ki;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct KEYBDINPUT {
    public ushort wVk;
    public ushort wScan;
    public uint dwFlags;
    public uint time;
    public IntPtr dwExtraInfo;
  }
  [DllImport("user32.dll", SetLastError = true)]
  public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

  public static void Tap(ushort vk, bool[] mods) {
    // mods: [Ctrl, Alt, Shift, Win]
    ushort[] vks = { 0x11, 0x12, 0x10, 0x5B };
    for (int i = 0; i < 4; i++) {
      if (mods[i]) Press(vks[i], false);
    }
    Press(vk, false);
    Press(vk, true);
    for (int i = 3; i >= 0; i--) {
      if (mods[i]) Press(vks[i], true);
    }
  }

  private static void Press(ushort vk, bool up) {
    INPUT[] inp = new INPUT[1];
    inp[0].type = 1; // INPUT_KEYBOARD
    inp[0].U.ki.wVk = vk;
    inp[0].U.ki.wScan = 0;
    inp[0].U.ki.dwFlags = up ? 2u : 0u; // KEYEVENTF_KEYUP
    inp[0].U.ki.time = 0;
    inp[0].U.ki.dwExtraInfo = IntPtr.Zero;
    SendInput(1, inp, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@

$parts = $Chord -split '\+'
$mods = [bool[]]@($false, $false, $false, $false)  # Ctrl Alt Shift Win
$vk = 0
foreach ($p in $parts) {
  switch ($p.Trim().ToLower()) {
    'ctrl'  { $mods[0] = $true }
    'alt'   { $mods[1] = $true }
    'shift' { $mods[2] = $true }
    'win'   { $mods[3] = $true }
    default {
      if ($p -match '^[a-z]$') { $vk = [int][char]$p.ToUpper() }
      elseif ($p -match '^[0-9]$') { $vk = 0x30 + [int]$p }
      elseif ($p -match '^F([0-9]|1[0-9]|2[0-4])$') { $vk = 0x70 + [int]$matches[1] - 1 }
      else { throw "unsupported key: $p" }
    }
  }
}
if ($vk -eq 0) { throw "no key in chord" }
[Keys]::Tap([System.UInt16]$vk, $mods)
Write-Output "sent $Chord"