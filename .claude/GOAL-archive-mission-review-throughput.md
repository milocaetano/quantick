# Mission — arm the cheap half of the workflow

**Tier:** `small`. Documentation only, well under the ceiling, no compiled code.

Be exact about what that buys, because the branch is a set of instructions and
instructions only run if someone follows them. Nothing here *forces* the guards
binary to exist: `guard-watch` still treats absence and cleanliness as the same
observable, and a `cargo clean` returns a worktree to silence with no signal.
What the branch changes is that the arming step now appears in all three places
a worktree is created, instead of nowhere. Follow-up 6 is the version that does
not depend on being read, and it needs an authorisation this session did not
have.

## Objective

Cut the wall-clock cost of a `/mission` without removing a review, by fixing
the two places where the flow's *cheap* half was switched off: the edit-time
guard hook never fires, and the fast build loop has no name anywhere in the
repository.

## Why it matters

The trader's complaint is that missions take hours and days, and that the
review chain never ends. Five agents analysed the flow. The measurement
refuted the obvious diagnosis: across the last 20 merged PRs, ordinary code
branches average **~1 review-fix commit**. The branches that burn rounds
(21 commits / 6 rounds; 9 / 3) are the ones with **zero production lines** —
meta-work on the workflow itself. The gate is not what is slow for coding.

What is slow is that the coding phase has no support:

| Measured in this checkout | Value |
| --- | --- |
| `target/debug/quantick-guards` present | **no** |
| Live worktrees, each with its own `target/` | **18** |
| `cargo check` in every `.md` in the repo | **0 occurrences** |
| `cargo build -p quantick-guards` (dependency-free) | **1.80s** |

The `guard-watch` hook runs `target/debug/quantick-guards` and is documented as
"silent when the binary has not been built". The binary is not built, and a
fresh worktree never builds it — so the hook that exists to report a crossed
size ceiling at the edit that caused it has been reporting **nothing**, for
every mission. The repository's cheapest structural check was off in exactly
the phase it was designed for, and the cost went to the reviews at the end.

That is also the answer to the modularity question. `delivery-review` was added
in response to `crates/app` reaching 72% of the repository — but it grades
conformance to the request ledger and never looks at file growth. The
mechanism that moved that number is the size ratchet (`QuantickApp` fields
133 → 97), and it is mechanical, costs ~1s, and fires at edit time. The lesson
is that a structural property is enforced most cheaply at the point where the
code is written, not at the PR.

## Request ledger

- **R1** — the flow is too slow; missions take hours and days.
- **R2** — keep the low-coupling quality while doing it. *"manter a
  performance de entrega rapida e mantendo qualidade de baixo acoplamento"*
- **R3** — stop the chains of reviews that never end and burn tokens.
- **R4** — improve the AI engineering workflow, not the product code.

## Acceptance criteria

- [x] **A1** — the repository names its edit-time loop, distinct from the four
      gate checks. *Evidence:* `CLAUDE.md` *Verification loop* states
      `cargo check -p <crate>` as the loop and the four as the gate.
      → `CLAUDE.md` *(R1, R4)*
- [x] **A2** — a fresh worktree arms `guard-watch` before the first edit, so
      the size ratchet reports while the code is being written. *Evidence:*
      `mission` step 6 carries `cargo build -p quantick-guards`; measured at
      1.80s in this worktree, and `./target/debug/quantick-guards` then exits 0.
      → `.claude/skills/mission/SKILL.md` *(R2, R4)*
- [ ] **A3** — `delivery-review` starts at its cheapest shape rather than its
      most expensive, without losing the fresh-context stranger.
      **WITHDRAWN, not delivered.** A *Cost discipline* section was written and
      then reverted after step 0 returned six findings against it, all tracing
      to one omission: the section was added without editing steps 2, 4 and 5,
      so it argued against five passages that still stood. The worst was not a
      contradiction but a new hole — grading from `branch.stat` plus the files
      as they stand lets a reviewer quote a sentence that was already on
      `origin/main` and mark the criterion DELIVERED, which is a false pass the
      full diff was the only input able to prevent. Making it correct means
      rewriting three steps of that skill; that is its own branch, not a
      paragraph bolted onto this one. *(R1, R3 — carried forward, undischarged)*

## Not applicable, and why

- **Hot path / performance evidence** — no compiled code changed.
- **`ui-harness`, `visual-qa`, `trader-ux-review`** — no surface changed.
- **`new-extension`** — no capability added.

