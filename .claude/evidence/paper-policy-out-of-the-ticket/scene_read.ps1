# scene_read.ps1 - ask a running instance what it believes is on screen.
#
# The structural half of the "nothing changed for the trader" proof: a
# screenshot says what the window looks like, this says what the application
# thinks is there - orders, position, risk, ruler - which does not move when a
# colour does.

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Mcp,
    [Parameter(Mandatory = $true)][string]$Label,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [Parameter(Mandatory = $true)][string]$ReplayDir,
    [int]$HoldSeconds = 45
)

New-Item -ItemType Directory -Force $OutDir | Out-Null

$scenes = @(
    @{ name = "paper_orders";        env = @{ QUANTICK_PAPER_ORDERS = "2" } },
    @{ name = "paper_order_bracket"; env = @{ QUANTICK_PAPER_ORDERS = "2"; QUANTICK_PAPER_ORDER_BRACKET = "1" } },
    @{ name = "paper_risk";          env = @{ QUANTICK_PAPER_RISK = "100:0.20:1:BRL" } },
    @{ name = "paper_demo";          env = @{ QUANTICK_PAPER_DEMO = "1" } }
)

foreach ($scene in $scenes) {
    $name = $scene.name
    $scratch = Join-Path $OutDir "_s\$Label-$name"
    New-Item -ItemType Directory -Force $scratch | Out-Null
    $log = Join-Path $scratch "app.log"

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
    $env:QUANTICK_REPLAY_DIR = $ReplayDir
    $env:QUANTICK_REPLAY_AUTOSTART = "1"
    $env:QUANTICK_REPLAY_DAY_BEFORE = "0"
    $env:QUANTICK_REPLAY_SPEED = "1000"
    $env:QUANTICK_CONTROL_ACCESS = "1"
    $env:QUANTICK_CONTROL_SCOPES = "all-reads,observe.paper"
    foreach ($k in $scene.env.Keys) { Set-Item "Env:$k" $scene.env[$k] }

    $proc = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $log -RedirectStandardOutput "$log.out"
    Start-Sleep -Seconds $HoldSeconds

    $lines = @(
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"paper-split","version":"1"}}}',
        '{"jsonrpc":"2.0","method":"notifications/initialized"}',
        '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"quantick_get_snapshot","arguments":{"scopes":["session.paper"]}}}'
    )
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Mcp; $psi.Arguments = "--profile observer"
    $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true
    $psi.UseShellExecute = $false
    $m = [System.Diagnostics.Process]::Start($psi)
    $nl = [char]10
    $bytes = [System.Text.Encoding]::ASCII.GetBytes($nl + ($lines -join $nl) + $nl)
    $m.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
    $m.StandardInput.BaseStream.Flush(); $m.StandardInput.Close()
    $out = $m.StandardOutput.ReadToEnd()
    $m.WaitForExit(15000) | Out-Null

    Set-Content -Path (Join-Path $OutDir "$Label-$name.json") -Value $out -Encoding utf8
    try { $proc.Kill(); $proc.WaitForExit(10000) | Out-Null } catch {}
    Write-Output "$Label/$name bytes=$($out.Length)"
}
