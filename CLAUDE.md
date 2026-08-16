# quantick

Real-time alternative bar charts (tick / volume / dollar / imbalance bars) for order flow trading. One deterministic Rust engine feeds chart, backtest and bot. See README.md for the full vision and roadmap.

## Commands

- Build: `cargo build --workspace`
- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all` (CI check: `cargo fmt --all -- --check`)

## Architecture

Cargo workspace, crates under `crates/`:

- `engine` (package `quantick-engine`) — raw trades in, alternative bars out. Headless and deterministic: no UI, no network, no async. Everything else depends on it; it depends on nothing else in the workspace.
- `orderbook` (package `quantick-orderbook`) — deterministic local order-book core: validated snapshots, absolute level updates, update-id continuity. A pure domain crate like `engine` (no network, no async, no clock); depends on nothing else in the workspace.
- `replay` (package `quantick-replay`) — recorded market-replay sessions: the CSV tick-file format (read, write and explain why a file was rejected), the folder scan behind the session browser, and the deterministic playback clock. A pure domain crate like `engine` — no network, no async, and it is *told* how much time passed rather than reading a clock; it depends only on `engine`.
- `indicators` (package `quantick-indicators`) — the indicator runtime: engine bars in, plot series out. The `Indicator` trait (commit/preview execution contract with rollback), incremental `ta.*` kernels, draw objects, and the headless `IndicatorHost` the chart, backtest and bot all consume. A pure domain crate like `engine`; depends only on `engine` (plus `rust_decimal` for the bar projection and `libm` for bit-exact transcendentals, the latter enforced by a grep-guard test).
- `pine` (package `quantick-pine`) — the "Quantick Pine" language frontend (Pine v5 subset, dialect reference in `docs/pine-dialect.md`): hand-rolled lexer/parser/compile passes/tree-walking interpreter, zero external dependencies. Compiles a `.pine` script into an implementation of the `indicators` crate's `Indicator` trait; depends only on `indicators`.
- `sim` (package `quantick-sim`) — deterministic paper-trading simulator: the trade stream the engine already consumes in, simulated fills, positions and closed trades out. Conservative tape-based fill model (a market order fills on the next print, a limit on a print at or through its price, a stop on the print that trades through its trigger — never on quotes the tape cannot prove), performance metrics, and the CSV trade-history format. A pure domain crate like `engine`: no UI, no network, no async, no wall clock; depends only on `engine`.
- `feed-binance` (package `quantick-feed-binance`) — live aggTrades feed from Binance public endpoints; produces the trade stream the engine consumes. Also captures synchronized L2 depth into `orderbook` state.
- `feed-hyperliquid` (package `quantick-feed-hyperliquid`) — public Hyperliquid perpetual trades and visible L2 snapshots. Maps venue-reported aggressor sides, assigns documented session-local monotonic ids over Hyperliquid's non-monotonic `tid`, and diffs complete top-20 book images through the provider-neutral `orderbook` contract.
- `feed-mt5` (package `quantick-feed-mt5`) — MetaTrader 5 tick feed. Listens on a local TCP socket for the QuantickBridge EA (`bridge/mt5/`, MQL5) running inside the logged-in terminal; no credentials anywhere. Side inference policy, synthetic ids and server-time conversion are documented in its `lib.rs`.
- `backtest` (package `quantick-backtest`) — the headless backtest harness, second of the three consumers: recorded sessions in, per-session and aggregate performance out. It owns the strategy port (a trait taking a closed bar, the indicator readings and the simulator state, returning `sim::Command`s), the run loop that interleaves prints, bars and fills, and its own report renderer. Headless like a domain crate — no UI, no network, no async, and the only wall-clock read is the stopwatch in its `main.rs`, whose numbers go to the stderr diagnostics and never into a report. Depends on `engine`, `indicators`, `pine`, `replay` and `sim`; nothing depends on it.
- `app` (package `quantick-app`) — desktop chart (egui/wgpu planned). A consumer of the engine, never the other way around. Feeds and symbols come from config (`crates/app/config/feeds.toml`, overridable via `QUANTICK_CONFIG` or `./quantick.toml`), never hardcoded.

Dependency direction is one-way. `app` → `pine` → `indicators` → `engine`; `app` also depends directly on `indicators`, `orderbook`, `replay`, `sim` and the feed crates; `backtest` → `pine` / `indicators` / `replay` / `sim` / `engine`, and nothing depends on `backtest` — a consumer is a leaf, never a dependency; `feed-*` → `engine` / `orderbook` only — a feed produces trades and has no business linking the script language. Never add a reverse edge. Feed crates never depend on each other.

Market replay is a *source*, not a chart mode: `app/src/feed/replay.rs` releases a recorded session down the same `FeedEvent` channel a live venue uses, so bars, navigation and metrics run one code path. UI affordances gate on `FeedCapabilities`, never on "is this a replay?" — a recording reports no depth and no history paging, and the heatmap toggle disables itself from that alone.

## Non-negotiable design rules

- **Determinism**: same trades in → same bars out, always. Inside the engine: no wall-clock time, no randomness, no iteration-order-dependent output (prefer `BTreeMap`/`Vec` over `HashMap` where order can leak into results). Guard with golden/snapshot tests that replay fixed trade fixtures.
- **One engine, three consumers**: chart, backtest and bot consume the same aggregator code path. Never fork bar-building logic per consumer.
- **Data honesty**: inferred or incomplete data is labeled as such, never silently patched.
- **Small and focused**: this is not a trading platform. Build bars, show bars, expose bars to code.

## Verification loop (mandatory)

Every change must pass all four checks before commit — no exceptions:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo build --workspace`
4. `cargo test --workspace`

