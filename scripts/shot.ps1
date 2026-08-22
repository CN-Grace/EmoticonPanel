Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class W2 {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$proc_pid = (Get-Process emoticon-panel-egui -ErrorAction SilentlyContinue | Select-Object -First 1).Id
if (-not $proc_pid) { Write-Output "app not running"; exit 1 }
$hits = New-Object System.Collections.ArrayList
$cb = [W2+EnumProc]{
  param($h, $l)
  $wpid = 0
  [W2]::GetWindowThreadProcessId($h, [ref]$wpid) | Out-Null
  if ($wpid -eq $proc_pid -and [W2]::IsWindowVisible($h)) {
    $r = New-Object W2+RECT
    [W2]::GetWindowRect($h, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    if ($w -ge 280 -and $w -le 420 -and $hh -ge 380 -and $hh -le 520) {
      $null = $hits.Add([pscustomobject]@{ hwnd = $h; L = $r.L; T = $r.T; W = $w; HH = $hh })
    }
  }
  return $true
}
[W2]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
if ($hits.Count -eq 0) { Write-Output "no window found"; exit 1 }
$win = $hits | Sort-Object W -Descending | Select-Object -First 1
Write-Output ("window rect: {0},{1} {2}x{3}" -f $win.L, $win.T, $win.W, $win.HH)
[W2]::ShowWindow($win.hwnd, 9) | Out-Null
[W2]::SetForegroundWindow($win.hwnd) | Out-Null
Start-Sleep -Milliseconds 800
$bmp = New-Object System.Drawing.Bitmap($win.W, $win.HH)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($win.L, $win.T, 0, 0, $bmp.Size)
$bmp.Save("H:\VibeCoding\EmoticonPanel\scripts\shot.png")
Write-Output "saved shot.png ($($win.W)x$($win.H))"