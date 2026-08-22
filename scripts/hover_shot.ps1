Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W3 {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$proc_pid = (Get-Process emoticon-panel-egui -ErrorAction SilentlyContinue | Select-Object -First 1).Id
if (-not $proc_pid) { Write-Output "app not running"; exit 1 }
$hits = New-Object System.Collections.ArrayList
$cb = [W3+EnumProc]{
  param($h, $l)
  $wpid = 0
  [W3]::GetWindowThreadProcessId($h, [ref]$wpid) | Out-Null
  if ($wpid -eq $proc_pid -and [W3]::IsWindowVisible($h)) {
    $r = New-Object W3+RECT
    [W3]::GetWindowRect($h, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    if ($w -ge 280 -and $w -le 420 -and $hh -ge 380 -and $hh -le 520) {
      $null = $hits.Add([pscustomobject]@{ hwnd = $h; L = $r.L; T = $r.T; W = $w; HH = $hh })
    }
  }
  return $true
}
[W3]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$win = $hits | Sort-Object W -Descending | Select-Object -First 1
if (-not $win) { Write-Output "no window"; exit 1 }
[W3]::SetForegroundWindow($win.hwnd) | Out-Null
Start-Sleep -Milliseconds 300
# hover 第一格中心 (窗口客户区左上角约为 L+3, T+3; 第一格中心 ≈ +12+37, +5+40)
$cx = $win.L + 3 + 12 + 37
$cy = $win.T + 3 + 5 + 40
[W3]::SetCursorPos($cx, $cy) | Out-Null
Write-Output ("cursor moved to {0},{1}" -f $cx, $cy)
Start-Sleep -Milliseconds 2200
$bmp = New-Object System.Drawing.Bitmap($win.W, $win.HH)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($win.L, $win.T, 0, 0, $bmp.Size)
$bmp.Save("H:\VibeCoding\EmoticonPanel\scripts\shot_hover1.png")
Start-Sleep -Milliseconds 260
$bmp2 = New-Object System.Drawing.Bitmap($win.W, $win.HH)
$g2 = [System.Drawing.Graphics]::FromImage($bmp2)
$g2.CopyFromScreen($win.L, $win.T, 0, 0, $bmp2.Size)
$bmp2.Save("H:\VibeCoding\EmoticonPanel\scripts\shot_hover2.png")
Write-Output "2 shots saved (260ms apart)"