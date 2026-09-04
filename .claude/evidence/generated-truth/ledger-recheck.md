# Evidence ledger re-verification (A12, R19)

The brief measured its ten claims at `bc39248`. `origin/main` is still at
`bc39248`, so this branch's base is the exact tree the audit read — the numbers
could only have aged if the audit misread them, not if the tree moved. Each
line below was re-checked anyway, with the command that produced the verdict.

Verdicts: **HIT** — the claim stands as written. **AGED** — the defect is real
but the cited numbers or locations have moved. **FALSE** — the claim does not
survive re-reading.

| # | Verdict | What re-verification found |
| --- | --- | --- |
| 1 | HIT | `docs/control-plane/README.md:38-44` §Precedence: "If they disagree, implementation stops until the documents are reconciled in a reviewed change. Source code, generated schemas, and tests become authoritative only after their corresponding implementation pull request lands." Echoed at `docs/README.md:3-8`: "the plan and the contract outrank the code until a reconciling change lands, which is the opposite of the usual default." |
| 2 | HIT | `docs/control-plane/roadmap.md:28` says PR 5c "open, base `main`"; `:29` says PR 6 "not started". Both shipped: `crates/app/src/control/contract.rs:548` documents `evidence.capture` and `:583` `evidence.read`, and `crates/app/src/control/trade.rs:71` defines `PLACE_CAPABILITY_ID = "trade.order.place"`. |
| 3 | AGED | The direction is right, the arithmetic is not reproducible as stated. `crates/app/src/control/` holds **26** `*_CAPABILITY_ID: &str` constants. `capability-inventory.md` mentions **101** distinct dotted identifiers in backticks, not the 88 the brief cites — the brief's count evidently used a narrower pattern. The baseline is genuinely stale: the file declares itself measured at `fcd2ac4`, which is 476 commits behind. The defect stands; the mission generates the file rather than recounting it, so the exact delta is what A4 will report, not a number carried in from the audit. |
| 4 | **FALSE** | No scope item depended on this line, so nothing else moves. The three "documented MCP tools that do not exist" are Rust **crate paths**, not tool names: `quantick_control::limits` at `docs/control-plane/roadmap.md:236` and `:340`, `quantick_pine::compile` at `:345`, `quantick_replay::context_path` at `:648`. `quantick_delete_everything` is a string literal at `crates/mcp/src/server.rs:470`, inside `#[cfg(test)] mod tests` (opened at `:280-281`), in an assertion that an **unknown** tool name comes back `INVALID_PARAMS` — the opposite of a tool that exists. The adapter's real tool set is the 15 names in `crates/mcp/src/tools.rs`. |
| 5 | HIT | `.claude/skills/ui-harness/references/hook-registry.md:59` writes `QUANTICK_DRAWING_MANAGER=1` (singular) in the prose of the `QUANTICK_CONTROL_ANNOTATE` row. The code reads `QUANTICK_DRAWINGS_MANAGER` at `crates/app/src/surfaces/drawing_chrome/mod.rs:1060`, and the registry's own row at `:82` spells it plural. One prose cell disagrees with the row two lines of the same file get right — exactly the class of defect a generated file cannot hold. |
| 6 | HIT | The four cited files read `QUANTICK_FOOTPRINT`, `QUANTICK_FOOTPRINT_SETTINGS`, `QUANTICK_FOOTPRINT_STYLE` (`footprint_config.rs`), `QUANTICK_INDICATORS_DIR` (`indicators/library.rs`), `QUANTICK_FAKE_STORE` (`workspace_bundle.rs`), and `QUANTICK_DEFAULT_FEED`, `QUANTICK_DEFAULT_SYMBOL`, `QUANTICK_LOG_FORMAT`, `QUANTICK_WINDOW_SIZE`, `QUANTICK_CONFIG` (`main.rs`). The exact count of undocumented hooks is what A5's guard will report; the mission does not carry the audit's nine forward as a number. |
| 7 | HIT | `.claude/skills/ui-harness/SKILL.md:45-46`: "`crates/app/src/harness.rs` owns every hook the window reads at launch". `harness.rs` names 26 distinct `QUANTICK_*` variables; `crates/app/src` as a whole names 130. |
| 8 | HIT | `.claude/skills/new-extension/SKILL.md:21` gives the indicator row as "`indicators` crate, or a `.pine` script compiled by `pine`", and `crates/indicators/src/native/mod.rs:9-11` states "a third native is a new file plus one line". `SavedKind` appears at 34 non-test sites across five files under `crates/app/src`: `app.rs`, `app/layout_wiring.rs`, `indicators/preset_file.rs`, `indicators/state_file.rs`, `layouts.rs`. The brief said ~31 sites across six files; the shape is confirmed, the exact site list is A10's to produce from the last real indicator addition rather than from a grep. |
| 9 | HIT | The magnitude differs from the brief, the defect does not. `CLAUDE.md`'s headless bullet names "the feeds" among the crates with "no UI, no network, no async, no wall clock". The three feed crates hold 190 `.await` points (96 binance, 51 mt5, 43 hyperliquid) and 89 `async` items; the brief's "357 async sites" is neither of those counts, so it is not reproducible as written. The three clock reads are exact: `crates/feed-binance/src/depth/stream.rs:665` and `crates/feed-mt5/src/stream.rs:2058` both `SystemTime::now()`, `crates/feed-hyperliquid/src/candles.rs:414` `tokio::time::Instant::now()`. |
| 10 | AGED | One of the three documents was already fixed. `CLAUDE.md:13`, `AGENTS.md:207` and `CONTRIBUTING.md:39` all state `cargo clippy --workspace --all-targets` with no `-D warnings`, and `CONTRIBUTING.md:44` explains the absence: "There is no `-D warnings` on that clippy line, and its absence is deliberate." The stale prescriptions that remain are `docs/mcp-control-plane-development-plan.md:1309` and `docs/indicator-system-plan.md:683`. CI at `.github/workflows/ci.yml:115` matches the three current documents. |

