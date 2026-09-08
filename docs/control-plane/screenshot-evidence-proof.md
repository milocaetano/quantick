# Screenshot evidence: geometry, original pixels and recovery

Issue [#337](https://github.com/milocaetano/quantick/issues/337) separates two
claims: maintained synthetic fractional geometry and an actual rendered capture.
The tests inject `RawScreenshot` through the existing gateway seam. They are
**geometry-contract evidence, not execution at Windows 150% DPI**. Capture tooling
retrieves the real egui image from an already running owned instance.

The maintained regressions live in
`crates/app/src/app/tests/screenshot_evidence_tests.rs`. Literal expectations pin
nonzero and clipped rectangles, all four products, original IDs, missing-bounds
codes, published scale rounding, PNG pixel bytes, retained reads and chunking.
Raw scale `1.504` publishes `1.5`: a logical x of `1000` therefore maps to `1500`,
not `1504`. Clipped rectangles retain their coordinates; `within_image` reports
whether they fit rather than silently cropping them. Existing unavailable-image,
redaction, consent and one-drain-skew regressions remain in place.

```powershell
cargo test -p quantick-app screenshot_evidence_tests
python -m unittest discover -s tools/evidence -p test_verify_screenshot_evidence.py
```

The scripts use Python's standard library. The verifier hashes the raw canonical
bundle bytes, validates every declared chunk hash, decodes production RGBA8 PNG
chunks/CRC/filters/pixels and checks the image digest and dimensions. Independent
decimal arithmetic applies the **published** scale to scene bounds; IDs, missing
bounds, instance/revision association and the explicit
`pixels_precede_projections_by_one_drain` disclosure must agree. The negative
fixtures include fully rehashed semantic corruptions, so checking only the outer
digest cannot make them pass. This bounded verifier is not a general PNG library.

## Recorded validation condition

The initial unfiltered `cargo test --workspace` run on 2026-09-08 failed the
existing observer capture budget guard: median 259 us against 250 us. The app
reported 1,910 passed, one failed and four ignored; all three new fractional
tests passed. The helper selected the lowest-p99 batch among three batches of
500 captures. No causal claim follows from this timing result.

One complete ordered verification is planned after the source update:
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
`cargo build --workspace`, then unfiltered `cargo test --workspace` with
`RUST_TEST_THREADS=1` set only in that test subprocess's environment. Record the
exact environment, commands and raw outputs. This serializes harness scheduling;
tests retain their internal concurrency, every default test and the existing
budget. It is not a passing result yet. Preserve the original failure and
cumulative counters; no retries until green. Default CI remains required, and a
serialized local pass would not establish default parallel-suite health.

CI also applies Ruff's `F` checks to all of `tools/`, including these scripts.
Run the applicable lint with an already installed tool and retain its receipt.

## Freeze source and isolate the launch

The source revision `S` includes this implementation and its mission archive.
Record a clean worktree, exact HEAD/tree, toolchain and lockfile hashes, ordered
fmt/clippy/build/workspace-test receipts, then commit under the campaign handoff.
Capture/recovery/review outcomes live in an external evidence index bound to `S`;
do not edit the archive merely to check boxes afterward. A necessary source edit
creates a new `S` and requires updated affected capture evidence.

Immediately before capture, from that clean source, build both binaries into an
owned target directory. Set `QUANTICK_GIT_COMMIT` to the verified full HEAD for
the existing build-environment provenance field; record that assignment, build
commands, exact executable SHA-256 and executable path/PID association. This
declaration is corroborated by build receipts, not accepted as proof by itself.
Check actual drive space before choosing the target directory.

```powershell
$env:QUANTICK_GIT_COMMIT = git rev-parse HEAD
$env:CARGO_TARGET_DIR = Join-Path (Get-Location) 'target'
cargo build -p quantick-app -p quantick-mcp
```

Use a unique owned working directory. Remove inherited `QUANTICK_*` variables
from the child environment before setting the intended ones. The following
overrides must all point inside that owned run directory; populate only authored
fixtures or repository defaults, never personal stores.

| Purpose | Explicit override |
| --- | --- |
| Feed configuration | `QUANTICK_CONFIG` |
| Workspace state | `QUANTICK_UI_STATE` |
| Indicator state/presets/library | `QUANTICK_INDICATORS_STATE`, `QUANTICK_INDICATOR_PRESETS`, `QUANTICK_INDICATORS_DIR` |
| Layouts and chart layers | `QUANTICK_LAYOUTS`, `QUANTICK_CHART_LAYERS` |
| Drawing presets | `QUANTICK_DRAWING_PRESETS` |
| Footprints | `QUANTICK_FOOTPRINT_SETTINGS`, `QUANTICK_FOOTPRINT_PRESETS` |
| Paper state/history | `QUANTICK_PAPER_STATE`, `QUANTICK_TRADES_DIR` |
| Symbol catalogue | `QUANTICK_SYMBOLS` |
| Bubble/strategy presets | `QUANTICK_BUBBLES`, `QUANTICK_STRATEGY_PRESETS` |
| Replay library | `QUANTICK_REPLAY_DIR` |

`store_home::home` still resolves Windows Documents through the known-folder
API, calls `create_dir_all` on its Quantick shelf and checks consolidation-marker
metadata. Resolve the real redirected known folder and check directory metadata
before launch. An existing shelf makes directory creation an idempotent probe.
All ten `COCKPIT_STORES` overrides skip personal source/destination contents; no
store rescue or marker write results. `QUANTICK_TRADES_DIR` returns before paper
journal consolidation. These overrides do not promise zero known-folder calls.
If the shelf is absent, resolve the resulting directory-creation side effect
within the authorized scope before launching; do not silently change the runtime.

Startup unconditionally calls `spawn_live` **before replay hooks**. Merely setting
replay autostart does not suppress initial exchange networking. Use the existing
MetaTrader provider as an idle owned loopback listener, with no terminal or bridge:

```toml
default_feed = "q7-fixture"
default_symbol = "Q7FIXTURE"

[[feeds]]
id = "q7-fixture"
name = "Authored offline fixture"
provider = "metatrader"
symbols = ["Q7FIXTURE"]
default_layout = "time+flow"
default_bars = "tick:50"

[metatrader]
# Replace this example with the actual reserved and checked owned port.
listen_addr = "127.0.0.1:19107"
bridge_autostart = false
bridge_command = []
```

The configuration parser **rejects port 0**. A wrapper can reserve an OS-selected
loopback port temporarily, put its actual nonzero port in the config, release it
immediately before spawn and verify the owned process/listener. Account for the
bind race; never reuse the trader's port 9100 or attach an existing listener.

Author a CSV labelled `source=authored offline evidence fixture`, with symbol
`Q7FIXTURE`, `timezone=+00:00`, `side_source=authored_fixture`, metadata header
`# quantick-replay 1` and columns `Date,Time,Price,Bid,Ask,Volume,Side`. Leave quotes
empty when none were authored; use `B`/`S` sides. A deterministic varying series
over enough minutes must visibly populate both time and tick canvases. Hash the
exact CSV/config bytes. This is fabricated test data, never a market observation.

Set existing hooks `QUANTICK_REPLAY_AUTOSTART=1`, an explicit fixture session and
speed, `QUANTICK_LAYOUT=time+flow`, and a practical `QUANTICK_WINDOW_SIZE` such as
`1200x800`. Grant only named observation/evidence permissions:

```text
QUANTICK_CONTROL_ACCESS=1
QUANTICK_CONTROL_SCOPES=observe.system,observe.workspace,observe.market,observe.chart,observe.health,observe.indicators,observe.orderflow,observe.attention,observe.events,observe.evidence,observe.screenshot
```

Scene access uses `observe.attention`; there is no `observe.scene` permission.
Start the owned GUI with PowerShell `Start-Process -PassThru`, an owned cwd and
separate stdout/stderr files. Record PID/executable hash; do not title-match or
inject input. Confirm replay loaded and inspect `APP_HEALTH_SUMMARY` and structured
diagnostics before trusting pixels. Do not launch until campaign resource/scope
coordination confirms this concrete recipe; that coordination is not a new
trader permission prompt.

## Capture and independently inspect originals

Discover supported existing runtime/host scale conditions from final source and
read-only host observations. Record actual window/monitor DPI, health scale and
screenshot `pixels_per_point`. Use a genuine available fractional condition when
supported and within scope. No global DPI changes, synthetic metadata, injected
zoom, unsupported environment overrides or silent skip count as OS proof. If
only unit scale is feasible, document the concrete reason beside the maintained
fractional tests. Reproduced DPI or other behavior defects retain their evidence
and are routed separately; this mission does not fix or close #240.

After the exact-source build/isolated launch, run the retriever with the owned PID
and full source SHA. The output directory must be new:

```powershell
python tools/evidence/capture_screenshot_evidence.py --mcp C:/owned/target/debug/quantick-mcp.exe --pid 12345 --source-sha FULL_SOURCE_SHA --output C:/owned/run/capture
python tools/evidence/verify_screenshot_evidence.py C:/owned/run/capture/manifest.json C:/owned/run/capture/bundle.json --png C:/owned/run/capture/screenshot.png
```

The retriever describes instances first, selects exactly the owned PID, describes
that instance, reads scene/health, then captures and reads every chunk. It checks
chunk index/offset/length/digests and associations, retaining original response
lines, exact concatenated bundle bytes, separate manifest and unmodified embedded
PNG. A `PrintWindow` image may supplement evidence but cannot replace the real
egui image. Keep private descriptor/token material out of published artifacts;
review raw operational logs before selecting a public-safe evidence pack.

**Predeclared tolerance: three physical pixels at each measured canvas edge.**
An independent inspector must open the original PNG and identify two distinct
visible canvas targets by stable `pane.<id>.canvas` IDs. Record measured edges,
the semantic pixel rectangle and differences; separately describe the visible
content that identifies each target. Integrity and arithmetic alone cannot prove
which pixels depict a named control. Optional overlays/crops are distinct assets,
never replacements for the original hashed PNG. Applicable two-canvas visual/UX
and evidence checks must pass; a general limitations note does not waive them.

## Recover durable original bytes

Local preparation performs no remote write. It optionally compresses the original
bundle without rewriting JSON/PNG and emits bounded ASCII base64 comment bodies:

```powershell
python tools/evidence/recover_screenshot_evidence.py prepare --capture C:/owned/run/capture --provenance C:/owned/run/provenance.json --inspection C:/owned/run/original-image-inspection.md --source-sha FULL_SOURCE_SHA --output C:/owned/run/recovery-pack
```

The command reports the projected comment count before publication. Each body
contains at most 48,000 base64 characters (36,000 compressed bytes). The original
PNG is already in the canonical bundle, so publication need not duplicate it.
The separate manifest and provenance remain recoverable artifacts too. Provenance
must contain top-level `source_sha` plus exact build/binary/config/fixture hashes.

The coordinator publishes the bodies sequentially as comments on #337, records
their actual IDs in a copy of `manifest-draft.json`, and pins the final recovery
manifest's SHA-256 in the campaign checkpoint. Preserve artifact names, ordering,
decoded lengths, per-chunk/compressed/original hashes and original PNG hash.
Then recover through fresh public raw-comment API reads:

```powershell
python tools/evidence/recover_screenshot_evidence.py recover --manifest C:/owned/run/recovery-manifest.json --manifest-sha256 FULL_MANIFEST_SHA256 --output C:/owned/run/fresh-recovery
```

Recovery compares original bytes, extracts the original PNG, reruns integrity and
geometry checks and writes `readback-report.json`. A transient local path or prose
`screenshot=true` claim is insufficient. Comments remain editable/deletable:
pinned full hashes detect changes but cannot restore deleted objects. Retain
local originals and the checkpoint's recovery manifest. No release, gist, extra
branch, paid service or deployment is needed.

Stop only owned app/MCP processes after capture. Record actual command UTC/exits
and original output hashes. Report validation failures before retry/repair and
retain cumulative per-signature history; automatic retry loops are not evidence.
The final source-bound evidence index identifies actual artifacts and independent
review/CI results. This document does not itself claim a capture, recovery or
score increase has happened.
