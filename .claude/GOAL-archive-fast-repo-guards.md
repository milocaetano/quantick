# Goal — the repository guards must not cost a build

Archived at the end of the branch `refactor/fast-repo-guards`, before either
pre-PR review ran, so both markers cover the branch the reviews saw.

The session did not start from `/mission` or from an issue: it started from a
retrospective on where a previous branch's time had gone, in which the size
ratchet came up as the second-largest avoidable cost. This file records the
request as it was actually made, so `delivery-review` has something to grade
against.

## Request ledger

Each ask is the trader's own, translated; the wording was Portuguese.

| # | Ask | Where it came from |
| --- | --- | --- |
| A1 | Explain what `size_guard` is | "sabe me dizer o que é o size_guard?" |
| A2 | It may be creating a bottleneck while developing — find a solution so it is invoked few times and costs development less | "isso pode estar criando um gargalo na hora de desenvolver... seja chamado poucas vezes e dar menos trabalho para desenvolvimento" |
| A3 | The point is to reduce coupling *while developing* | "A ideia é diminuir acoplamento quando desenvolve" |
| A4 | Development must be fast: guarantee uncoupled code **without** hurting development performance | "quero que o desenvolvimento seja rapido. Precisa encontrar um jeito de garantir codigo nao acoplado sem prejudicar a performance do desenvolvimento" |
| A5 | Scope: move all three guards, not only the size one | Answer to a scoping question |
| A6 | Include the advisory edit-time hook | Answer to a scoping question |

A4 is the constraint that shapes everything: the guarantee is not to be
traded away for speed. Both halves must hold at once — so nothing here
weakens a guard, and the whole gain has to come from removing cost that was
never buying anything.

## Acceptance criteria

| # | Criterion | Evidence |
| --- | --- | --- |
| C1 | The guards answer in seconds, not minutes | Measured before and after on a warm `target/`: 4m02s → 0.9s, with the guards' own work at 5.08s of the former |
| C2 | A single file can be checked without a build | `--file` measured at 39ms |
| C3 | No rule softened: every threshold, keyword and grandfathered path survives | `THRESHOLD`, `SLACK`, `KEYWORDS`, both `ALLOWED` lists and all 18 ceilings carried across; diffed against `origin/main`'s table |
| C4 | CI and enforcement unchanged | `cargo test --workspace` still runs all three; `.github/workflows/ci.yml` needed no edit |
| C5 | Raising a ceiling stays a visible, signed decision | Growth still fails hard and asks for a comment; only the shrink direction is automated |
| C6 | The edit-time hook never blocks, never invokes cargo, never delays an edit | `guard-watch` is `PostToolUse`, runs `target/debug/` directly, exits silent with no binary; six cases in `guardrails_test.sh` |
| C7 | The new crate is registered everywhere the repo checks | `Cargo.toml` members, `workspace_deps.rs` `ALLOWED`, `CLAUDE.md` crate map and dependency direction, `AGENTS.md` table |
| C8 | The four verification checks pass | fmt, clippy, build clean; `cargo test --workspace --no-fail-fast` 88 targets green |
| C9 | The hook suite passes | 45/45 |

## Deferrals

- **`crates/pine/tests/workspace_deps.rs` stays where it is.** It is the same
  family as the three moved here — a repository guard rather than a test of
  `pine` — and moving it would let `cargo test -p quantick-guards` answer the
  dependency-direction question too. It was left out because the trader scoped
  this branch to three guards, and because `pine` is cheap to build in a way
  `app` is not, so the payoff is much smaller. Worth its own change.
- **One known-red test.** `the_bridge_paging_tests_pass` fails in this
  environment for a reason predating the branch: the `python3` on PATH is a
  Windows Store alias. The Python it wraps passes 31/31 when run directly, and
  this branch does not touch `feed-mt5`.
