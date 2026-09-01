# Launch quantick with a scene's hooks, wait for health, capture by PID.
param(
    [Parameter(Mandatory = $true)][string]$Scene,
    [hashtable]$Hooks = @{},
    [int]$WaitSeconds = 14
)

$root = "C:\Users\User\AppData\Local\Temp\claude\C--src-quantick\ca25ac58-e89f-431b-844a-1274a09d8b7e\scratchpad\vqa"
$exe = "C:\quantick-agent-target\debug\quantick-app.exe"
New-Item -ItemType Directory -Force -Path "$root\shots" | Out-Null
New-Item -ItemType Directory -Force -Path "$root\stores" | Out-Null

# Every QUANTICK_* store at scratch, so a run never reads or rewrites the
# trader's real cockpit. Cleared first so hooks cannot leak between scenes.
Get-ChildItem Env: | Where-Object { $_.Name -like "QUANTICK_*" } |
    ForEach-Object { Remove-Item "Env:$($_.Name)" -ErrorAction SilentlyContinue }

$env:QUANTICK_UI_STATE = "$root\stores\ui-state.toml"
$env:QUANTICK_LAYOUTS = "$root\stores\layouts.toml"
$env:QUANTICK_INDICATORS_STATE = "$root\stores\indicators.toml"
$env:QUANTICK_INDICATOR_PRESETS = "$root\stores\indicator-presets.toml"
$env:QUANTICK_CHART_LAYERS = "$root\stores\layers.toml"
$env:QUANTICK_DRAWING_PRESETS = "$root\stores\drawing-presets.toml"
$env:QUANTICK_FOOTPRINT_SETTINGS = "$root\stores\footprint.toml"
$env:QUANTICK_FOOTPRINT_PRESETS = "$root\stores\footprint-presets.toml"
$env:QUANTICK_SYMBOLS = "$root\stores\symbols.toml"
$env:QUANTICK_PAPER_STATE = "$root\stores\paper-state.toml"
$env:QUANTICK_TRADES_DIR = "$root\stores\trades"
$env:RUST_LOG = "quantick=info"
# Per the capture memory: without this a third of the chart is clipped.
$env:__COMPAT_LAYER = "DPIUNAWARE"

foreach ($k in $Hooks.Keys) { Set-Item -Path "Env:$k" -Value $Hooks[$k] }

$log = "$root\$Scene.log"
$proc = Start-Process -FilePath $exe -PassThru -RedirectStandardError $log `
    -RedirectStandardOutput "$root\$Scene.out"
Write-Output "pid $($proc.Id) scene $Scene"

# Gate on health: fps >= 50 means the surface really presents.
$ok = $false
for ($i = 0; $i -lt $WaitSeconds; $i++) {
    Start-Sleep -Seconds 1
    if (Test-Path $log) {
        $health = Select-String -Path $log -Pattern 'APP_HEALTH_SUMMARY' -ErrorAction SilentlyContinue |
            Select-Object -Last 1
        if ($health -and $health.Line -match 'fps[=:]?(\d+(\.\d+)?)') {
            if ([double]$Matches[1] -ge 50) { $ok = $true; break }
        }
    }
}
Write-Output "health-ok: $ok"

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr d, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@ -ErrorAction SilentlyContinue

$proc.Refresh()
$h = $proc.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { Write-Output "no window"; exit 1 }
$r = New-Object Win+RECT
[void][Win]::GetWindowRect($h, [ref]$r)
$w = $r.R - $r.L; $ht = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[void][Win]::PrintWindow($h, $hdc, 2)   # PW_RENDERFULLCONTENT
$g.ReleaseHdc($hdc); $g.Dispose()
$out = "$root\shots\$Scene.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "saved $out ($w x $ht)"

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
Select-String -Path $log -Pattern 'APP_HEALTH_SUMMARY' | Select-Object -Last 1 |
    ForEach-Object { Write-Output $_.Line }
