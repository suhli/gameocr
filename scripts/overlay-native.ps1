param(
  [ValidateSet('inspect','focus','click','background','stats')][string]$Action='inspect',
  [Parameter(Mandatory)][int]$OwnerPid,
  [string]$Hwnd='0', [int]$X=0, [int]$Y=0
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class OverlayNativeTest {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr data);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr data);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int count);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr hwnd,int index);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd,out Rect rect);
  [DllImport("user32.dll")] public static extern bool GetWindowDisplayAffinity(IntPtr hwnd,out uint affinity);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hwnd);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from,uint to,bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd,int action);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
  [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
  [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
  public static long[] Windows(int owner) {var list=new List<long>();EnumWindows((h,d)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==owner)list.Add(h.ToInt64());return true;},IntPtr.Zero);return list.ToArray();}
}
'@
if($Action -eq 'stats') {
  $processes=Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId
  $ids=[Collections.Generic.HashSet[int]]::new();$null=$ids.Add($OwnerPid)
  do { $changed=$false;foreach($entry in $processes){if($ids.Contains([int]$entry.ParentProcessId) -and $ids.Add([int]$entry.ProcessId)){$changed=$true}} }while($changed)
  $cpu=0.;$memory=0L
  foreach($id in $ids){$p=Get-Process -Id $id -ErrorAction SilentlyContinue;if($p){$cpu+=$p.CPU;$memory+=$p.WorkingSet64}}
  [pscustomobject]@{cpuSeconds=$cpu;workingSetBytes=$memory;processes=$ids.Count;timestamp=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()}|ConvertTo-Json -Compress
  exit
}
function Focus-TestWindow([IntPtr]$Handle) {
  $foreground=[OverlayNativeTest]::GetForegroundWindow();$unused=0u
  $thread=[OverlayNativeTest]::GetWindowThreadProcessId($foreground,[ref]$unused)
  $current=[OverlayNativeTest]::GetCurrentThreadId();$attached=$false
  try {
    if($thread -ne $current){$attached=[OverlayNativeTest]::AttachThreadInput($current,$thread,$true)}
    $null=[OverlayNativeTest]::ShowWindow($Handle,9)
    $null=[OverlayNativeTest]::BringWindowToTop($Handle)
    $null=[OverlayNativeTest]::SetForegroundWindow($Handle)
  } finally {if($attached){$null=[OverlayNativeTest]::AttachThreadInput($current,$thread,$false)}}
  if([OverlayNativeTest]::GetForegroundWindow() -ne $Handle){throw 'Test target could not become foreground; desktop interaction unavailable.'}
}
if($Action -ne 'inspect') {
  $handle=[IntPtr]::new([Convert]::ToInt64($Hwnd,16));$actualOwner=0u
  $null=[OverlayNativeTest]::GetWindowThreadProcessId($handle,[ref]$actualOwner)
  if($actualOwner -ne $OwnerPid){throw 'Refusing to operate a window outside the owned test process.'}
  if($Action -in @('focus','click','background')){Focus-TestWindow $handle}
  if($Action -eq 'click') {
    Start-Sleep -Milliseconds 200
    $original=[OverlayNativeTest+Point]::new();$null=[OverlayNativeTest]::GetCursorPos([ref]$original)
    try {$null=[OverlayNativeTest]::SetCursorPos($X,$Y);[OverlayNativeTest]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[OverlayNativeTest]::mouse_event(4,0,0,0,[UIntPtr]::Zero);[OverlayNativeTest]::mouse_event(0x800,0,0,120,[UIntPtr]::Zero)}
    finally {$null=[OverlayNativeTest]::SetCursorPos($original.x,$original.y)}
  }
}
$windows=foreach($value in [OverlayNativeTest]::Windows($OwnerPid)) {
  $handle=[IntPtr]::new($value);$title=[Text.StringBuilder]::new(1024);$null=[OverlayNativeTest]::GetWindowText($handle,$title,1024)
  if(!$title.Length){continue}
  $rect=[OverlayNativeTest+Rect]::new();$null=[OverlayNativeTest]::GetWindowRect($handle,[ref]$rect)
  $affinity=0u;$affinityOk=[OverlayNativeTest]::GetWindowDisplayAffinity($handle,[ref]$affinity)
  [pscustomobject]@{hwnd=$value.ToString('x');title=$title.ToString();visible=[OverlayNativeTest]::IsWindowVisible($handle);style=[OverlayNativeTest]::GetWindowLongPtrW($handle,-20).ToInt64();rect=@($rect.left,$rect.top,$rect.right,$rect.bottom);dpi=[OverlayNativeTest]::GetDpiForWindow($handle);affinity=$affinity;affinityOk=$affinityOk}
}
[pscustomobject]@{windows=@($windows);foreground=[OverlayNativeTest]::GetForegroundWindow().ToInt64().ToString('x')}|ConvertTo-Json -Depth 4 -Compress
