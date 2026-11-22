<#
.SYNOPSIS
Prepares a developer machine to run several quantick agents at once without
starving it of CPU and RAM. See docs/dev-machine.md for the reasoning.

.DESCRIPTION
Idempotent. Every step reports what it found; only the steps below change
anything, and only outside the repository:

  - writes `[build] jobs = <n>` to the user's cargo config when no cap exists;
  - with -PruneMerged, removes clean worktrees whose branch is merged into
    origin/main, and deletes their branches;
  - with -LowerAgentPriority, sets running Claude Code processes to
    BelowNormal, so the cargo they start inherits it;
  - with -DisableRustAnalyzerPlugin, turns off the rust-analyzer LSP plugin in
    the user's Claude Code settings.

-Check changes nothing and exits 1 when the cargo cap is missing.

Windows PowerShell 5.1 compatible.
#>
[CmdletBinding()]
param(
    [switch]$Check,
    [int]$Jobs = 0,
    [switch]$PruneMerged,
    [switch]$LowerAgentPriority,
    [switch]$DisableRustAnalyzerPlugin
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

# Half the logical processors leaves room for a second build and the chart.
# Two is the floor so a small machine still pipelines crates.
$DefaultJobsDivisor = 2
$MinimumJobs = 2
$RustAnalyzerPlugin = 'rust-analyzer-lsp@claude-plugins-official'
$Utf8NoBom = New-Object System.Text.UTF8Encoding $false

function Write-Step([string]$Status, [string]$Message) {
    Write-Host ('[{0,-7}] {1}' -f $Status, $Message)
}

function Get-CargoConfigPath {
    $cargoHome = $env:CARGO_HOME
    if (-not $cargoHome) { $cargoHome = Join-Path $HOME '.cargo' }
    foreach ($name in 'config.toml', 'config') {
        $path = Join-Path $cargoHome $name
        if (Test-Path $path) { return $path }
    }
    return (Join-Path $cargoHome 'config.toml')
}

function Set-CargoJobsCap {
    $path = Get-CargoConfigPath
    $text = ''
    if (Test-Path $path) { $text = [IO.File]::ReadAllText($path) }
    $existing = [regex]::Match($text, '(?m)^\s*jobs\s*=\s*(\S+)')
    if ($existing.Success) {
        Write-Step 'ok' "cargo build.jobs = $($existing.Groups[1].Value) ($path)"
        return $true
    }
    if ($env:CARGO_BUILD_JOBS) {
        Write-Step 'ok' "CARGO_BUILD_JOBS = $env:CARGO_BUILD_JOBS in the environment"
        return $true
    }
    $n = $Jobs
    if ($n -le 0) {
        $n = [Math]::Max($MinimumJobs, [int][Math]::Floor([Environment]::ProcessorCount / $DefaultJobsDivisor))
    }
    if ($Check) {
        Write-Step 'missing' "cargo has no build.jobs cap; setup would write jobs = $n to $path"
        return $false
    }
    $build = [regex]::Match($text, '(?m)^\[build\][^\S\r\n]*\r?$')
    if ($build.Success) {
        $text = $text.Insert($build.Index + $build.Length, "`njobs = $n")
    } else {
        if ($text.Length -gt 0) {
            if (-not $text.EndsWith("`n")) { $text += "`n" }
            $text += "`n"
        }
        $text += "[build]`njobs = $n`n"
    }
    New-Item -ItemType Directory -Force -Path (Split-Path $path) | Out-Null
    [IO.File]::WriteAllText($path, $text, $Utf8NoBom)
    Write-Step 'changed' "wrote [build] jobs = $n to $path"
    return $true
}

function Invoke-Git([string[]]$Arguments) {
    $output = & git @Arguments 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    return $output
}

function Get-MergedWorktrees([string]$Repo) {
    if ($null -eq (Invoke-Git @('-C', $Repo, 'rev-parse', '--verify', '--quiet', 'origin/main'))) {
        Write-Step 'skip' 'no origin/main; run git fetch first'
        return @()
    }
    # A worktree cut a moment ago sits on origin/main's first-parent line and
    # is not merged work; a merged PR branch is reached only through a merge.
    $mainline = @{}
    foreach ($sha in (Invoke-Git @('-C', $Repo, 'rev-list', '--first-parent', 'origin/main'))) { $mainline[$sha] = $true }
    $merged = @()
    $path = $null
    $branch = $null
    foreach ($line in ((Invoke-Git @('-C', $Repo, 'worktree', 'list', '--porcelain')) + '')) {
        if ($line -like 'worktree *') { $path = $line.Substring(9); $branch = $null }
        elseif ($line -like 'branch refs/heads/*') { $branch = $line.Substring(18) }
        elseif ($line -eq '' -and $path -and $branch -and $branch -ne 'main') {
            $tip = Invoke-Git @('-C', $Repo, 'rev-parse', $branch)
            & git -C $Repo merge-base --is-ancestor $branch origin/main 2>$null
            if ($LASTEXITCODE -eq 0 -and -not $mainline.ContainsKey($tip)) {
                $merged += [pscustomobject]@{ Path = $path; Branch = $branch }
            }
            $path = $null
        }
    }
    return $merged
}

function Invoke-WorktreeReport {
    $repo = Invoke-Git @('-C', $PSScriptRoot, 'rev-parse', '--path-format=absolute', '--git-common-dir')
    if (-not $repo) { Write-Step 'skip' 'not inside a quantick checkout'; return }
    $repo = Split-Path $repo
    $merged = @(Get-MergedWorktrees $repo)
    if ($merged.Count -eq 0) { Write-Step 'ok' 'no merged worktrees left behind'; return }
    if (-not $PruneMerged -or $Check) {
        Write-Step 'found' "$($merged.Count) merged worktree(s); rerun with -PruneMerged to remove the clean ones:"
        $merged | ForEach-Object { Write-Host "          $($_.Path) ($($_.Branch))" }
        return
    }
    foreach ($w in $merged) {
        if (Invoke-Git @('-C', $w.Path, 'status', '--porcelain')) {
            Write-Step 'kept' "$($w.Path) has uncommitted changes"
            continue
        }
        & git -C $repo worktree remove $w.Path 2>$null
        if ($LASTEXITCODE -ne 0) { Write-Step 'kept' "$($w.Path) could not be removed (a process may hold it)"; continue }
        & git -C $repo branch -d $w.Branch 2>$null | Out-Null
        Write-Step 'removed' "$($w.Path) ($($w.Branch))"
    }
}

function Invoke-PriorityStep {
    $agents = @(Get-Process -Name claude -ErrorAction SilentlyContinue)
    if (-not $LowerAgentPriority -or $Check) {
        Write-Step 'info' "$($agents.Count) Claude Code process(es) running; -LowerAgentPriority sets them to BelowNormal"
        return
    }
    foreach ($p in $agents) {
        try { $p.PriorityClass = 'BelowNormal'; Write-Step 'changed' "claude pid $($p.Id) -> BelowNormal" }
        catch { Write-Step 'kept' "claude pid $($p.Id): $($_.Exception.Message)" }
    }
}

function Invoke-RustAnalyzerStep {
    $path = Join-Path $HOME '.claude\settings.json'
    if (-not (Test-Path $path)) { Write-Step 'ok' 'no Claude Code user settings'; return }
    $text = [IO.File]::ReadAllText($path)
    $pattern = '("' + [regex]::Escape($RustAnalyzerPlugin) + '"\s*:\s*)true'
    if ($text -notmatch $pattern) { Write-Step 'ok' 'rust-analyzer LSP plugin is not enabled for agents'; return }
    if (-not $DisableRustAnalyzerPlugin -or $Check) {
        Write-Step 'found' 'rust-analyzer LSP plugin is enabled; each agent may start its own (2-6 GB). -DisableRustAnalyzerPlugin turns it off'
        return
    }
    [IO.File]::WriteAllText($path, ([regex]::Replace($text, $pattern, '${1}false')), $Utf8NoBom)
    Write-Step 'changed' "disabled $RustAnalyzerPlugin in $path (new sessions)"
}

function Invoke-DefenderReport {
    try {
        $status = Get-MpComputerStatus -ErrorAction Stop
        if ($status.RealTimeProtectionEnabled) {
            Write-Step 'info' 'Defender real-time protection is on; a Dev Drive for the worktrees keeps it on and cheap'
        } else {
            Write-Step 'warn' 'Defender real-time protection is off; this machine runs build scripts from ~580 crates'
        }
    } catch {
        Write-Step 'skip' 'Defender status unavailable'
    }
}

$capped = Set-CargoJobsCap
Invoke-WorktreeReport
Invoke-PriorityStep
Invoke-RustAnalyzerStep
Invoke-DefenderReport

if ($Check -and -not $capped) { exit 1 }
exit 0
