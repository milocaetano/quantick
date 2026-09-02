# quantick

Real-time alternative bar charts (tick / volume / dollar / imbalance bars) for order flow trading. One deterministic Rust engine feeds chart, backtest and bot.

Authoritative for working rules — each stated once, operatively. The reasoning lives in `docs/agentic-development.md`; the crate map, dependency graph and MCP control plane in `AGENTS.md`; the gate mechanics in `.claude/hooks/README.md`; everything else indexed by `docs/README.md`.

## Commands

Between edits: `cargo check -p <crate>`, `cargo test -p <crate> <filter>`, `cargo test -p quantick-guards`. Before every commit, all four — `check` never stands in for `clippy`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

CI runs those four plus what cargo cannot see — `sh .claude/hooks/guardrails_test.sh`, `ruff check --select F` over `tools/mt5/` and `bridge/mt5/`, `python3 tools/mt5/test_export_session.py`, `python3 bridge/mt5/tests/test_*.py`. Run the ones your change touches, watch with `gh pr checks <n> --watch`; red CI never merges.

## Architecture

Crates under `crates/`; `AGENTS.md` *The map* owns the descriptions and the graph. The invariants:

- **Dependency direction is one-way; never add a reverse edge.** `app` → `pine` → `indicators` → `engine`; `sim` → `trading` → `engine`; `control-local` → `control`.
- **Leaves stay leaves** — nothing depends on `app`, `backtest`, `mcp` or `guards`.
- **Pure domain crates stay pure** — `engine`, `orderbook`, `trading`, `control`: no UI, network, async or wall clock. `replay` and `strategy` are *told* the time.
- **Feeds never depend on each other**, and never on the script language.
- **`guards` has no dependencies at all**; nothing may be added to its manifest.
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
- **Arm the guard in a fresh worktree** — with no `target/` the `PostToolUse` hook reports nothing at all, not "clean". `cargo build -p quantick-guards` costs seconds; it is advisory and gates nothing.

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
- **One mission, one tier** — `/mission` takes `small` (the default), `medium`, `high` or `max`, scaling the ceremony and whether `delivery-review` runs. `small` is the only tier the hooks see; the skill owns the table.
- **Arch-review before PR** — over `git diff origin/main...HEAD` (the remote ref deliberately: `main...HEAD` in a worktree credits other branches' merged work to this one). Its step 0 runs `code-review` on the same diff. Resolve every Blocker and Should-fix; note deferrals in the PR body. A docs/skills change waives shape dimensions 1–7 and 9, never 8 and never step 0.
- **Delivery-review before PR** — arch-review asks whether the branch is well built, never whether it is what was asked for. Runs last, over the branch as shipped, grading every ask in the goal file (`.claude/GOAL.md` or its `GOAL-archive-<slug>.md`, committed before either review) and every acceptance criterion as DELIVERED / PARTIAL / MISSING / UNPROVEN, from a fresh-context subagent. Passes only when nothing is unmet. A `small` mission is exempt, bounded by a diff-size ceiling that revokes the exemption if the branch grows. A branch not from `/mission` is graded against its issue's acceptance criteria, and the verdict says so.
- **The review chain has a budget: three rounds per branch**, then the remainder ships as recorded PR follow-ups. A round is one pass by everything that owes the branch a review — arch-review's bug pass and shape pass, then delivery-review — plus the commit answering them; not three per skill, since a fix commit stales both markers and re-runs both. Nothing is discarded to fit: findings defer into the PR body with their severity. An open Blocker never defers, and if it runs the budget out the branch goes to the trader. On reaching the budget, say the shape — findings shrinking is convergence, findings flat or climbing into the last round's code is a design problem.
- **Subagents are routed by model, named at the call** — retrieval `haiku`, applying someone else's checklist `sonnet`, open judgement (bugs, docking, design) the default strong model. Omitting the field inherits the caller's and bills retrieval at open-judgement rates. Today `delivery-review`'s criteria pass is the only routed site; this is the standard the next one meets.
- **The worktree rule and both reviews are enforced by hooks, not memory** — a write to the main checkout on `main` is denied, and `gh pr create` is gated on both reviews having run for the exact diff being shipped (hashed, so a rebase does not invalidate a valid review); on arch-review alone for a `small` mission within its ceiling. The tier is read, never required. `.claude/hooks/README.md` owns the mechanism.
