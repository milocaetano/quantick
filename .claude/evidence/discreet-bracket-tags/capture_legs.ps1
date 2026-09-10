# capture_legs.ps1 - photograph the bracket-leg tags (SL/TP) on a live chart.
# Every store at scratch; one launch per scene; capture by PID.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Label,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Only = ""
)
Add-Type -AssemblyName System.Drawing
$sig = @'
using System;
using System.Runtime.InteropServices;
public class WinCap2 {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
'@
if (-not ([System.Management.Automation.PSTypeName]'WinCap2').Type) { Add-Type -TypeDefinition $sig }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$scratch = Join-Path $OutDir "_scratch"; $logs = Join-Path $OutDir "_logs"
New-Item -ItemType Directory -Force $scratch, $logs | Out-Null

# hold = seconds after a healthy frame before the shutter; the position scene
# waits for the live tape to fill one of the two resting orders.
$scenes = @(
    @{ name = "order_bracket";       hold = 8;  env = @{ QUANTICK_PAPER_ORDERS = "1"; QUANTICK_PAPER_ORDER_BRACKET = "1" } },
    @{ name = "order_bracket_hover"; hold = 8;  env = @{ QUANTICK_PAPER_ORDERS = "1"; QUANTICK_PAPER_ORDER_BRACKET = "1"; QUANTICK_PAPER_ORDER_HOVER = "1" } },
    @{ name = "position_demo";       hold = 12; env = @{ QUANTICK_PAPER_DEMO = "1" } }
)
foreach ($scene in $scenes) {
    $name = $scene.name
    if ($Only -ne "" -and $name -ne $Only) { continue }
    $log = Join-Path $logs "$Label-$name.log"; $png = Join-Path $OutDir "$Label-$name.png"
    $s = Join-Path $scratch "$Label-$name"; New-Item -ItemType Directory -Force $s | Out-Null
    Get-ChildItem Env: | Where-Object { $_.Name -like "QUANTICK_*" } | ForEach-Object { Remove-Item "Env:$($_.Name)" -ErrorAction SilentlyContinue }
    $env:RUST_LOG = "quantick=info"
    $env:__COMPAT_LAYER = "DPIUNAWARE"
    $env:QUANTICK_TRADES_DIR = Join-Path $s "trades"
    $env:QUANTICK_PAPER_STATE = Join-Path $s "paper-state.toml"
    $env:QUANTICK_UI_STATE = Join-Path $s "ui-state.toml"
    $env:QUANTICK_INDICATORS_STATE = Join-Path $s "indicators-state.toml"
    $env:QUANTICK_INDICATORS_DIR = Join-Path $s "indicators"
    $env:QUANTICK_LAYOUTS = Join-Path $s "layouts.toml"
    # Deterministic tape: a synthetic replay day, offline feed (no book).
    $env:QUANTICK_CONFIG = Join-Path $PSScriptRoot "offline.toml"
    $env:QUANTICK_REPLAY_DIR = Join-Path $PSScriptRoot "replay"
    $env:QUANTICK_REPLAY_AUTOSTART = "1"
    $env:QUANTICK_REPLAY_DAY_BEFORE = "0"
    foreach ($k in $scene.env.Keys) { Set-Item "Env:$k" $scene.env[$k] }
    $proc = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $log -RedirectStandardOutput "$log.out"
    Start-Sleep -Seconds 3
    $deadline = (Get-Date).AddSeconds(40); $healthy = $false
    while ((Get-Date) -lt $deadline) {
        $proc.Refresh(); if ($proc.HasExited) { break }
        if (Test-Path $log) {
            $fps = Select-String -Path $log -Pattern 'fps=(\d+)' -AllMatches | ForEach-Object { $_.Matches } | ForEach-Object { [int]$_.Groups[1].Value }
            if ($fps -and ($fps | Measure-Object -Maximum).Maximum -ge 50) { $healthy = $true; break }
        }
        Start-Sleep -Milliseconds 800
    }
    Start-Sleep -Seconds $scene.hold
    $proc.Refresh()
    if ($proc.HasExited -or $proc.MainWindowHandle -eq 0) { Write-Output "$Label/$name NO_WINDOW"; continue }
    $r = New-Object WinCap2+RECT
    [WinCap2]::GetWindowRect($proc.MainWindowHandle, [ref]$r) | Out-Null
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
    $ok = [WinCap2]::PrintWindow($proc.MainWindowHandle, $hdc, 2)
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    try { $proc.Kill(); $proc.WaitForExit(10000) | Out-Null } catch {}
    Write-Output "$Label/$name ${w}x${h} healthy=$healthy printwindow=$ok"
}