## Commands

```sh
sed -n '35,50p' docs/control-plane/README.md
sed -n '1,12p'  docs/README.md
sed -n '20,35p' docs/control-plane/roadmap.md
sed -n '1,12p'  docs/control-plane/capability-inventory.md
grep -rn 'evidence\.capture\|evidence\.read' crates/app/src/control/contract.rs
grep -rn 'trade\.order\.place'               crates/app/src/control/trade.rs
grep -rn '_CAPABILITY_ID: &str' crates/app/src/control/ | wc -l
grep -oE '`[a-z_]+\.[a-z_.]+`' docs/control-plane/capability-inventory.md | sort -u | wc -l
grep -rn 'quantick_control\b\|quantick_pine\|quantick_replay' docs/
grep -rn '"quantick_[a-z_]*"' crates/mcp/src/tools.rs
sed -n '455,480p' crates/mcp/src/server.rs
grep -rn 'QUANTICK_DRAWINGS\?_MANAGER' crates/app/src/ .claude/skills/
grep -oE 'QUANTICK_[A-Z0-9_]+' crates/app/src/harness.rs | sort -u | wc -l
grep -rhoE 'QUANTICK_[A-Z0-9_]+' crates/app/src --include=*.rs | sort -u | wc -l
grep -rn 'SavedKind' crates/app/src --include=*.rs | grep -v '/tests/' | wc -l
grep -rho '\.await' crates/feed-*/src | wc -l
grep -rn 'SystemTime::now\|Instant::now' \
  crates/feed-binance/src/depth/stream.rs \
  crates/feed-mt5/src/stream.rs \
  crates/feed-hyperliquid/src/candles.rs
grep -rn 'clippy' CLAUDE.md AGENTS.md CONTRIBUTING.md
grep -n  'clippy' docs/mcp-control-plane-development-plan.md \
                  docs/indicator-system-plan.md .github/workflows/ci.yml
```

## What the re-check changed about the mission

- **R21 withdrawn** on line 4. It is struck through on the ledger in
  `GOAL.md` with this reasoning; no acceptance criterion cited it.
- **S6 recorded** on line 10. One clippy command already stands across the
  three documents the acceptance criteria name, so A10 grades that half as
  already-true and fixes only the two plan documents.
- **Lines 3, 6 and 8 are not carried forward as numbers.** The audit's 79/88,
  its nine, and its ~31 were each unreproducible by the pattern the brief
  implies. This costs the mission nothing, because generating a file and
  guarding it produces the true delta as output — which is the whole argument
  of the mission, applied to its own brief.
