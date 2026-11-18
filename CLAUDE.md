# quantick

Desktop order-flow trading on one deterministic Rust engine.

Rule authority. Rationale: `docs/agentic-development.md`; map: `AGENTS.md`; gates: `.claude/hooks/README.md`.

## Architecture

`AGENTS.md` maps crate ownership and dependencies:

- **Dependency direction is one-way; no reverse edges.** `app` → `pine` → `indicators` → `engine`; `sim` → `trading` → `engine`; `control-local`, `control-host` → `control`; see `guards/src/graph.rs`. Inside a crate too: `guards/src/cycle.rs` fails a module cycle.
- **Leaves stay leaves** — nothing depends on `app`, `backtest`, `mcp`, `guards`.
- **Everything below `app` is headless** — no UI, no network, no async, no wall clock. That is `ui-model`, `alerts`, `appearance`, `anchored-studies`, `backpressure`, `chart`, `chart-interaction`, `drawings`, `engine`, `orderbook`, `orderflow`, `trading`, `control`, `control-local`, `control-host`, `control-handlers`, `control-schema`, `operability`, `indicators`, `indicator-session`, `pine`, `replay`, `session`, `sim`, `sources`, `stores`, `paper`, `civil`, `layers`, `workspace` and `strategy`, and it binds third-party crates; `guards/src/headless.rs` scans all but network. `replay` and `strategy` are told the time, never read it. `backtest` and `mcp` are headless, except `backtest`'s stderr stopwatch.
- **`feed`, `feed-*` and `broker-mt5` are the exception** — `feed` owns the runtimes, threads and clock, the venues stamp arrival; neither crosses `FeedEvent`. `platform` binds OS windows for `app` alone: safe calls, no egui or domain.
- **`feed-binance`, `feed-hyperliquid` and `feed-mt5` never depend on each other**, and never on the script language. A feed produces trades.
- **`guards` has no dependencies at all** — its `dependencies` tables stay empty.
- **Replay is a source, not a chart mode** — same `FeedEvent` channel a live venue uses. UI gates on `FeedCapabilities`, never on "is it a replay?".
- Feeds and symbols come from config (`crates/app/config/feeds.toml`, `QUANTICK_CONFIG` or `./quantick.toml`), never code.

## Non-negotiable design rules

- **Determinism** — same trades in, same bars out. In the engine: no wall clock, no randomness, no iteration-order-dependent output (`BTreeMap`/`Vec` over `HashMap`). Golden tests over fixed fixtures.
- **One engine, three consumers** — chart, backtest and bot share the aggregator. Never fork bar-building per consumer.
- **Data honesty** — inferred or incomplete data is labelled, never silently patched: a depth reduction is an "unattributed L2 reduction", never a cancellation.
- **Desktop and local** — no web app, mobile or cloud. A real-broker venue stays off until enabled in config.
- **Operable without a hand** — no capability ships reachable by mouse alone: a named call, a readable result, a registry entry.
- **English is the repository's language** — identifiers, comments, doc comments, log/error/panic messages, UI strings, test names, assertion text, `.pine` scripts, config comments, everything under `docs/`, and branch names, commit messages and PR titles and bodies. Sessions with the trader happen in any language; the rule starts where something lands in the repo. Four exemptions, each where the foreign text *is* the data or the name: proper names of real people and products (`López de Prado`; B3's *mini índice* / *mini dólar*); localisation resources and language-detector word lists; a fixture reproducing text a real system emits; a marked, attributed quotation. The code, comment and test name around one stay English. Pre-existing lines are grandfathered — the finding is a line a diff *authors*. This bullet is the rule's single owner: `crates/guards/src/language.rs` enforces the mechanical half.

## Keeping the trunk small

- **A capability docks as a new file plus one registration line** — not a field, an init, a draw call and a hotkey in `QuantickApp`. No port to dock against? Build one: `.claude/skills/new-extension/SKILL.md`.
- **Copy the target shape, not the code beside it** — a new state change is one command its headless owner executes (`session/src/paper_command.rs`), sent by MCP (`control-handlers/src/trade.rs`) and by UI in one line (`app/src/app/chrome/toolbar.rs`); `app` keeps fields, calls, input, egui. Legacy: `apply_menu_command_on`'s window arms, the `control/ports.rs` window traits. Owner tests decide; `app` tests wiring only.
- **The size ratchet** (`crates/guards/src/size.rs`, production lines) caps a file at 1,500 lines; `size-baseline.txt` holds signed exceptions under a budget, so raising one lowers another in the same change. Never exceed a ceiling or sit over 200 lines below it — `--tighten` writes the new number.
- **UI-free code in `app` is ratcheted** — `guards/src/ui_free.rs` caps `crates/app` lines that never name `egui`/`eframe`; such code belongs in a headless crate.
- **Instructions are ratcheted too** — `guards/src/context.rs` caps this file, `AGENTS.md` and Markdown under `.claude/skills/`, `.agents/` and `docs/workflow/`; only deleting prose buys room. A `SKILL.md` states each deciding rule once; detail goes to `references/`, reasoning to `docs/agentic-development.md`.

## Workflow

- Engine code is test-first: fixture trades plus expected bars, then implement until green.
- Task branches `feat/` `fix/` `docs/`; integration branches `feature/`. Conventional commits, imperative.
- **One goal, one worktree** — cut from updated target under `../quantick-worktrees/`, never in the main checkout or a shared writer's tree: `git fetch origin && git worktree add -b <prefix>/<slug> ../quantick-worktrees/<prefix>-<slug> origin/main`, then `cargo build -p quantick-guards` in it. After the merge, from the main checkout: `git worktree remove ../quantick-worktrees/<prefix>-<slug> && git branch -d <prefix>/<slug>`.
- **Process belongs to the skill that runs it** — verification, review and the PR flow live in `implement` (`feature` for integration branches); [`CONTRIBUTING.md`](CONTRIBUTING.md) holds the checks.
- **Only the user merges to `main`.** Agents merge only into an explicitly authorized integration branch under its contract — no auto-merge, merge queue, direct integration push or protection bypass. Hand off a ready PR.
- **Name the subagent model at the call** — retrieval `haiku`, checklist application `sonnet`, open judgement (bugs, docking, design) the default strong model.
