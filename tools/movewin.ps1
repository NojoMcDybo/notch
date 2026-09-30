# Test-Hilfe: sichtbares Fenster eines Prozesses verschieben.  .\movewin.ps1 -Name Haze-Widget-Lab-Glass -Title Lab -X 0 -Y 200
param([string]$Name, [string]$Title = "", [int]$X, [int]$Y)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class MW { [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f); }
"@
foreach ($p in Get-Process $Name -ErrorAction SilentlyContinue) {
  if ($p.MainWindowHandle -ne 0 -and $p.MainWindowTitle -like "*$Title*") {
    [MW]::SetWindowPos($p.MainWindowHandle, [IntPtr]::Zero, $X, $Y, 0, 0, 0x0001 -bor 0x0004) | Out-Null
    "verschoben: $($p.MainWindowTitle)"
  }
}
