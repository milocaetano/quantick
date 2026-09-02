# GOAL — a ratchet for the context files, and the cut it makes possible

## Request (as made, in conversation)

> Voce como expert em engenharia de IA e Claude, para ter economia de tokens,
> com foco em entrega rapida de novos feature. Quero manter foco em
> modularidade de codigo. Que seja sempre testado, modular, extensivo e sem
> criar arquivos gigantes. Para que permita escalar. Quero que voce melhore
> ainda mais nossos arquivos de Claude que sao injetados no contexto. Menos
> aqui e mais. Veja o que ja foi feito e o que pode melhrorar ainda mais.
>
> — followed by `https://github.com/milocaetano/quantick/pull/279` and
> `isso no projeto quantick`, scoping the request to this repository and to
> the work PR #279 started.

Scope was then narrowed by the trader to: **the context ratchet plus the
three large skills.** The `ui-harness` hook registry is explicitly out.

## Request ledger

- **R1** — go further than PR #279 on the files injected into a session's
  context.
- **R2** — the result must cost fewer tokens.
- **R3** — keep it modular; no giant files.
- **R4** — keep it tested.
- **R5** — keep it extensible, so it scales rather than needing this cleanup
  again.
- **R6** — do not slow down feature delivery.

## What PR #279 left

`CLAUDE.md` is 9,047 bytes and lean. The weight moved to the skills, which
load whole when invoked:

| file | bytes |
| --- | --- |
| `.claude/skills/arch-review/SKILL.md` | 48,154 |
| `.claude/skills/mission/SKILL.md` | 33,527 |
| `.claude/skills/delivery-review/SKILL.md` | 27,282 |

A `/mission → /arch-review → /delivery-review → /ship` cycle injects about
28k tokens of instruction before a line of code is read. The structural cause
is that `crates/guards/src/size.rs` rations only `.rs` under `crates/`:
nothing has ever bounded a context file, which is why one reached 48 KB while
production code is held to 1,500 lines.

## Acceptance criteria

- **A1** (R1, R2, R5) — a second ratchet guards the files that enter a
  session's context, with recorded ceilings, a signed-raise rule and a total
  budget, exactly as the size ratchet does for code.
- **A2** (R3, R5) — the two ratchets share one mechanism. The baseline
  format, the verdicts, the budget rule and `--tighten` are written once, and
  a third ratchet is a policy constant rather than a copy.
- **A3** (R1, R2) — the three large skills are cut the way PR #279 cut
  `CLAUDE.md`: every operative rule kept and stated once, the reasoning moved
  out, and the per-dimension or per-step detail split into `references/` that
  load only when that dimension or step is in scope.
- **A4** (R1) — no rule is lost. Every pointer from another file still
  resolves, and every gate still names its owner.
- **A5** (R4) — the new guard is covered by tests in the crate, and
  `sh .claude/hooks/guardrails_test.sh` still passes.
- **A6** (R6) — the ceilings recorded are the sizes after the cut, so the
  ratchet starts tight rather than blessing today's weight.

## Injected gates

- **G1** — `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test
  --workspace`.
- **G2** — `cargo test -p quantick-guards` and `sh
  .claude/hooks/guardrails_test.sh`.
- **G3** — `arch-review` over `git diff origin/main...HEAD`, full shape pass:
  this branch changes Rust, so the docs waiver does not apply.
- **G4** — `delivery-review` against this file.
