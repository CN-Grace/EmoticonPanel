Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class FE2 {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public struct R { public int L,T,Rt,B; }
}
"@
function Find-Win([string]$procName) {
  $proc_pid = (Get-Process $procName -ErrorAction SilentlyContinue | Select-Object -First 1).Id
  if (-not $proc_pid) { return $null }
  $hits = New-Object System.Collections.ArrayList
  $cb = [FE2+EnumProc]{
    param($h, $l)
    $wpid = 0
    [FE2]::GetWindowThreadProcessId($h, [ref]$wpid) | Out-Null
    if ($wpid -eq $script:curPid -and [FE2]::IsWindowVisible($h)) {
      $r = New-Object FE2+R
      [FE2]::GetWindowRect($h, [ref]$r) | Out-Null
      $w = $r.Rt - $r.L; $hh = $r.B - $r.T
      if ($script:curName -eq 'emoticon-panel-lite' -and $w -ge 280 -and $w -le 420 -and $hh -ge 380 -and $hh -le 520) {
        $null = $hits.Add([pscustomobject]@{ H = $h; L = $r.L; T = $r.T; Rt = $r.Rt; B = $r.B })
      } elseif ($script:curName -eq 'mspaint' -and $w -ge 150 -and $hh -ge 80) {
        $null = $hits.Add([pscustomobject]@{ H = $h; L = $r.L; T = $r.T; Rt = $r.Rt; B = $r.B })
      }
    }
    return $true
  }
  $script:curPid = $proc_pid; $script:curName = $procName
  [FE2]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
  if ($hits.Count -gt 0) { return $hits | Sort-Object { $_.Rt - $_.L } -Descending | Select-Object -First 1 }
  return $null
}
function ClickXY($x, $y) {
  [FE2]::SetCursorPos($x, $y) | Out-Null
  Start-Sleep -Milliseconds 120
  [FE2]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30
  [FE2]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 80
}
function GetR($w) {
  $r = New-Object FE2+R; [FE2]::GetWindowRect($w.H, [ref]$r) | Out-Null
  return [pscustomobject]@{ H = $w.H; L = $r.L; T = $r.T; Rt = $r.Rt; B = $r.B }
}

Start-Process mspaint -ErrorAction SilentlyContinue | Out-Null
Start-Sleep -Milliseconds 1500
$app = Find-Win 'emoticon-panel-lite'
if (-not $app) { Write-Output "FAIL app window not found"; exit 1 }
$pt = Find-Win 'mspaint'
if (-not $pt) { Write-Output "FAIL mspaint window not found"; exit 1 }
Write-Output ("app=({0},{1})-({2},{3})  mspaint=({4},{5})-({6},{7})" -f $app.L, $app.T, $app.Rt, $app.B, $pt.L, $pt.T, $pt.Rt, $pt.B)

# attach: ⚙(右下) -> 选择(面板首行右) -> 点 mspaint
ClickXY ($app.Rt - 22) ($app.B - 23)
Start-Sleep -Milliseconds 500
$app = GetR $app
$selX = $app.L + 272   # 校准: 设置面板首行(目标窗口)绿色按钮中心
$selY = $app.T + 132
ClickXY $selX $selY
Start-Sleep -Milliseconds 500
$pt = Find-Win 'mspaint'
ClickXY (($pt.L + $pt.Rt) / 2) (($pt.T + $pt.B) / 2)
Start-Sleep -Milliseconds 1500

$appA = Find-Win 'emoticon-panel-lite'
$ptA = Find-Win 'mspaint'
if (-not $appA -or -not $ptA) { Write-Output "FAIL after attach"; exit 1 }
$dx = $appA.L - $ptA.Rt
Write-Output ("attach后  mspaint.right={0} app.left={1}  dx={2}" -f $ptA.Rt, $appA.L, $dx)
$ok1 = [Math]::Abs($dx - 8) -le 45

[FE2]::MoveWindow($ptA.H, $ptA.L + 140, $ptA.T + 70, ($ptA.Rt - $ptA.L), ($ptA.B - $ptA.T), $true) | Out-Null
Start-Sleep -Milliseconds 1000
$appB = Find-Win 'emoticon-panel-lite'
$ptB = Find-Win 'mspaint'
$dxB = $appB.L - $ptB.Rt
Write-Output ("移动后    mspaint.left={0} app.left={1}  dx={2}" -f $ptB.L, $appB.L, $dxB)
$ok2 = [Math]::Abs($dxB - 8) -le 45 -and [Math]::Abs(($appB.L - $appA.L) - 140) -le 35

[FE2]::ShowWindow($ptB.H, 0) | Out-Null
Start-Sleep -Milliseconds 1000
$appC = Find-Win 'emoticon-panel-lite'
$appVis = if ($appC) { [FE2]::IsWindowVisible($appC.H) } else { $false }
Write-Output ("隐藏后    app 可见 = $appVis  (期望 False)")
$ok3 = -not $appVis

[FE2]::ShowWindow($ptB.H, 9) | Out-Null
Start-Sleep -Milliseconds 1200
$appD = Find-Win 'emoticon-panel-lite'
$ptD = Find-Win 'mspaint'
$ok4 = $false
if ($appD -and $ptD) {
  $dxD = $appD.L - $ptD.Rt
  Write-Output ("恢复后    mspaint.right={0} app.left={1}  dx={2} visible={3}" -f $ptD.Rt, $appD.L, $dxD, [FE2]::IsWindowVisible($appD.H))
  $ok4 = [FE2]::IsWindowVisible($appD.H) -and [Math]::Abs($dxD - 8) -le 45
}
Write-Output ("RESULT: attach={0} move={1} hide={2} restore={3}  =>  {4}" -f $ok1, $ok2, $ok3, $ok4, ($(if ($ok1 -and $ok2 -and $ok3 -and $ok4) { "ALL PASS" } else { "FAIL" })))