## Deferral requested — NOT granted

**The second marker.** The largest single source of the "reviews behind
reviews" is that `arch-review-ok` and `delivery-review-ok` both key on the same
branch diff, so any fix commit stales *both* and a finding from the later
review sends the branch back through the earlier one from scratch. The fix is
one marker, with `delivery-review` running first and cheaply and `arch-review`
recording the marker last over the final branch.

It is **not in this branch.** Implementing it means editing
`.claude/hooks/guardrails.sh` to remove a gate, and the session's auto-mode
classifier denied those edits — correctly, since it is a guardrail being
loosened by an agent. The half-applied edit was reverted; `guardrails.sh` is
untouched on this branch and `guardrails_test.sh` passes 96/0.

This needs the trader's explicit authorisation. It is recorded here so the next
session finds the analysis rather than re-deriving it.

## Recommended follow-ups, not in this branch

1. **One marker** (above) — the biggest remaining win.
2. **Collapse the four tiers to two.** `pr-gate` can see exactly one
   distinction, and `medium` vs `high` differ at the gate by nothing. Four
   names for one cliff, and it quadruples the leading-word misparse surface
   `mission` already admits it cannot close. No archive sampled declares a tier
   at all.
3. **Fix the size measurement's pathspec.** `guardrails.sh:281` uses `-- .`,
   which is cwd-relative: `cd <worktree>/crates/app && gh pr create` measures
   only that subtree and stops matching `SIZE_EXCLUDES`, which is anchored at
   `.claude/`. Should be `-- :/` with `:(exclude,top)`.
4. **A generated `codebase-map`.** Nothing maps inside `crates/app`, where 168
   files and every large one live. Every file already opens with a `//!` line;
   a `--map` mode on the dependency-free guards binary would emit the index and
   keep it from going stale.
5. **A shared cargo target directory.** Every worktree holds its own 12–16 GB
   and every mission starts from a cold dependency graph. **Do not do this
   without changing `guard-watch` first.** `guardrails.sh:582` hardcodes
   `binary="$root/target/debug/quantick-guards"`, where `$root` is the worktree
   toplevel. Redirecting `CARGO_TARGET_DIR` moves the binary out from under that
   path, `[ -x "$binary" ] || exit 0` goes silent again — indistinguishably from
   clean — and every worktree loses the arming this branch just added, at once.
   The build-time win would silently buy back the exact defect this branch
   fixed. It is also why the change was not shipped here as a tracked
   `.cargo/config.toml`: CI caches `./target` via `Swatinem/rust-cache@v2` and
   would miss the cache too.
6. **Make `guard-watch` report its own absence.** The deepest fix, and the one
   that makes 5 safe. Today absence and cleanliness are the same observable, so
   a `cargo clean` or a wiped `target/` returns a worktree to silence
   mid-mission with nothing to notice. One `context` line the first time the
   hook finds no binary, naming the build command, turns a property every agent
   in every worktree has to remember into one the mechanism reports about
   itself. It touches `guardrails.sh`, so it needs the same authorisation as
   the marker change above.

## The request as received

Quoted verbatim, in the trader's own language, under `CLAUDE.md`'s exemption
for a marked and attributed quotation: the wording carries the ask, and
translating it would make this section a paraphrase of the thing it exists to
preserve.

> A gnt tem fluxo de trabalho com IA. Hoje eu utilizo a Mission. /mission Esse
> fluxo tem o objetivo de manter um código mais modular, com o acoplamento
> possível. Daí eu vi que ele não estava seguindo as regras do arquivo app tava
> ficando gigante com 70% das linhas de codigo. Então eu crei outros skill
> delivery review e pedi para nao acomplar mais com arquivos gigtantes. O
> probleam o fluxo ficou pesado demorando horas e dias para terminar tarefas.
> Isso prejudica nosso trabalho. Eu quero manter a performance de entrega
> rapida e mantendo qualidad ede baixo acoplomanteo. A gnt a se prendendo em
> vários reviews em cadeia que nao terminam nunca além de queimar ainda mais
> tokens. O objetivo é melhroar os fluxo de engenharia de IA de como usamos Ia
> para trabalho e não para melhorar o codigo. O codigo quem vai fazer isso
> serão os agentes duranteo desenvovimento

> podemos manter o delivery review mas precisa ser menos gastgante mesmo
> reviravolta...
