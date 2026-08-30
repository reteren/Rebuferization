# Win32 helpers: window find, screenshot (PrintWindow -> raw BMP via GDI),
# real input. No System.Drawing dependency. Runs under Windows PowerShell 5.1.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class RbfWin {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetWindowTextW(IntPtr hwnd, StringBuilder sb, int max);
  [DllImport("user32.dll")]
  public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")]
  public static extern bool ShowWindow(IntPtr hwnd, int cmd);
  [DllImport("user32.dll")]
  public static extern bool SetForegroundWindow(IntPtr hwnd);
  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
  [DllImport("user32.dll")]
  public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
  [DllImport("user32.dll")]
  public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")]
  public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")]
  public static extern IntPtr GetDC(IntPtr hwnd);
  [DllImport("user32.dll")]
  public static extern int ReleaseDC(IntPtr hwnd, IntPtr hdc);
  [DllImport("gdi32.dll")]
  public static extern IntPtr CreateCompatibleDC(IntPtr hdc);
  [DllImport("gdi32.dll")]
  public static extern IntPtr CreateCompatibleBitmap(IntPtr hdc, int w, int h);
  [DllImport("gdi32.dll")]
  public static extern IntPtr SelectObject(IntPtr hdc, IntPtr obj);
  [DllImport("gdi32.dll")]
  public static extern bool DeleteObject(IntPtr obj);
  [DllImport("gdi32.dll")]
  public static extern bool DeleteDC(IntPtr hdc);
  [DllImport("gdi32.dll")]
  public static extern int GetDIBits(IntPtr hdc, IntPtr bmp, uint start, uint lines, byte[] bits, ref BITMAPINFOHEADER bi, uint usage);

  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int L, T, R, B; }

  [StructLayout(LayoutKind.Sequential)]
  public struct BITMAPINFOHEADER {
    public uint biSize;
    public int biWidth;
    public int biHeight;
    public ushort biPlanes;
    public ushort biBitCount;
    public uint biCompression;
    public uint biSizeImage;
    public int biXPelsPerMeter;
    public int biYPelsPerMeter;
    public uint biClrUsed;
    public uint biClrImportant;
  }

  public static string Title(IntPtr hwnd) {
    StringBuilder sb = new StringBuilder(256);
    GetWindowTextW(hwnd, sb, 256);
    return sb.ToString();
  }

  public static bool ClickAt(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(80);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
    System.Threading.Thread.Sleep(60);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    return true;
  }

  /// Captures a window to a 32bpp BMP byte array (top-down rows, BGRX).
  public static byte[] CaptureWindowBmp(IntPtr hwnd) {
    RECT r;
    if (!GetWindowRect(hwnd, out r)) return null;
    int w = r.R - r.L, h = r.B - r.T;
    if (w <= 0 || h <= 0) return null;
    IntPtr wdc = GetDC(hwnd);
    if (wdc == IntPtr.Zero) return null;
    IntPtr mem = CreateCompatibleDC(wdc);
    IntPtr bmp = CreateCompatibleBitmap(wdc, w, h);
    IntPtr old = SelectObject(mem, bmp);
    try {
      PrintWindow(hwnd, mem, 2); // PW_RENDERFULLCONTENT
      BITMAPINFOHEADER bi = new BITMAPINFOHEADER();
      bi.biSize = 40;
      bi.biWidth = w;
      bi.biHeight = -h; // top-down
      bi.biPlanes = 1;
      bi.biBitCount = 32;
      bi.biCompression = 0; // BI_RGB
      byte[] bits = new byte[w * h * 4];
      GetDIBits(mem, bmp, 0, (uint)h, bits, ref bi, 0);
      // BMP container: file header (14) + info header (40) + pixels
      byte[] file = new byte[54 + bits.Length];
      file[0] = (byte)'B'; file[1] = (byte)'M';
      uint fileSize = (uint)file.Length;
      file[2] = (byte)fileSize; file[3] = (byte)(fileSize >> 8); file[4] = (byte)(fileSize >> 16); file[5] = (byte)(fileSize >> 24);
      file[10] = 54;
      byte[] biBytes = new byte[40];
      IntPtr biPtr = Marshal.AllocHGlobal(40);
      Marshal.StructureToPtr(bi, biPtr, false);
      Marshal.Copy(biPtr, biBytes, 0, 40);
      Marshal.FreeHGlobal(biPtr);
      Array.Copy(biBytes, 0, file, 14, 40);
      for (int i = 0; i < bits.Length; i += 4) { bits[i + 3] = 255; }
      Array.Copy(bits, 0, file, 54, bits.Length);
      return file;
    } finally {
      SelectObject(mem, old);
      DeleteObject(bmp);
      DeleteDC(mem);
      ReleaseDC(hwnd, wdc);
    }
  }
}
"@