CI (`.github/workflows/ci.yml`) enforces the same four checks on every PR and on pushes to `main`, plus two the workspace cannot see: `ruff check --select F` over `tools/mt5/` and `bridge/mt5/`, and `python3 tools/mt5/test_export_session.py`. The MetaTrader bridge and the session exporter are Python — cargo never compiles them, and an undefined name there ships silently. Run both locally when you touch either folder. After pushing a PR, watch CI with `gh pr checks <n> --watch` and fix any failure before requesting review or merging. A PR with red CI is never merged.

## Workflow

- Engine code is developed test-first: write fixture trades + expected bars, then implement until green.
- Branches: `feat/<desc>`, `fix/<desc>`, `docs/<desc>`. Commit messages: conventional style (`feat: ...`, `fix: ...`), imperative mood, English.
- **One goal, one worktree**: every new goal/task starts on its own branch cut from updated `main`, checked out in a dedicated git worktree under `../quantick-worktrees/` — never worked on in the main checkout. Parallel agents must never share a working tree. Create it with:

  ```sh
  git fetch origin
  git worktree add -b <prefix>/<slug> ../quantick-worktrees/<prefix>-<slug> origin/main
  ```

  Work happens inside that directory. After the branch is merged, clean up from the main checkout: `git worktree remove ../quantick-worktrees/<prefix>-<slug>` then `git branch -d <prefix>/<slug>`.
- **Arch-review before PR**: before opening a PR (and before any merge), run the `arch-review` skill over `git diff main...HEAD` and resolve every Blocker and Should-fix finding. A finding deliberately deferred is noted in the PR body. No branch ships un-reviewed. The skill's step 0 runs the bundled `code-review` over the same diff, so one command does correctness before shape. On a docs/skills change, where `mission` waives arch-review, run `code-review` on its own — the bug pass is never the waived part.
- **The two rules above are enforced by hooks, not by memory**: a write landing in the main checkout while it sits on `main` is denied, and `gh pr create` is gated on arch-review having run for the branch. See `.claude/hooks/README.md` for what fires when, why the gate is a prompt hook, and how to override.
