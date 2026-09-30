# Listet die Fenster des laufenden notch-Prozesses (Debug-Hilfe)
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public class W {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
  public struct RECT { public int L,T,R,B; }
  public static List<string> List(uint pid) {
    var o = new List<string>();
    EnumWindows((h, l) => { uint id; GetWindowThreadProcessId(h, out id); if (id == pid) {
      var sb = new StringBuilder(200); GetWindowTextW(h, sb, 200); RECT r; GetWindowRect(h, out r);
      o.Add(String.Format("{0} vis={1} '{2}' {3},{4}-{5},{6} ex=0x{7:X}", h, IsWindowVisible(h), sb, r.L, r.T, r.R, r.B, GetWindowLongPtrW(h, -20).ToInt64())); }
      return true; }, IntPtr.Zero);
    return o;
  }
}
"@
foreach ($p in Get-Process notch) { [W]::List([uint32]$p.Id) }
