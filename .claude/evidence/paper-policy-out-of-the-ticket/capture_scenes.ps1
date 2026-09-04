# capture_scenes.ps1 - photograph the nine paper-ticket harness surfaces.
#
# One launch per scene, each with its own hooks, every store pointed at a
# scratch folder so a run never touches the trader's own workspace, journal
# or paper state. The tape is a recorded session played from a scratch copy
# of one day: a deterministic tape is what makes two runs comparable at all.
#
# Usage:
#   powershell -File capture_scenes.ps1 -Exe <path> -Label before -OutDir <dir>

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Label,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$ReplayDir = "",
    [int]$SettleSeconds = 25,
    [string]$Autostart = "paused",
    [string]$Speed = "",
    [int]$HoldSeconds = 0,
    [int]$ExpectWidth = 0,
    [int]$ExpectHeight = 0
)

Add-Type -AssemblyName System.Drawing

$sig = @'
using System;
using System.Runtime.InteropServices;
public class WinCap {
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
'@
if (-not ([System.Management.Automation.PSTypeName]'WinCap').Type) {
    Add-Type -TypeDefinition $sig
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
$scratch = Join-Path $OutDir "_scratch"
$logs = Join-Path $OutDir "_logs"
New-Item -ItemType Directory -Force $scratch | Out-Null
New-Item -ItemType Directory -Force $logs | Out-Null

# The nine surfaces the pixel criterion names. Each value is the extra env
# this scene needs, on top of the common block below.
$scenes = @(
    @{ name = "paper_orders";           env = @{ QUANTICK_PAPER_ORDERS = "2" } },
    @{ name = "paper_order_bracket";    env = @{ QUANTICK_PAPER_ORDERS = "2"; QUANTICK_PAPER_ORDER_BRACKET = "1" } },
    @{ name = "paper_order_hover";      env = @{ QUANTICK_PAPER_ORDERS = "2"; QUANTICK_PAPER_ORDER_HOVER = "1" } },
    @{ name = "paper_risk";             env = @{ QUANTICK_PAPER_RISK = "100:0.20:1:BRL" } },
    @{ name = "paper_demo";             env = @{ QUANTICK_PAPER_DEMO = "1" } },
    @{ name = "paper_strategy_editor";  env = @{ QUANTICK_PAPER_STRATEGY_EDITOR = "1" } },
    @{ name = "cmd_preview";            env = @{ QUANTICK_CMD_PREVIEW = "buy@0.5"; QUANTICK_PAPER_DEMO = "1" } },
    @{ name = "paper_ruler_ticks";      env = @{ QUANTICK_PAPER_RULER_TICKS = "10"; QUANTICK_PAPER_RISK = "100:0.20:1:BRL" } },
    @{ name = "toast_paper";            env = @{ QUANTICK_TOAST = "paper" } }
)

function Stop-Ours($proc) {
    if ($proc -and -not $proc.HasExited) {
        try { $proc.Kill() } catch {}
        try { $proc.WaitForExit(10000) | Out-Null } catch {}
    }
}

foreach ($scene in $scenes) {
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    $retry = $false
    $name = $scene.name
    $log = Join-Path $logs "$Label-$name.log"
    $png = Join-Path $OutDir "$Label-$name.png"

    # Every QUANTICK_* store at scratch, per scene, so no scene inherits the
    # last one's state and nothing lands in the trader's own folders.
    $sceneScratch = Join-Path $scratch $name
    New-Item -ItemType Directory -Force $sceneScratch | Out-Null

    # Clear every QUANTICK_* first: hooks leak between runs otherwise, and one
    # capture then shows two surfaces.
    Get-ChildItem Env: | Where-Object { $_.Name -like "QUANTICK_*" } |
        ForEach-Object { Remove-Item "Env:$($_.Name)" -ErrorAction SilentlyContinue }

    $env:RUST_LOG = "quantick=info"
    # A third of the chart is clipped without this on a scaled monitor.
    $env:__COMPAT_LAYER = "DPIUNAWARE"
    $env:QUANTICK_TRADES_DIR = Join-Path $sceneScratch "trades"
    $env:QUANTICK_PAPER_STATE = Join-Path $sceneScratch "paper-state.json"
    $env:QUANTICK_UI_STATE = Join-Path $sceneScratch "ui-state.json"
    $env:QUANTICK_INDICATORS_STATE = Join-Path $sceneScratch "indicators-state.json"
    $env:QUANTICK_INDICATORS_DIR = Join-Path $sceneScratch "indicators"
    $env:QUANTICK_LAYOUTS = Join-Path $sceneScratch "layouts.json"
    # The depth book is fed independently of the replay tape - it comes from
    # whichever venue the default feed dials - so a run with network paints a
    # book and one without says "no book", and the two are not comparable.
    # This config's only feed is a MetaTrader listener on a port nothing ever
    # connects to, and it names no bridge_command, so nothing is spawned and
    # no book ever arrives. The tape still comes from the replay recording.
    $env:QUANTICK_CONFIG = Join-Path $PSScriptRoot "offline.toml"
    if ($ReplayDir -ne "") {
        $env:QUANTICK_REPLAY_DIR = $ReplayDir
        $env:QUANTICK_REPLAY_AUTOSTART = $Autostart
        if ($Speed -ne "") { $env:QUANTICK_REPLAY_SPEED = $Speed }
        $env:QUANTICK_REPLAY_DAY_BEFORE = "0"
    }
    foreach ($k in $scene.env.Keys) { Set-Item "Env:$k" $scene.env[$k] }

    $proc = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $log `
        -RedirectStandardOutput "$log.out"
    Start-Sleep -Seconds 3

    # Wait for the window, then for a healthy frame: a capture taken while the
    # surface is occluded or idle comes back blank, and blank is an
    # environment state, not a render regression.
    $deadline = (Get-Date).AddSeconds($SettleSeconds)
    $healthy = $false
    while ((Get-Date) -lt $deadline) {
        $proc.Refresh()
        if ($proc.HasExited) { break }
        if (Test-Path $log) {
            $fps = Select-String -Path $log -Pattern 'fps=(\d+)' -AllMatches |
                ForEach-Object { $_.Matches } | ForEach-Object { [int]$_.Groups[1].Value }
            if ($fps -and ($fps | Measure-Object -Maximum).Maximum -ge 50) { $healthy = $true; break }
        }
        Start-Sleep -Milliseconds 800
    }

    if ($HoldSeconds -gt 0) { Start-Sleep -Seconds $HoldSeconds }

    $proc.Refresh()
    if ($proc.HasExited) {
        Write-Output "$Label/$name EXITED_EARLY code=$($proc.ExitCode)"
        continue
    }
    if ($proc.MainWindowHandle -eq 0) {
        Write-Output "$Label/$name NO_WINDOW"
        Stop-Ours $proc
        continue
    }

    $r = New-Object WinCap+RECT
    [WinCap]::GetWindowRect($proc.MainWindowHandle, [ref]$r) | Out-Null
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) {
        Write-Output "$Label/$name BAD_RECT ${w}x${h}"
        Stop-Ours $proc
        continue
    }

    if ($ExpectWidth -gt 0 -and ($w -ne $ExpectWidth -or $h -ne $ExpectHeight)) {
        Write-Output "$Label/$name WRONG_SIZE ${w}x${h} (wanted ${ExpectWidth}x${ExpectHeight}) - retrying"
        Stop-Ours $proc
        $retry = $true
        continue
    }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $ok = [WinCap]::PrintWindow($proc.MainWindowHandle, $hdc, 2)
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()

    Stop-Ours $proc
    $hash = (Get-FileHash $png -Algorithm SHA256).Hash
    Write-Output "$Label/$name ${w}x${h} healthy=$healthy printwindow=$ok $hash"
    break
  }
}
