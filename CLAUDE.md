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
- `feed-binance` (package `quantick-feed-binance`) — live aggTrades feed from Binance public endpoints; produces the trade stream the engine consumes. Also captures synchronized L2 depth into `orderbook` state.
- `feed-mt5` (package `quantick-feed-mt5`) — MetaTrader 5 tick feed. Listens on a local TCP socket for the QuantickBridge EA (`bridge/mt5/`, MQL5) running inside the logged-in terminal; no credentials anywhere. Side inference policy, synthetic ids and server-time conversion are documented in its `lib.rs`.
- `app` (package `quantick-app`) — desktop chart (egui/wgpu planned). A consumer of the engine, never the other way around. Feeds and symbols come from config (`crates/app/config/feeds.toml`, overridable via `QUANTICK_CONFIG` or `./quantick.toml`), never hardcoded.

Dependency direction is one-way: `app` / `feed-*` → `engine` / `orderbook` (the domain crates). Never add a reverse edge. Feed crates never depend on each other.

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

CI (`.github/workflows/ci.yml`) enforces the same four checks on every PR and on pushes to `main`. After pushing a PR, watch CI with `gh pr checks <n> --watch` and fix any failure before requesting review or merging. A PR with red CI is never merged.

## Workflow

- Engine code is developed test-first: write fixture trades + expected bars, then implement until green.
- Branches: `feat/<desc>`, `fix/<desc>`, `docs/<desc>`. Commit messages: conventional style (`feat: ...`, `fix: ...`), imperative mood, English.
