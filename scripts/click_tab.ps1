Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W5 {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$proc_pid = (Get-Process emoticon-panel-lite -ErrorAction SilentlyContinue | Select-Object -First 1).Id
if (-not $proc_pid) { Write-Output "app not running"; exit 1 }
$hits = New-Object System.Collections.ArrayList
$cb = [W5+EnumProc]{
  param($h, $l)
  $wpid = 0
  [W5]::GetWindowThreadProcessId($h, [ref]$wpid) | Out-Null
  if ($wpid -eq $proc_pid -and [W5]::IsWindowVisible($h)) {
    $r = New-Object W5+RECT
    [W5]::GetWindowRect($h, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    if ($w -ge 280 -and $w -le 420 -and $hh -ge 380 -and $hh -le 520) {
      $null = $hits.Add([pscustomobject]@{ L = $r.L; T = $r.T; R = $r.R; B = $r.B })
    }
  }
  return $true
}
[W5]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$win = $hits | Sort-Object { $_.R - $_.L } -Descending | Select-Object -First 1
if (-not $win) { Write-Output "no window"; exit 1 }
[W5]::SetForegroundWindow([IntPtr]::Zero) | Out-Null
$tab = [int]$args[0]
$cx = $win.L + 12 + 18 + $tab * 36
$cy = $win.B - 25
[W5]::SetCursorPos($cx, $cy) | Out-Null
Start-Sleep -Milliseconds 150
[W5]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 30
[W5]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
Write-Output ("clicked tab {0} at {1},{2} (win L={3} B={4})" -f $tab, $cx, $cy, $win.L, $win.B)