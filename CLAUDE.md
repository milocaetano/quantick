# quantick

Real-time alternative bar charts (tick / volume / dollar / imbalance bars) for order flow trading. One deterministic Rust engine feeds chart, backtest and bot.

Authoritative for working rules — each stated once, operatively. The reasoning lives in `docs/agentic-development.md`; the crate map, dependency graph and MCP control plane in `AGENTS.md`; the gate mechanics in `.claude/hooks/README.md`; everything else indexed by `docs/README.md`.

## Verification loop (mandatory)

Between edits: `cargo check -p <crate>`, `cargo test -p <crate> <filter>`, `cargo test -p quantick-guards`. Before every commit, all four — `check` never stands in for `clippy`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo build --workspace
cargo test --workspace
```

`cargo test -p quantick-guards` runs the guards in about a second — no dependencies to build, so ask it after a batch of edits, not at the end of a suite. The `PostToolUse` hook runs the same binary per edited file; advisory, gates nothing, and in a fresh worktree reports nothing at all — not "clean", *nothing* — until `cargo build -p quantick-guards` has run there.

`cargo run -p quantick-guards -- --report` prints the numbers a refactor is judged on — lines per crate, largest files, widest structs, each ratchet against its budget — deterministically, so two runs diff into what a merge changed.

CI runs those four plus what cargo cannot see — `sh .claude/hooks/guardrails_test.sh`, `ruff check --select F` over `tools/mt5/` and `bridge/mt5/`, `python3 tools/mt5/test_export_session.py`, `python3 bridge/mt5/tests/test_*.py`. Run the ones your change touches, watch with `gh pr checks <n> --watch`; red CI never merges.

## Architecture

Crates under `crates/`; `AGENTS.md` *The map* owns the descriptions and the graph. The invariants:

- **Dependency direction is one-way; never add a reverse edge.** `app` → `pine` → `indicators` → `engine`; `sim` → `trading` → `engine`; `control-local` → `control`; the table is `guards/src/graph.rs`. Inside a crate too: cargo cannot see a module cycle, so `guards/src/cycle.rs` fails the build on a new one.
- **Leaves stay leaves** — nothing depends on `app`, `backtest`, `mcp` or `guards`.
- **Everything below `app` is headless** — no UI, no network, no async, no wall clock. That is `engine`, `orderbook`, `orderflow`, `trading`, `control`, `control-local`, `indicators`, `pine`, `replay`, `sim` and `strategy`, and it binds third-party crates too; `guards/src/headless.rs` scans all but the network. `replay` and `strategy` are *told* how much time passed rather than reading a clock. `backtest` and `mcp` are headless too; `backtest`'s only wall-clock read is the stopwatch in its `main.rs`, whose numbers reach stderr and never a report.
- **`feed` and the `feed-*` crates are the exception** — `feed` owns the runtimes, threads and clock, the venues stamp arrival; neither crosses the `FeedEvent` channel.
- **`feed-binance`, `feed-hyperliquid` and `feed-mt5` never depend on each other**, and never on the script language. A feed produces trades.
- **`guards` has no dependencies at all** — its `dependencies` tables stay empty.
- **Replay is a source, not a chart mode** — same `FeedEvent` channel a live venue uses. UI gates on `FeedCapabilities`, never on "is this a replay?".
- Feeds and symbols come from config (`crates/app/config/feeds.toml`, `QUANTICK_CONFIG` or `./quantick.toml`), never hardcoded.

## Non-negotiable design rules

- **Determinism** — same trades in, same bars out. In the engine: no wall clock, no randomness, no iteration-order-dependent output (`BTreeMap`/`Vec` over `HashMap`). Golden tests over fixed fixtures.
- **One engine, three consumers** — chart, backtest and bot share the aggregator. Never fork bar-building per consumer.
- **Data honesty** — inferred or incomplete data is labelled, never silently patched.
- **Small and focused** — not a trading platform. Build bars, show bars, expose bars to code.
- **Operable without a hand** — no capability ships reachable by mouse alone: a named call, a readable result, a registry entry. Gate: `arch-review`'s *The second operator*.
- **English is the repository's language** — identifiers, comments, doc comments, log/error/panic messages, UI strings, test names, assertion text, `.pine` scripts, config comments, everything under `docs/`, and branch names, commit messages and PR titles and bodies. Sessions with the trader happen in any language; the rule starts where something lands in the repo. Four exemptions, each where the foreign text *is* the data or the name: proper names of real people and products (`López de Prado`; B3's *mini índice* / *mini dólar*); localisation resources and language-detector word lists; a fixture reproducing text a real system emits; a marked, attributed quotation. The code, comment and test name around one stay English. Pre-existing lines are grandfathered — the finding is a line a diff *authors*. This bullet is the rule's single owner: `arch-review` dimension 8 grades it, `crates/guards/src/language.rs` enforces the mechanical half.

## Keeping the trunk small

- **A capability docks as a new file plus one registration line** — not a field, an init, a draw call and a hotkey in `QuantickApp`. No port to dock against? Build one: `.claude/skills/new-extension/SKILL.md`.
- **A surface that moves out takes its tests with it.**
- **The size ratchet enforces this** — `crates/guards/src/size.rs`, ceilings in `crates/guards/size-baseline.txt`. Production lines only, threshold 1,500; `tests/` untracked.
- **Teeth both ways** — no growth past a ceiling, and no sitting more than 200 lines below one. `cargo run -p quantick-guards -- --tighten` writes the new number when a file shrinks.
- **Growth is pay-as-you-go** — a raise must be signed in the baseline with a reason, and a budget caps the sum of all ceilings, so raising one means lowering another in the same change.

## Keeping the instructions small

The context ratchet covers this file, `AGENTS.md`, and Markdown under `.claude/skills/` or `.agents/`; goal files are excluded. Its ceilings are in `context-baseline.txt`, using the mechanism shared with `size` in `ratchet.rs`.

- **A `SKILL.md` states every rule that decides an outcome, once, operatively.** Reasoning, histories and per-dimension detail go to `references/` beside it, read on demand — a waived dimension then costs nothing. A working rule's reasoning goes to `docs/agentic-development.md`.
- **The budget is the whole tracked weight**, not just the ceilings: files over 10,000 bytes carry a signed entry, and every smaller one still counts. So splitting prose into sub-threshold files buys nothing — only deleting it does.

## Workflow

- Engine code is test-first: fixture trades plus expected bars, then implement until green.
- Branches `feat/` `fix/` `docs/`; conventional commits, imperative, English.
- **One goal, one worktree** — cut from updated `main`, under `../quantick-worktrees/`, never the main checkout, never shared between parallel agents. The last line below is not optional:

  ```sh
  git fetch origin
  git worktree add -b <prefix>/<slug> ../quantick-worktrees/<prefix>-<slug> origin/main
  cd ../quantick-worktrees/<prefix>-<slug> && cargo build -p quantick-guards
  ```

  After the merge, from the main checkout: `git worktree remove ../quantick-worktrees/<prefix>-<slug>` then `git branch -d <prefix>/<slug>`.
- **One mission, one tier** — `/mission` in Claude Code or `$mission` in Codex takes `small` (the default), `medium`, `high` or `max`, scaling the ceremony and whether `delivery-review` runs. `small` is the only tier the hooks see; the skill owns the table.
- **Both reviews before `gh pr ready`** — `arch-review` over `git diff origin/main...HEAD`, its step 0 running `code-review` on the same diff; resolve every Blocker and Should-fix, note deferrals in the PR body. A docs/skills change waives shape dimensions 1–7 and 9, never 8 and never step 0. Then `delivery-review`, last, over the branch as shipped and from fresh context: every ask in the goal file (`.claude/GOAL.md` or its `GOAL-archive-<slug>.md`, committed before either review) and every criterion graded DELIVERED / PARTIAL / MISSING / UNPROVEN, passing only when nothing is unmet. `small` is exempt from it within a diff-size ceiling; a branch not from `/mission` is graded against its issue, and says so.
- **Two phases, and the PR carries the state.** Phase one makes it work and ends at a draft PR, where `ai-review` posts each finding as its own resolvable thread. Phase two closes them one at a time, from fresh context, allowed to redesign — signatures included. Rounds are not counted; threads are. If the open set does not shrink between two runs, the branch goes to the trader.
- **Subagents are routed by model, named at the call** — retrieval `haiku`, applying someone else's checklist `sonnet`, open judgement (bugs, docking, design) the default strong model. Omitting the field inherits the caller's and bills retrieval at open-judgement rates. `delivery-review`'s criteria pass is the standard the next routed site meets.
- **The worktree rule and the reviews are enforced by hooks, not memory** — a write to the main checkout on `main` is denied; a draft PR opens ungated, while `gh pr ready` and `gh pr merge` want both markers for the exact diff (hashed, so a rebase keeps a valid review) and zero open `ai-review` threads. `small` needs arch-review alone, within its ceiling. `.claude/hooks/README.md` owns the mechanism.
