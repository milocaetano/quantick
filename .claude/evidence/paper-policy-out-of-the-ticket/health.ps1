# health.ps1 - APP_HEALTH_SUMMARY under a dense tape, one build.
#
# The seam added a field hop on the per-trade path and a per-call AccountEnv
# on the sizing path. Neither should be measurable, and this is what says so
# rather than asserting it.

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Label,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [Parameter(Mandatory = $true)][string]$ReplayDir,
    [int]$HoldSeconds = 70
)

New-Item -ItemType Directory -Force $OutDir | Out-Null
$scratch = Join-Path $OutDir "_s\$Label"
New-Item -ItemType Directory -Force $scratch | Out-Null
$log = Join-Path $OutDir "$Label.log"

Get-ChildItem Env: | Where-Object { $_.Name -like "QUANTICK_*" } |
    ForEach-Object { Remove-Item "Env:$($_.Name)" -ErrorAction SilentlyContinue }

$env:RUST_LOG = "quantick=info"
$env:__COMPAT_LAYER = "DPIUNAWARE"
$env:QUANTICK_TRADES_DIR = Join-Path $scratch "trades"
$env:QUANTICK_PAPER_STATE = Join-Path $scratch "paper-state.json"
$env:QUANTICK_UI_STATE = Join-Path $scratch "ui-state.json"
$env:QUANTICK_INDICATORS_STATE = Join-Path $scratch "ind.json"
$env:QUANTICK_INDICATORS_DIR = Join-Path $scratch "ind"
$env:QUANTICK_LAYOUTS = Join-Path $scratch "layouts.json"
# A dense tape: the whole recorded day at speed, with the paper demo trading
# through it so the per-trade path this branch touched is actually exercised.
$env:QUANTICK_REPLAY_DIR = $ReplayDir
$env:QUANTICK_REPLAY_AUTOSTART = "1"
$env:QUANTICK_REPLAY_DAY_BEFORE = "0"
$env:QUANTICK_REPLAY_SPEED = "1000"
$env:QUANTICK_PAPER_DEMO = "1"

$proc = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $log -RedirectStandardOutput "$log.out"
Start-Sleep -Seconds $HoldSeconds
try { $proc.Kill(); $proc.WaitForExit(10000) | Out-Null } catch {}

$lines = Select-String -Path $log -Pattern 'APP_HEALTH_SUMMARY' | ForEach-Object { $_.Line }
Write-Output "$Label : $($lines.Count) health lines"
$fps = @(); $avg = @()
foreach ($l in $lines) {
    if ($l -match 'fps=([0-9.]+)') { $fps += [double]$Matches[1] }
    if ($l -match 'frame_avg(?:_ms)?=([0-9.]+)') { $avg += [double]$Matches[1] }
}
if ($fps.Count) {
    $s = $fps | Measure-Object -Average -Minimum -Maximum
    Write-Output ("  fps       n={0} mean={1:N2} min={2:N2} max={3:N2}" -f $s.Count, $s.Average, $s.Minimum, $s.Maximum)
}
if ($avg.Count) {
    $s = $avg | Measure-Object -Average -Minimum -Maximum
    Write-Output ("  frame_avg n={0} mean={1:N3} min={2:N3} max={3:N3}" -f $s.Count, $s.Average, $s.Minimum, $s.Maximum)
}
