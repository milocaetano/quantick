param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Label,
    # Scratch directory for logs and the per-run store; never the trader's.
    [Parameter(Mandatory = $true)][string]$Root,
    [string]$ReplayDir = (Join-Path $HOME "Documents\Quantick\replay"),
    [string]$Session = "2026-08-25",
    [int]$Seconds = 45
)
# One dense-replay run of a quantick-app build for the live envelope's frame
# timing (docs/quality/live-envelope.md). Every store is pointed at a scratch
# directory, so the trader's workspace is never read or written; the run is
# killed, not closed, so nothing is saved on exit either. Logs land in
# $Root\$Label.err.log; tools/live_envelope/frame_timing.py reads them.
New-Item -ItemType Directory -Force $Root | Out-Null
$store = Join-Path $Root "store-$Label"
if (Test-Path $store) { Remove-Item -Recurse -Force $store -Confirm:$false }
New-Item -ItemType Directory -Force $store | Out-Null
Copy-Item (Join-Path $PSScriptRoot "..\..\config\bubbles.toml") (Join-Path $store "bubbles.toml")

# Clear every QUANTICK_* inherited from the shell first.
Get-ChildItem Env: | Where-Object { $_.Name -like "QUANTICK_*" } | ForEach-Object { Remove-Item "Env:$($_.Name)" }

$env:QUANTICK_UI_STATE = Join-Path $store "ui-state.toml"
$env:QUANTICK_CHART_LAYERS = Join-Path $store "chart-layers.toml"
$env:QUANTICK_SYMBOLS = Join-Path $store "symbols.toml"
$env:QUANTICK_TRADES_DIR = Join-Path $store "trades"
$env:QUANTICK_LAYOUTS = Join-Path $store "layouts"
$env:QUANTICK_PANE_LAYOUTS = Join-Path $store "pane-layouts.toml"
$env:QUANTICK_INDICATORS_STATE = Join-Path $store "indicators.toml"
$env:QUANTICK_INDICATORS_DIR = Join-Path $store "indicators"
$env:QUANTICK_INDICATOR_PRESETS = Join-Path $store "indicator-presets.toml"
$env:QUANTICK_STRATEGY_PRESETS = Join-Path $store "strategy-presets.toml"
$env:QUANTICK_DRAWING_PRESETS = Join-Path $store "drawing-presets.toml"
$env:QUANTICK_FOOTPRINT_PRESETS = Join-Path $store "footprint-presets.toml"
$env:QUANTICK_PAPER_STATE = Join-Path $store "paper.toml"
$env:QUANTICK_TOOL_FAVORITES = Join-Path $store "tool-favorites.toml"
$env:QUANTICK_BUBBLES = Join-Path $store "bubbles.toml"

$env:QUANTICK_REPLAY_DIR = $ReplayDir
$env:QUANTICK_REPLAY_AUTOSTART = "1"
$env:QUANTICK_REPLAY_SESSION = $Session
$env:QUANTICK_REPLAY_SPEED = "60"
$env:QUANTICK_REPLAY_DAY_BEFORE = "0"
$env:QUANTICK_BOOK_AUTOSTART = "1"
$env:QUANTICK_BUBBLES_AUTOSTART = "1"
$env:QUANTICK_FOOTPRINT_AUTOSTART = "1"
$env:QUANTICK_LIVE_STRIP_AUTOSTART = "1"
$env:QUANTICK_WINDOW_SIZE = "1500x900"
$env:QUANTICK_LOG_FORMAT = "json"
$env:__COMPAT_LAYER = "DPIUNAWARE"
$env:RUST_LOG = "quantick=info"

$out = Join-Path $Root "$Label.out.log"
$err = Join-Path $Root "$Label.err.log"
$proc = Start-Process -FilePath $Exe -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
Start-Sleep -Seconds $Seconds
$ws = (Get-Process -Id $proc.Id -ErrorAction SilentlyContinue).WorkingSet64
Stop-Process -Id $proc.Id -Force -Confirm:$false
Start-Sleep -Seconds 2
"$Label pid=$($proc.Id) working_set_at_end=$ws"
