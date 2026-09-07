# Q3 incremental lane transport evidence

Status: producer/worker correctness, all final performance gates and the final workspace checks pass. The initial workspace observer timing failure and bounded unchanged retry are retained. Independent reviews and remote delivery remain coordinator-owned and pending.

## Scope and boundary

Issue [#134](https://github.com/milocaetano/quantick/issues/134), campaign [#330/Q3](https://github.com/milocaetano/quantick/issues/330). Integrated baseline `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`; PR target `campaign/architecture-a`.

`LaneTransport` holds only a rung budget and the number of forming trades already sent. The actual pane producer takes its unsent suffix from the retained tape. The worker owns and concatenates the ordered run across command batches. `BarClosed`, `Backfilled`, `Rebuild`, vanished partial and lane disable cut the old run. Rebuild and lane enable cold-seed the current partial once, including when no new feed data arrives. A lane width change resamples without retransmitting the prefix.

The public v1 control/wire contracts, bar-building rules, financial behavior and retained trades are untouched. There is no new capability or registry entry. Existing ladder golden tests are unchanged. Clearing an earlier batch partial at a close/rebuild is required so an earlier epoch cannot become the new epoch's preview.

The worker still folds the entire forming run in `lane_prefixes`: this task reduces producer copying and transport, not that fold. Worker logical retained length is the current run; vector capacity retains its allocation high-water mark across closes/rebuilds and is released on disable/vanished partial. These measurements do not establish global linear/constant cost or the A+ scalability gate. Issue #155 remains separate.

## Reproduction and evidence identity

Raw evidence root: `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q3-validation/`.

- `host-release.txt` preserves the literal exclusive host grant and UTC receipt before arming; additional host yields are in `commands.txt` and the campaign journal.
- `predeclared-conditions.txt` SHA256 `54F962F20193A611AD55E2B0FBF710ABEA42989B3EE27105A0D9FBD00D0D35F5` was written before any measurement. `predeclared-correction.txt` corrects the inspected startup hook before measurement: a private, unused MT5 loopback port with bridge autostart disabled supplies an offline opening source, then replay autostart replaces it. The stall hook only overrides health presentation and is not used.
- All cargo commands use `CARGO_TARGET_DIR=C:/src/quantick-agent-target-fix`. `target-ownership.json` records no user Quantick app process, and only owned cargo/rustc processes. Ownership observation followed the arming launch; no existing app was displaced. Source, environment, binary and fixture hashes are retained in this raw directory.
- `baseline-instrumentation.diff` contains only the test fixture and test-only command counters; baseline production behavior is unchanged. `baseline-tests.exe` and `baseline-app.exe` are retained for alternating runs.

### Executable reproduction

Run from the repository root on Windows with Rust, Python and PowerShell installed, in an exclusive build/measurement window. The checked-in CPU fixture is self-contained:

```powershell
$q3Scratch = Join-Path $env:TEMP ('quantick-q3-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $q3Scratch | Out-Null
$env:CARGO_TARGET_DIR = Join-Path $q3Scratch 'target'
cargo test -p quantick-app lane -- --nocapture
cargo test -p quantick-app app::tests::control_plane_tests::incremental_lane_dense_frame_benchmark -- --exact --ignored --nocapture --test-threads=1
cargo build -p quantick-app
Copy-Item (Join-Path $env:CARGO_TARGET_DIR 'debug/quantick-app.exe') (Join-Path $q3Scratch 'candidate-app.exe')
```

For baseline, use a separate checkout at `0bd50f9b815a05e2ba8d0c9804324dbb415f6658` and apply the retained `final-baseline-instrumentation.diff` with `git apply`. This copies the identical ignored fixture, adds the same `cfg(test)` counter at `IndicatorWorker::send`, and adds the retained-run probe returning `(0, 0)` because baseline retains no run. It changes no baseline production code. Build both the test and ordinary app there with the same commands and a separate target directory. Preserve each source diff and binary SHA256 before measurement. Run each retained test executable with:

```powershell
& $q3TestExe 'app::tests::control_plane_tests::incremental_lane_dense_frame_benchmark' --exact --ignored --nocapture --test-threads=1
Get-FileHash -Algorithm SHA256 $q3TestExe
```

Use B1,A1,A2,B2,B3,A3, finishing each command before the next. Each executes time then dollar. Compute each version's median of its three run means, p95s and p99s, then candidate/baseline ratios. Ceilings remain 1.10 for mean and 1.15 for either tail. The original-harness pilot is a separate B1/A1 comparison, never pooled with this set.

Generate the desktop CSV with this Python script, passing the scratch root as its first argument. Use LF endings, ids 1..180000, and verify the hash against the artifact table:

```python
import datetime, hashlib, sys
from pathlib import Path
root = Path(sys.argv[1])
tape = root / 'replay' / 'BTCUSDT' / '20240703.csv'
tape.parent.mkdir(parents=True, exist_ok=True)
with tape.open('w', encoding='utf-8', newline='\n') as out:
    out.write('# quantick-replay 1\n# symbol=BTCUSDT\n# timezone=+00:00\n# side_source=exchange\nDate,Time,Price,Bid,Ask,Volume,Side\n')
    for i in range(1, 180001):
        stamp = datetime.datetime.fromtimestamp((1720000020000 + i) / 1000, datetime.timezone.utc)
        out.write(f"{stamp:%Y-%m-%d},{stamp:%H:%M:%S}.{i % 1000:03d},{60000 + (i % 20) / 10:.1f},,,0.01,{'S' if i % 3 == 0 else 'B'}\n")
print(hashlib.sha256(tape.read_bytes()).hexdigest())
```

Save this as `time-feeds.toml` in the scratch root; for `dollar-feeds.toml`, change `default_bars` to `dollar:12000000`. Port 19234 must be unused for the offline opening source.

```toml
default_feed = "q3-offline"
default_symbol = "BTCUSDT"
[[feeds]]
id = "q3-offline"
name = "Q3 offline replay"
provider = "metatrader"
symbols = ["BTCUSDT"]
default_layout = "flow"
default_bars = "time:1m"
bubble_preset = "dense tape btc"
[metatrader]
listen_addr = "127.0.0.1:19234"
side_source = "tick_rule"
bridge_autostart = false
```

Run this launcher in a fresh PowerShell process for each version/spec/run, setting `$q3Scratch` to the generated directory. Use the corresponding source checkout as working directory so repository preset assets resolve identically. Each run uses fresh stores and captures only its owned PID; preserve any existing application and stores.

```powershell
$q3Version = 'candidate'; $q3Spec = 'time'; $q3RunNumber = 1
$q3Run = Join-Path $q3Scratch "gui-$q3Version-$q3Spec-$q3RunNumber"
New-Item -ItemType Directory -Path $q3Run | Out-Null
Get-ChildItem Env:QUANTICK_* | ForEach-Object { Remove-Item -LiteralPath "Env:$($_.Name)" }
foreach ($q3Store in @('UI_STATE','LAYOUTS','INDICATORS_STATE','INDICATOR_PRESETS','CHART_LAYERS','DRAWING_PRESETS','FOOTPRINT_SETTINGS','FOOTPRINT_PRESETS','SYMBOLS','PAPER_STATE','STRATEGY_PRESETS')) {
    [Environment]::SetEnvironmentVariable("QUANTICK_$q3Store", (Join-Path $q3Run "$q3Store.toml"), 'Process')
}
$env:QUANTICK_TRADES_DIR = Join-Path $q3Run 'trades'
$env:QUANTICK_CONFIG = Join-Path $q3Scratch "$q3Spec-feeds.toml"
$env:QUANTICK_REPLAY_DIR = Join-Path $q3Scratch 'replay'
$env:QUANTICK_REPLAY_AUTOSTART = '1'
$env:QUANTICK_REPLAY_SPEED = '1'
$env:QUANTICK_REPLAY_DAY_BEFORE = '0'
$env:QUANTICK_INDICATORS_AUTOSTART = '1'
$env:QUANTICK_BUBBLES_AUTOSTART = '1'
$env:QUANTICK_WINDOW_SIZE = '1400x900'
$env:QUANTICK_LAYOUT = 'flow'
$env:QUANTICK_LOG_FORMAT = 'json'
$env:RUST_LOG = 'quantick=info'
$q3Exe = Join-Path $q3Scratch "$q3Version-app.exe"
Get-FileHash $q3Exe | ConvertTo-Json | Set-Content (Join-Path $q3Run 'exe-hash.json')
Get-ChildItem Env:QUANTICK_* | Select-Object Name,Value | ConvertTo-Json | Set-Content (Join-Path $q3Run 'environment.json')
$q3App = Start-Process -FilePath $q3Exe -WorkingDirectory (Get-Location).Path -WindowStyle Hidden -RedirectStandardOutput (Join-Path $q3Run 'stdout.log') -RedirectStandardError (Join-Path $q3Run 'stderr.log') -PassThru
@{pid=$q3App.Id; started_utc=[DateTime]::UtcNow.ToString('o'); exe=$q3Exe} | ConvertTo-Json | Set-Content (Join-Path $q3Run 'process.json')
Start-Sleep -Seconds 26
$q3Capture = Get-Content -Raw tools/capture_window.ps1
$q3Capture = $q3Capture.Replace('Get-Process -Name $ProcessName', "Get-Process -Id $($q3App.Id)")
& ([scriptblock]::Create($q3Capture)) -OutPath (Join-Path $q3Run 'screen.png')
$q3App.CloseMainWindow() | Out-Null
if (-not $q3App.WaitForExit(2000)) { Stop-Process -Id $q3App.Id }
Get-Content (Join-Path $q3Run 'stderr.log') | ForEach-Object {
    try { $_ | ConvertFrom-Json } catch {}
} | Where-Object event_code -eq 'APP_HEALTH_SUMMARY' | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $q3Run 'health.json')
```

Execute desktop B1,A1,A2,B2 for time, then B1,A1,A2,B2 for dollar. Retain health records at launch age 6..26 seconds. Summarize mean frame time and median reported worst per run, then each version's two-run medians. Keep CPU and desktop results separate. Inspect every screenshot for complete HUD/lane presentation; PrintWindow can return a partial image as documented below. Reproduction does not replace any original recorded sample.

## Predeclared conditions

CPU: 1400x900 logical points, `dense tape btc`, CVD enabled, actual `ChartState` specification asserted as time(60000) or dollar(12000000). Synthetic BTC prints cost 60000 + (id modulo 20)/10, quantity 0.01, buy except every third sell, timestamp 1720000020000 + id milliseconds. Initial history 8000 prints; 30 warmup and 600 measured frames, 64 prints/frame. Timed production `draw_frame`, with lane publication observed during measured frames before any final flush. Three alternating pairs B/A, A/B, B/A. All valid samples retained. Gate median frame mean at most baseline +10%, p95 and p99 at most +15%.

Desktop: the same synthetic print recipe, 180000-print offline CSV, replay 1x, 1400x900, dense preset and EMA+CVD through existing native-autostart hook. Two alternating pairs B/A, A/B for each time/dollar spec. Fresh isolated stores each run; 6 seconds warmup, 20 seconds observed, PID-specific screenshot. Ordinary `APP_HEALTH_SUMMARY` is the HUD source: reported mean at most +10%, median reported worst at most +15%; >=50fps and a visibly presented capture required. Desktop presentation and CPU fixture are separate evidence.

## Correctness evidence

`cargo test -p quantick-app lane -- --nocapture` passed 111 tests (5 ignored). Raw log `lane-tests-repair1.log` includes the existing, unchanged `the_lane_samples_end_where_the_preview_does` and all new producer/worker cases.

The actual producer test `producer_transport_is_new_prints_independent_of_old_history` calls `ChartPane::ingest_live_trade` and `publish_partial`, then counts `run.len()` at the real worker command-send boundary. All six combinations passed:

| Old-history prints | New one-print drains | New entries | Separate cold seed | Final worker run |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 128 | 128 | 0 | 128 |
| 0 | 256 | 256 | 0 | 256 |
| 1024 | 128 | 128 | 1024 | 128 |
| 1024 | 256 | 256 | 1024 | 256 |
| 8192 | 128 | 128 | 8192 | 128 |
| 8192 | 256 | 256 | 8192 | 256 |

Every extra no-new-data publication sends zero entries. The historical seed is explicit; the continuously enabled new epoch sends N entries, replacing the original N*(N+1)/2 full-prefix copying shape. Closures send their authoritative bar separately, so prints belonging only to already closed bars need no forming-run transport.

`producer_close_rebuild_lane_toggle_and_empty_updates_preserve_the_run` and `producer_prepend_rewind_and_repeated_rebuild_seed_the_recut_partial_once` cover close/vanished partial, recut, backfill, prepend, reset/rewind, a pane seeded mid-session, repeated rebuild, disable/re-enable, lane resize and repeated empty updates; retained tape count stays unchanged. Worker tests use signed quantities +2,-1,+4,-2,+3,-1,+5 after a +10 history bar, independently expecting CVD rungs (3,15),(6,15),(7,20), committed column [10], preview20. A single complete queue, irregular updates in one batch, and separate worker drains all agree. Additional independently specified cases prove close/rebuild/backfill/vanish/disable boundaries do not leak the old run.

## Original pilot, retained separately

Before the final set, the coordinator strengthened lane observation from warmup-or-measured frames to measured frames only. That decision preceded the original candidate pilot. `measured-lane-assertion-decision.txt` records the instruction to retain both original binaries/harness hashes and report their valid B/A pilot separately, then run the complete final set with an identical stronger harness. Nothing was rejected as a timing outlier, and identities are not blended.

| Spec | Version | Mean ms | p95 ms | p99 ms | Worst ms | Prints/s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| time(1m) | baseline | 0.710618 | 1.006000 | 1.298700 | 1.782600 | 89878.488 |
| time(1m) | candidate | 0.368243 | 0.543700 | 0.697500 | 1.440600 | 173112.867 |
| dollar(12000000) | baseline | 0.640367 | 0.932100 | 1.056800 | 1.128100 | 99729.561 |
| dollar(12000000) | candidate | 0.612171 | 1.010600 | 1.347800 | 2.347300 | 104293.979 |

The adverse dollar pilot p99 rose from **1.0568 to 1.3478ms (+27.54%)**, above 15%; it is retained explicitly. The final predeclared aggregate below does not reproduce that tail regression. There is no attributed cause or unproven noise explanation. Pilot limitation: its lane visibility assertion could be satisfied during warmup.

## Final identical-harness CPU set

Executed order: B1,A1,A2,B2,B3,A3; each executable runs time then dollar. All six invocations exit0, and all valid samples are shown. The CPU fixture is headless production frame work, not display/VSync frame time.

| Spec | Version/run | Mean ms | p95 ms | p99 ms | Worst ms | Prints/s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| time(1m) | baseline/1 | 0.770502 | 1.167000 | 1.493000 | 2.137600 | 82875.611 |
| time(1m) | baseline/2 | 0.782574 | 1.115600 | 1.303900 | 1.668700 | 81611.261 |
| time(1m) | baseline/3 | 0.759939 | 1.096800 | 1.297500 | 1.430400 | 84039.223 |
| time(1m) | candidate/1 | 0.348687 | 0.433500 | 0.539000 | 0.959600 | 182809.003 |
| time(1m) | candidate/2 | 0.373045 | 0.464300 | 0.595900 | 0.967000 | 170873.690 |
| time(1m) | candidate/3 | 0.396168 | 0.491600 | 0.673900 | 1.018700 | 160892.484 |
| dollar(12000000) | baseline/1 | 0.733454 | 1.160900 | 1.441600 | 1.651300 | 87071.315 |
| dollar(12000000) | baseline/2 | 0.717262 | 1.050800 | 1.236400 | 1.604100 | 89031.545 |
| dollar(12000000) | baseline/3 | 0.692509 | 1.040700 | 1.206200 | 1.325500 | 92203.343 |
| dollar(12000000) | candidate/1 | 0.612108 | 0.988100 | 1.151300 | 1.386600 | 104303.242 |
| dollar(12000000) | candidate/2 | 0.615317 | 0.906400 | 1.010200 | 1.524700 | 103746.295 |
| dollar(12000000) | candidate/3 | 0.624616 | 0.931200 | 1.102600 | 1.219500 | 102211.794 |

| Spec | Version | Entries per 600 frames | Bytes | Retained entries | Retained capacity |
| --- | --- | ---: | ---: | ---: | ---: |
| time(1m) | baseline | 17491200 | 979507200 | 0 | 0 |
| time(1m) | candidate | 38400 | 2150400 | 48320 | 64512 |
| dollar(12000000) | baseline | 6011200 | 336627200 | 0 | 0 |
| dollar(12000000) | candidate | 38304 | 2145024 | 8320 | 16384 |

Every run has 600 measured updates and the same entry counts. Seeds occur during warmup. Dollar drains that close a bar omit its closed-only prints from the forming-run payload; hence 38304 entries rather than 38400. Baseline retains no run between worker batches; candidate intentionally retains the current run. Trade size is 56 bytes.

## Original desktop Perf HUD and presentation

Executed time order: B1,A1,A2,B2; dollar order: B1,A1,A2,B2. Each ordinary app runs the same offline tape at 1x in fresh isolated stores. Rows summarize every health point with launch age 6..26 seconds; all runs have 10 retained points. `APP_HEALTH_SUMMARY` supplies the same FPS/frame counters as the visible Perf HUD. There is no timing substitution with the headless CPU fixture.

| Spec | Version/run | Mean frame ms | Mean CPU ms | Median reported worst ms | Maximum reported worst ms | Mean FPS | Prints/s |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| time | baseline/1 | 16.663112 | 2.594289 | 18.622601 | 20.748199 | 59.70 | 998.119 |
| time | baseline/2 | 16.664229 | 2.708880 | 18.678301 | 20.900299 | 59.40 | 997.002 |
| time | candidate/1 | 16.663306 | 2.435971 | 20.322900 | 21.145300 | 59.60 | 992.694 |
| time | candidate/2 | 16.666514 | 2.249845 | 18.015499 | 19.975700 | 59.50 | 998.969 |
| dollar | baseline/1 | 16.666474 | 2.807356 | 18.177999 | 20.892599 | 59.40 | 993.277 |
| dollar | baseline/2 | 16.665570 | 2.609229 | 18.323850 | 21.338200 | 59.70 | 992.230 |
| dollar | candidate/1 | 16.665432 | 2.607695 | 18.991799 | 19.703199 | 59.40 | 994.883 |
| dollar | candidate/2 | 16.666842 | 2.571652 | 18.354400 | 21.159500 | 59.30 | 995.521 |

Complete time captures `gui-baseline-time-2/screen.png` and `gui-candidate-time-2/screen.png` show the chart, active tape/CVD, replay controls and original bottom Perf HUD; the candidate visibly reads 60fps /16.7ms. Core lane and CVD rendering remain visible in `gui-candidate-dollar-1/screen.png`.

**Capture limitation:** several original PID-specific PrintWindow images contain white client-bottom bands; all originals remain intact. Near-white full-width row counts: baseline time1=37, time2=0; candidate time1=89, time2=0; baseline dollar1=11, dollar2=0; candidate dollar1=63, dollar2=115. Candidate time1 omits replay controls that baseline time1 shows. No complete visual PASS is claimed from those partial images, and no capture-cause or render-regression attribution is made. This limitation does not replace or alter any HUD timing record. The complete time pair supplies the original time-or-dollar HUD capture, while the complete time/dollar health sets supply both measured spec comparisons.

## Unchanged performance gates

All ten predeclared median aggregate gates pass (`comparison.json`, parser command exit0). Neither the adverse pilot nor the partial images were silently dropped or used to loosen a threshold.

| Fixture/spec | Metric | Baseline | Candidate | Candidate/base | Ceiling | Verdict |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| cpu/time(1m) | mean_ms | 0.770502 | 0.373045 | 0.484158 | 1.10 | PASS |
| cpu/time(1m) | p95_ms | 1.115600 | 0.464300 | 0.416189 | 1.15 | PASS |
| cpu/time(1m) | p99_ms | 1.303900 | 0.595900 | 0.457014 | 1.15 | PASS |
| cpu/dollar(12000000) | mean_ms | 0.717262 | 0.615317 | 0.857869 | 1.10 | PASS |
| cpu/dollar(12000000) | p95_ms | 1.050800 | 0.931200 | 0.886182 | 1.15 | PASS |
| cpu/dollar(12000000) | p99_ms | 1.236400 | 1.102600 | 0.891783 | 1.15 | PASS |
| gui/time | mean_ms | 16.663671 | 16.664910 | 1.000074 | 1.10 | PASS |
| gui/time | median_worst_ms | 18.650451 | 19.169199 | 1.027814 | 1.15 | PASS |
| gui/dollar | mean_ms | 16.666022 | 16.666137 | 1.000007 | 1.10 | PASS |
| gui/dollar | median_worst_ms | 18.250925 | 18.673100 | 1.023132 | 1.15 | PASS |

## Failure and repair ledger

Counters are cumulative. Operation failures: **7**. Repair failures: **0**. Successful repair/retry attempts: **7**. Counts and each signature persist after repairs.

| Signature | Occurrence | Repair attempt | Evidence and outcome |
| --- | ---: | ---: | --- |
| Q3_BENCH_FINAL_LANE_ASSERT | 1 | 1 | `cpu-B1.log`, exit101. Existing Flush-only worker batches can clear published lane samples. The fixture now observes a nonempty CVD lane during actual drawn frames, before Flush. No production behavior changed for this harness repair. |
| Q3_BENCH_SPEC_UNAPPLIED | 1 | 1 | `cpu-B1-repair1.log` exits0 but transports only 14400 entries: actual tick50 revealed that `retain` had not selected the requested bar kind. Repaired with `SpecSelector.set`, both staged apply calls, and an actual-state assertion. This invalid fixture is retained, excluded from time/dollar evidence. `cpu-B1-valid.log` is valid. |
| Q3_GUI_SCRIPT_EXECUTION_POLICY | 1 | 1 | Direct invocation of the owned script was refused before app creation. Process-local PowerShell `-ExecutionPolicy Bypass` launched it successfully; no machine policy was changed. |
| Q3_DRAW_BORROW_E0502 | 1 | 1 | `lane-tests1.log`, compilation exit101. Drawing holds a history slice while publishing borrowed the whole pane. Repair splits lane/state/worker field borrows and uses the same `LaneTransport.command`; guards and focused lane suite passed after host reacquisition. |
| Q3_EVIDENCE_LOG_ENCODING | 1 | 1 | `comparison.log` exits1 before gate computation: PowerShell raw logs use UTF16LE. The parser now detects BOMs; `comparison-repair1.log` exits0. Raw logs and measurements are unchanged. |
| Q3_DOC_HERESTRING_PARSE | 1 | 1 | Nested PowerShell here-string documentation authoring was rejected by the parser, exit1 before any repository mutation. Direct `apply_patch` wrote the reproduction recipe; the documented CSV generator reproduces the exact original fixture SHA256. The failed authoring command/output remain in the session transcript and `failures.txt` records the signature and bounded repair. |
| Q3_WORKSPACE_OBSERVER_BUDGET | 1 | 1 | `final-test.log` exit101: 1913 app tests passed, one failed, five ignored. The existing observer capture median was268us (p99660us, worst2925us), above its unchanged250us budget. The exact isolated test passed (`observer-budget-isolated-repair1.log`), then one unchanged `cargo test --workspace` retry passed (`final-test-repair1.log`). No source, budget or runner settings changed; no cause is inferred. |

No valid timing sample was discarded. `failures.txt` retains append-only original observations, including the clarification that early `repair=0` fields meant repair failures, not repair attempt counts.

## Validation and review

Arming guards build and app all-target check: exit0. Guards after baseline harness, implementation batches and mission archival: exit0. Focused lane tests: 111 passed/5 ignored, exit0; strengthened baseline and candidate test builds and their guard checks exit0. Final ordered `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, and `cargo build --workspace`: all exit0 in `final-fmt.log`, `final-clippy.log`, `final-build.log`. `cargo test --workspace` passed exit0 in `final-test-repair1.log` after the one retained observer budget failure and unchanged bounded retry. No ignored benchmark was enabled by the ordinary suite. The archive is `.claude/GOAL-archive-incremental-lane-transport.md`; independent reviews, exact-head CI and remote delivery remain coordinator-owned and pending.

Final base readback on 2026-09-07: `git fetch origin` exit0; HEAD, `origin/campaign/architecture-a` and their merge-base all resolve to `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`. No rebase was necessary. Only the six assigned implementation/test/archive/dossier files enter the final diff. Cargo.lock, Python/MT5 and guardrail hook paths are unchanged, so their extra path-specific checks do not apply.

## Host and artifact identities

Windows 11 Pro 10.0.26200 x64; Intel Core i7-11800H, 8 cores/16 logical processors. Rust1.98.0, commit88d9e12ae178fab0fb5cc050a94da85685d449ea, x86_64-pc-windows-msvc, LLVM22.1.8. Repository dev/test profile: workspace optimization1, dependencies2. No other mission compiled or measured during the comparison sets.

| Artifact | SHA256 |
| --- | --- |
| `replay/BTCUSDT/20240703.csv` | `b957b56c7ea6c89c934b65012aab1dcac1c0328dbbf943760ba0d1f85064d618` |
| `baseline-app.exe` | `c8320bec69cdf76de8c0a401a4d105029a7f2e82c759c33848ef033df852233c` |
| `candidate-app.exe` | `0580d7f0ba2fea6c371d1076c21459bf1743eec72fd846794bd892f1fea52879` |
| `baseline-tests.exe` | `06bc03e4043d6d5bafac9dbe6e3a497a3a3a0d3cd9e33c154a555c88e50c3994` |
| `candidate-tests.exe` | `6af957b77d333e34e93f9b81c9d4ffa88498f6394d15a9c8cf994ad2def99618` |
| `pilot-baseline-tests.exe` | `dac80dab55099a4e3e244362af28de21c8c6193a7baa8f215ff998f59026b6e3` |
| `pilot-candidate-tests.exe` | `cae85994dbbf46c2bdc0a9195ffa88695d98e9ef5aa678c655d0a3e225a03f5f` |
| `predeclared-conditions.txt` | `54f962f20193a611ad55e2b0fbf710abea42989b3ee27105a0d9fbd00d0d35f5` |
| `predeclared-correction.txt` | `b88ae15e86864c8b0f2a5da47650adeaa1b1fc326546efef18cb43591c33553c` |
| `crates/app/src/pane.rs` | `e3a0c1b4c5b2a3eee042faeabcd09413dd825b04bfe76644b135ba15bcc773e8` |
| `crates/app/src/indicator_worker.rs` | `5a1a04cc294d40b03cf637d56bc015f75d721a55d90a37f8f1a0de41edf39d1f` |
| `crates/app/src/app/tests/control_plane_tests.rs` | `08d823e082cdb2d0361de8842438a37a0f7de8bc530875d242a1bfa27034a5f6` |
| `crates/app/src/pane/tests/lane_transport_tests.rs` | `a9e8610f4abfe6b27442eaea34af8f24221e9ff4a0f24e37dbcd28f3dab7e001` |
| `crates/app/config/bubbles.toml` | `5db6b44a4564f7e0cd26482a3f1badd17eb41788e6959453330f2738bb37cd4d` |
| `Cargo.lock` | `02d505e112dcce244b11627708da2815c6068402997a5c0d805b0e483d1c6721` |
