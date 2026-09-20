# quantick

Real-time alternative bar charts (tick / volume / dollar / imbalance) for order flow trading; one deterministic Rust engine feeds chart, backtest and bot.

Rule authority. Rationale: `docs/agentic-development.md`; map: `AGENTS.md`; gates: `.claude/hooks/README.md`.

## Verification loop (mandatory)

Between edits: `cargo check -p <crate>`, `cargo test -p <crate> <filter>`, `cargo test -p quantick-guards`. Before code commits, all four — `check` never stands in for `clippy`. Prose-only checks and evidence reuse follow [the delivery contract](docs/workflow/delivery.md#validation-follows-changed-inputs); full final-head CI remains mandatory:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo build --workspace
cargo test --workspace
```

`PostToolUse` is silent until guards are built in that worktree; silence is not a pass. `cargo run -p quantick-guards -- --report` prints metrics.

CI also runs — `sh .claude/hooks/guardrails_test.sh`, `ruff check --select F` over `tools/mt5/` and `bridge/mt5/`, `python3 tools/mt5/test_export_session.py`, `python3 tools/outside_score/test_measure.py`, `python3 bridge/mt5/tests/test_*.py`, and `cargo deny check bans licenses` when `Cargo.lock` moves. Run what your change touches; watch `gh pr checks <n> --watch`; red CI never merges.

## Architecture

`AGENTS.md` maps crate ownership and dependencies:

- **Dependency direction is one-way; never add a reverse edge.** `app` → `pine` → `indicators` → `engine`; `sim` → `trading` → `engine`; `control-local`, `control-host` → `control`; the table is `guards/src/graph.rs`. Inside a crate too: `guards/src/cycle.rs` fails a module cycle.
- **Leaves stay leaves** — nothing depends on `app`, `backtest`, `mcp`, `guards`.
- **Everything below `app` is headless** — no UI, no network, no async, no wall clock. That is `anchored-studies`, `backpressure`, `chart`, `chart-interaction`, `engine`, `orderbook`, `orderflow`, `trading`, `control`, `control-local`, `control-host`, `control-schema`, `operability`, `indicators`, `indicator-session`, `pine`, `replay`, `sim`, `sources`, `stores`, `paper`, `civil`, `layers`, `workspace` and `strategy`, and it binds third-party crates; `guards/src/headless.rs` scans all but the network. `replay` and `strategy` are told the time, never read it. `backtest` and `mcp` are headless too; `backtest`'s one wall-clock read is its `main.rs` stopwatch, to stderr only.
- **`feed` and the `feed-*` crates are the exception** — `feed` owns the runtimes, threads and clock, the venues stamp arrival; neither crosses `FeedEvent`.
- **`feed-binance`, `feed-hyperliquid` and `feed-mt5` never depend on each other**, and never on the script language. A feed produces trades.
- **`guards` has no dependencies at all** — its `dependencies` tables stay empty.
- **Replay is a source, not a chart mode** — same `FeedEvent` channel a live venue uses. UI gates on `FeedCapabilities`, never on "is it a replay?".
- Feeds and symbols come from config (`crates/app/config/feeds.toml`, `QUANTICK_CONFIG` or `./quantick.toml`), never code.

## Non-negotiable design rules

- **Determinism** — same trades in, same bars out. In the engine: no wall clock, no randomness, no iteration-order-dependent output (`BTreeMap`/`Vec` over `HashMap`). Golden tests over fixed fixtures.
- **One engine, three consumers** — chart, backtest and bot share the aggregator. Never fork bar-building per consumer.
- **Data honesty** — inferred or incomplete data is labelled, never silently patched: a depth reduction is an "unattributed L2 reduction", never a cancellation.
- **Small and focused** — not a trading platform. Build, show and expose bars.
- **Operable without a hand** — no capability ships reachable by mouse alone: a named call, a readable result, a registry entry. Gate: `arch-review`'s *The second operator*.
- **English is the repository's language** — identifiers, comments, doc comments, log/error/panic messages, UI strings, test names, assertion text, `.pine` scripts, config comments, everything under `docs/`, and branch names, commit messages and PR titles and bodies. Sessions with the trader happen in any language; the rule starts where something lands in the repo. Four exemptions, each where the foreign text *is* the data or the name: proper names of real people and products (`López de Prado`; B3's *mini índice* / *mini dólar*); localisation resources and language-detector word lists; a fixture reproducing text a real system emits; a marked, attributed quotation. The code, comment and test name around one stay English. Pre-existing lines are grandfathered — the finding is a line a diff *authors*. This bullet is the rule's single owner: `arch-review` dimension 8 grades it, `crates/guards/src/language.rs` enforces the mechanical half.

## Keeping the trunk small

- **A capability docks as a new file plus one registration line** — not a field, an init, a draw call and a hotkey in `QuantickApp`. No port to dock against? Build one: `.claude/skills/new-extension/SKILL.md`. A surface that moves out takes its tests with it.
- **The size ratchet** (`crates/guards/src/size.rs`, production lines) caps a file at 1,500 lines; `size-baseline.txt` holds signed exceptions under a budget, so raising one lowers another in the same change. No growth past a ceiling, nor over 200 lines below one — `--tighten` writes the new number.
- **UI-free code in `app` is ratcheted** — `guards/src/ui_free.rs` caps `crates/app` lines that never name `egui`/`eframe`; such code belongs in a headless crate.
- **Instructions are ratcheted too** — this file, `AGENTS.md`, and Markdown under `.claude/skills/`, `.agents/`, `docs/campaign/` and `docs/workflow/` (not goal files or evidence), in `context-baseline.txt`: over 10,000 bytes a file needs a signed ceiling, and the budget counts every file, so only deleting prose buys room. A `SKILL.md` states each rule that decides an outcome once, operatively; detail goes to its `references/`, reasoning to `docs/agentic-development.md`.

## Workflow

- Engine code is test-first: fixture trades plus expected bars, then implement until green.
- Task branches `feat/` `fix/` `docs/`; integration branches `campaign/`. Conventional commits, imperative, English.
- **One goal, one worktree** — cut from updated `main` (campaign children: the base in [the integration contract](docs/campaign/integration.md)) under `../quantick-worktrees/`, never in the main checkout or a shared writer's tree, and arm the guards:

  ```sh
  git fetch origin
  git worktree add -b <prefix>/<slug> ../quantick-worktrees/<prefix>-<slug> origin/main
  cd ../quantick-worktrees/<prefix>-<slug> && cargo build -p quantick-guards
  ```

  After the merge, from the main checkout: `git worktree remove ../quantick-worktrees/<prefix>-<slug>` then `git branch -d <prefix>/<slug>`.
- **One mission, one tier** — `/mission` (Codex: `$mission`) takes `small` (default), `medium`, `high` or `max`; the skill owns the table, and `small` is the only tier the hooks see.
- **All review evidence before readiness** — `arch-review` with its bug pass, `ai-review` on the draft PR, then `delivery-review` last; each skill's producer publishes a durable current-review report before recording its private projection, and a marker alone is not evidence. `small` skips only delivery review, within its diff-size ceiling.
- **Only the user merges to `main`.** Agents merge only into an explicitly authorized campaign branch under the integration contract — no auto-merge, merge queue, direct integration push or protection bypass. Hand off a ready PR.
- **Two phases, and the PR carries the state.** Phase one ends at a draft PR with resolvable `ai-review` findings; phase two repairs them from fresh context, redesign included. [The delivery contract](docs/workflow/delivery.md) owns reconciliation, finding identity, bounded repair and delta follow-ups; no unresolved required finding passes by exhaustion or re-stamped markers.
- **Name the subagent model at the call** — retrieval `haiku`, checklist application `sonnet`, open judgement (bugs, docking, design) the default strong model.
- **Hooks enforce these** — a write to the main checkout on `main` is denied; readiness and merge need exact-diff projections, matching durable reports and zero open AI-review threads; mission and ship completion run the final verifier (exact-head green CI, literal *What done means* reconciliation), even on an already-ready PR.
