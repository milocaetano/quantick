# Open the app on the real MetaTrader session and wait for the whole day to
# land before photographing it. This is the trader's own scenario, end to end.
$root = "C:\Users\User\AppData\Local\Temp\claude\C--src-quantick\ca25ac58-e89f-431b-844a-1274a09d8b7e\scratchpad\vqa"
$scratch = "C:\Users\User\AppData\Local\Temp\claude\C--src-quantick\ca25ac58-e89f-431b-844a-1274a09d8b7e\scratchpad"
$exe = "C:\quantick-agent-target\debug\quantick-app.exe"

Get-ChildItem Env: | Where-Object { $_.Name -like "QUANTICK_*" } |
    ForEach-Object { Remove-Item "Env:$($_.Name)" -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path "$root\stores3" | Out-Null
foreach ($pair in @(
    @("QUANTICK_UI_STATE", "ui-state.toml"), @("QUANTICK_LAYOUTS", "layouts.toml"),
    @("QUANTICK_INDICATORS_STATE", "indicators.toml"),
    @("QUANTICK_INDICATOR_PRESETS", "indicator-presets.toml"),
    @("QUANTICK_CHART_LAYERS", "layers.toml"),
    @("QUANTICK_DRAWING_PRESETS", "drawing-presets.toml"),
    @("QUANTICK_FOOTPRINT_SETTINGS", "footprint.toml"),
    @("QUANTICK_FOOTPRINT_PRESETS", "footprint-presets.toml"),
    @("QUANTICK_SYMBOLS", "symbols.toml"),
    @("QUANTICK_PAPER_STATE", "paper-state.toml"),
    @("QUANTICK_TRADES_DIR", "trades"))) {
    Set-Item -Path "Env:$($pair[0])" -Value "$root\stores3\$($pair[1])"
}
$env:RUST_LOG = "quantick=info"
$env:__COMPAT_LAYER = "DPIUNAWARE"
$env:QUANTICK_CONFIG = "$scratch\mt5-config.toml"
$env:QUANTICK_WINDOW_SIZE = "1600x1000"

$log = "$root\mt5-open.log"
Remove-Item $log -ErrorAction SilentlyContinue
$proc = Start-Process -FilePath $exe -PassThru -RedirectStandardError $log `
    -RedirectStandardOutput "$root\mt5-open.out"
Write-Output "pid $($proc.Id)"

# Wait for the opening block to be charted, not merely for a healthy frame.
$landed = $false
for ($i = 0; $i -lt 90; $i++) {
    Start-Sleep -Seconds 1
    if (Test-Path $log) {
        $ready = Select-String -Path $log -Pattern 'MT5_HISTORY_READY' -ErrorAction SilentlyContinue
        if ($ready) { $landed = $true }
        # Give the opening slices time to arrive behind it.
        $done = Select-String -Path $log -Pattern 'BRIDGE_OPENING_COMPLETE' -ErrorAction SilentlyContinue
        if ($landed -and $done) { Start-Sleep -Seconds 3; break }
    }
}
Write-Output "history landed: $landed"

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win2 {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr d, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@ -ErrorAction SilentlyContinue
$proc.Refresh()
$h = $proc.MainWindowHandle
if ($h -ne [IntPtr]::Zero) {
    $r = New-Object Win2+RECT
    [void][Win2]::GetWindowRect($h, [ref]$r)
    $bmp = New-Object System.Drawing.Bitmap(($r.R - $r.L), ($r.B - $r.T))
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc(); [void][Win2]::PrintWindow($h, $hdc, 2); $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save("$root\shots\mt5-session-open.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "captured"
}
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

Write-Output "--- what the bridge and the app said ---"
foreach ($pat in @('BRIDGE_BACKFILL_SESSION', 'BRIDGE_OPENING_SLICED', 'BRIDGE_TICK_FLOOR_IMPLAUSIBLE',
                   'MT5_HISTORY_READY', 'MT5_OPENING_PAGE_READY', 'BRIDGE_OPENING_COMPLETE')) {
    $hit = Select-String -Path $log -Pattern $pat | Select-Object -Last 1
    if ($hit) { Write-Output ($hit.Line.Substring(0, [Math]::Min(230, $hit.Line.Length))) }
}
Write-Output "--- opening pages seen ---"
(Select-String -Path $log -Pattern 'MT5_OPENING_PAGE_READY').Count
Write-Output "--- health under the load ---"
Select-String -Path $log -Pattern 'APP_HEALTH_SUMMARY' | Select-Object -Last 1 |
    ForEach-Object { if ($_.Line -match '(fps=\d+ frame_avg_ms=[\d.]+ frame_cpu_ms=[\d.]+ frame_worst_ms=[\d.]+)') { $Matches[1] } }
Select-String -Path $log -Pattern 'APP_HEALTH_SUMMARY' | Select-Object -Last 1 |
    ForEach-Object { if ($_.Line -match '(live_trades=\d+)') { $Matches[1] } }
Write-Output "--- slow frames ---"
(Select-String -Path $log -Pattern 'APP_SLOW_FRAMES').Count
