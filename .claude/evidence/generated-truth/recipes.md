# The four falsified recipes (A10 — R14, R15, R16, R17)

## 1. The indicator docking row (R14)

**Claim:** `new-extension` gave the indicator port as "`indicators` crate, or a
.pine script", and `crates/indicators/src/native/mod.rs:9-11` says "a third
native is a new file plus one line".

**Measured** against `NativeCvd`, the newest native, production sites only:
```
crates/app/src/app/layout_wiring.rs:635:            SavedKind::NativeCvd => IndicatorSource::NativeCvd,
crates/app/src/app.rs:1096:            pane.add_indicator(IndicatorSource::NativeCvd);
crates/app/src/app.rs:2791:                self.add_native_indicator(SavedKind::NativeCvd);
crates/app/src/app.rs:3517:            SavedKind::NativeCvd => IndicatorSource::NativeCvd,
crates/app/src/indicators/state_file.rs:35:    NativeCvd,
crates/app/src/indicator_worker.rs:88:    NativeCvd,
crates/app/src/indicator_worker.rs:127:            IndicatorSource::NativeCvd => Ok(Box::new(Cvd::new())),
crates/app/src/indicator_worker.rs:209:            IndicatorSource::NativeCvd => "native.cvd".to_owned(),
crates/app/src/indicator_worker.rs:219:            IndicatorSource::NativeCvd => "CVD".to_owned(),
crates/app/src/indicator_worker.rs:1108:            source: IndicatorSource::NativeCvd,
crates/app/src/indicator_worker.rs:1141:            (doomed, IndicatorSource::NativeCvd),
crates/app/src/indicator_worker.rs:1142:            (survivor, IndicatorSource::NativeCvd),
crates/app/src/indicator_worker.rs:1231:            source: IndicatorSource::NativeCvd,
crates/app/src/indicator_worker.rs:1287:            source: IndicatorSource::NativeCvd,
```
One `SavedKind` variant plus 14 call sites across four files. The
`indicators`-crate half of the claim is true — a native is a new file and one
`pub use` there — and that is exactly why the row misled: it stated the cheap
half and omitted the `app` half. Corrected in `new-extension/SKILL.md`, which
now names all fifteen sites and points at the `.pine` route first.

## 2. The feeds clause in CLAUDE.md's headless bullet (R15)

**Claim:** the feeds are among the crates with "no UI, no network, no async, no
wall clock".

**Measured:**
```
feed-binance       .await points: 96
feed-mt5           .await points: 51
feed-hyperliquid   .await points: 43
crates/feed-binance/src/depth/stream.rs:665:    SystemTime::now()
crates/feed-mt5/src/stream.rs:2058:    SystemTime::now()
crates/feed-hyperliquid/src/candles.rs:414:    let deadline = tokio::time::Instant::now() + REPLY_TIMEOUT;
```
190 `.await` points and three clock reads. The rule was never true of the
feeds and could not be: a socket is their job. `CLAUDE.md` now states the
exception narrowly — neither may cross the `FeedEvent` channel — and the
reasoning is in `docs/agentic-development.md`.

## 3. The harness.rs ownership claim (R16)

**Claim:** `ui-harness/SKILL.md` — "`crates/app/src/harness.rs` owns every hook
the window reads at launch".

**Measured:** `harness.rs` declares 24 of 126; 37 files own the rest.
Corrected, and the registry's generated `Declared in` column now answers the
question the claim was standing in for.

## 4. The stale -D warnings prescriptions (R17)

**Re-verification found one of the brief's three already fixed.** `CLAUDE.md`,
`AGENTS.md` and `CONTRIBUTING.md` already carried one flag-free clippy line,
and `CONTRIBUTING.md:44` explains the absence. Two stale prescriptions
remained, and a fourth the brief did not name:
```
docs/mcp-control-plane-development-plan.md:1309   fixed
docs/indicator-system-plan.md:683                 fixed
.github/PULL_REQUEST_TEMPLATE.md:12               fixed (not in the brief)
```
Every remaining mention in the tree now *explains* the absence rather than
prescribing the flag:
```
./CONTRIBUTING.md:44:There is no `-D warnings` on that clippy line, and its absence is deliberate.
./docs/agentic-development.md:67:The clippy line carries no `-D warnings`, and used to. The levels moved into
```
One command now stands across `CLAUDE.md`, `CONTRIBUTING.md` and `AGENTS.md`:
```
13:cargo clippy --workspace --all-targets
39:cargo clippy --workspace --all-targets
207:cargo clippy --workspace --all-targets
115:        run: cargo clippy --workspace --all-targets
```
