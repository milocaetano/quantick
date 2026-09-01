# Mission — mission tiers

**Objective.** Give `/mission` a tier parameter (`small | medium | high | max`)
that scales the ceremony to the size of the task, and make `small` genuinely
cheap by exempting it from `delivery-review` at the `pr-gate` hook.

**Tier:** `high`. It edits the gate that guards every other
branch; the shape pass and both reviews apply in full.

**Why it matters.** Today every mission pays the same price: an interrogation
round, a full gate table, a `high`-effort bug pass and a fresh-context
conformance review. That price is right for a feature and absurd for a one-line
fix, so the flow gets skipped rather than scaled — and a skipped flow protects
nothing. A tier makes the cheap path an *official* path with its own recorded
gate, instead of an off-the-books one.

## Request ledger

| # | Ask | Source |
| --- | --- | --- |
| R1 | `/mission` accepts a tier as a parameter, with exactly the four names the trader gave: `small`, `medium`, `high`, `max`. | *"aceitar small medium high max como parmetro"* |
| R2 | The tier reduces the processing a mission costs — a lever on work done, not a label. | *"para evitar menor processamento e tudo"* |
| R3 | `small` does not run a long code review. | *"nao precisa de um code review lonog"* |
| R4 | `small` does not run `delivery-review` at all. | *"nem emsmo confirmar com um delivery review"* |
| R5 | **(purpose — judges the rest)** smaller tasks reach delivery quickly through a leaner flow. | *"algo mais enxuto para tarefas menores serem entregues logo"* |

## Decisions taken by the trader

- **D1** — `small` skips `delivery-review` outright, which requires changing
  `pr-gate` in `.claude/hooks/guardrails.sh` to read a tier declaration.
  Presented against two alternatives that left the hook intact (a cheap inline
  delivery-review, or a `small` that never opens a PR); the trader chose the
  hook change knowingly, with the cost stated.
- **D2** — the default tier, when `/mission` is called with no level, is
  `medium`. The rigour of today's flow becomes an explicit `/mission high`.
- **D3** — this change ships the normal way: worktree, branch, both reviews, PR.

## Assumptions

- **S1** — **the exemption is bounded by the shipped diff**, at 300 changed
  lines (insertions + deletions against `origin/main`). Not asked, and the
  closest thing here to inventing scope, so the reasoning is on the record: D1
  removes a gate the hooks' own README says was deliberately built without an
  override, because a skip file "hands the kill switch to precisely the caller
  with a motive to use it". Binding the word `small` to a measurable property of
  the branch is what stops that from recurring — an agent that declares `small`
  dishonestly at PR time gets the exemption only on branches where declaring it
  honestly would also have been allowed. The number is one constant with a
  comment, reversible in one edit; 300 lines sits comfortably above a fix or a
  doc paragraph and comfortably below anything carrying enough asks for a ledger
  to be worth grading.
- **S2** — the tier is the **leading token** of the argument, so
  `/mission small fix the axis labels` parses as tier `small`. An objective that
  genuinely begins with one of the four words is misread by this rule; the
  mitigation is that step 1 echoes the parsed tier and the kept objective in one
  line, making a misparse visible in the first turn rather than at the PR.
- **S3** — a tier may be **raised** mid-mission and never **lowered**. Lowering
  is the escape hatch D1 must not accidentally build; raising is what an honest
  mission does when the work turns out bigger than it looked.
- **S4** — `max` does not launch `code-review ultra` itself. The harness states
  that ultra is user-triggered and billed and that the agent must not attempt
  it, so `max` runs `high` and *tells the trader* the ultra option exists.
- **S5** — the tier file is `mission-tier` in the worktree's own git dir, beside
  the two markers: per-branch, never committed, same lifetime and same discovery
  path as the mechanism it modifies. It holds **`<branch> <tier>`**. The branch
  half was not in the first draft and the arch-review found why it had to be:
  the markers hold a sha and go stale when the branch moves, while a bare tier
  word outlives its mission, so a worktree reused for a second branch inherited
  the exemption and opened a PR ungraded. Reproduced, then closed.
- **S6** *(wanted to ask)* — whether `small` should also skip the four-check
  verification loop. Went with **no**: fmt/clippy/build/test are cheap when
  nothing compiled changed, and they are the only thing left standing between a
  `small` branch and `main` once `delivery-review` is gone.

## Acceptance criteria

- [x] **A1** — `/mission` accepts `small`, `medium`, `high` and `max` as the
      leading argument, and the skill states what each one changes.
      *Evidence:* the tier table in the skill, one row per step that scales, one
      column per tier. → `.claude/skills/mission/SKILL.md`. *(R1, R2)*
- [x] **A2** — with no tier given the mission runs at `medium`, and the skill
      says so where the argument is defined.
      *Evidence:* the default stated in the argument section.
      → `.claude/skills/mission/SKILL.md`. *(R1)*
- [x] **A3** — `small` runs the bug pass at `low` effort and limits the shape
      pass to the dimensions the diff touches, dimension 8 always included.
      *Evidence:* the tier-aware effort rule in step 0 and the shape-scope rule.
      → `.claude/skills/arch-review/SKILL.md`. *(R3)*
- [x] **A4** — a `small` mission opens its PR with **no** `delivery-review-ok`
      marker, and every other tier still cannot.
      *Evidence:* passing cases in the hook suite asserting both directions.
      → `.claude/hooks/guardrails_test.sh`. *(R4)*
- [x] **A5** — the `small` exemption cannot be taken by a branch that is not
      small: over the ceiling the full gate applies again.
      *Evidence:* a suite case whose branch exceeds the ceiling and is denied
      naming `delivery-review-ok`. → `.claude/hooks/guardrails_test.sh`. *(R4, S1)*
- [x] **A6** — the denial an agent sees when it has simply not run
      `delivery-review` is **unchanged**: it never names the tier file, the tier
      words, or any way to create them.
      *Evidence:* a suite case asserting that denial carries neither the file
      name nor the word `small`. → `.claude/hooks/guardrails_test.sh`. *(R4)*
- [x] **A7** — the tier is recorded where it cannot silently drift: the tier
      file the gate reads, a `**Tier:**` line in `GOAL.md`, and the PR body.
      *Evidence:* the recording step in the skill, plus a suite case pinning
      that `guardrails.sh` and `mission/SKILL.md` agree on the file name and the
      four words. → `.claude/skills/mission/SKILL.md`,
      `.claude/hooks/guardrails_test.sh`. *(R1, S5)*
- [x] **A8** — `small` also drops the interrogation round, the injected gate
      rows the diff does not touch, and the `/goal` handover, so the saving is
      real rather than a renamed review.
      *Evidence:* those rows in the tier table, each naming what is skipped.
      → `.claude/skills/mission/SKILL.md`. *(R2, R5)*
- [x] **A9** — a tier can be raised mid-mission and never lowered, and a `small`
      mission whose diff crosses the ceiling is told to raise it.
      *Evidence:* the stated rule, and the denial text for that case.
      → `.claude/skills/mission/SKILL.md`, `.claude/hooks/guardrails.sh`. *(S3)*
- [x] **A10** — every document describing this flow to a reader carries the
      tier: the hooks README, `CLAUDE.md`, `ship`, `delivery-review` and
      `docs/agentic-development.md`. A gate described in five places and changed
      in one is the drift this repo already has a test for.
      *Evidence:* the tier named in each file. → those five files. *(R1, R4)*

## Injected gates

- [ ] **G1** — every artifact in English (`CLAUDE.md` owns the rule and its
      exemptions). *Evidence:* `arch-review` dimension 8 clean.
      → the arch-review verdict.
- [x] **G2** — the four checks green after rebasing on latest `main`.
      *Evidence:* four exit-0 runs. → the PR body.
- [x] **G3** — `sh .claude/hooks/guardrails_test.sh` reports zero failures, and
      reports failures when `guardrails.sh` is neutered to `exit 0`.
      *Evidence:* both run outputs. → the PR body.
- [ ] **G4** — `arch-review` run with every Blocker and Should-fix resolved, or
      deferred in the PR body. *Evidence:* the verdict. → the PR body.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — the PR is open.

## Not applicable, and why

- **`visual-qa` and `trader-ux-review`** — nothing user-visible changes. This
  branch touches skills, hooks and docs; the desktop app is not compiled
  differently by any line of it.
- **`new-extension`** — no capability is added. The change modifies an existing
  gate rather than docking a new module at a port.
- **Hot-path performance evidence** — no shipped code path is touched. The hook
  runs once per `gh pr create`, and the two `git` calls the exemption adds run
  only on a branch that declared a tier.
- **The docs/skills shape waiver** — *deliberately not claimed in full*.
  `mission` waives shape dimensions 1–7 and 9 for prose, but says a shell script
  or a test shipping alongside the prose takes the full shape pass, and the
  substance of this branch is exactly that.

## Evidence

Recorded 2026-08-31, on `feat/mission-tiers` at commit `7b01b04`.

| # | Where it landed |
| --- | --- |
| A1, A8 | `## Tiers` in `.claude/skills/mission/SKILL.md`: an eight-row table, one row per step that scales, one column per tier. `small` reads *skipped* for the interrogation and the `/goal` line, *not run* for `delivery-review`, and narrows the injected gates to English plus the four checks. |
| A2 | *Argument* section of the same file: "With no tier given, the mission runs at **`medium`**." |
| A3 | `.claude/skills/arch-review/SKILL.md`, step 0: the tier overrides the effort default (`low` for `small`), and *The mission's tier scopes the shape pass* limits the dimensions read, keeping 8 and step 0 always. |
| A4 | `guardrails_test.sh`: *a small mission opens its PR on arch-review alone* (silent), *a small mission still cannot skip arch-review* (deny), and a loop over the tiers the script declares asserting *the `<tier>` tier still requires delivery-review* for each of `medium`, `high`, `max`. Plus *an unrecognised tier grants nothing*. |
| A5 | `guardrails_test.sh`: a second worktree built one line over `SMALL_TIER_MAX_CHANGED_LINES`, read from the script rather than hardcoded. Two cases — *a small mission that outgrew the ceiling pays in full*, and *is told the measured size that cost it the exemption*, which asserts the exact figure so a `declared_tier` that stopped recognising `small` cannot pass the first. |
| A6 | `guardrails_test.sh`, the block after the ceiling cases: it runs the gate on an untiered branch and fails if the denial contains the tier file name or the word `small`. Absence, checked directly, because `run` can only assert presence. |
| A7 | `mission` step 6 records the file; step 5 requires the `**Tier:**` line; the drift check at the foot of `guardrails_test.sh` fails unless `guardrails.sh` and the skill agree on the file name and on all four tier words, and the pre-existing check that every `absolute-git-dir)/…` name in the prose is one the gate reads was widened to cover it. |
| A9 | *A tier goes up, never down* in the skill; and in `guardrails.sh`, the over-ceiling denial says so in the message it hands back. |
| A10 | Named in `.claude/hooks/README.md` (new section *The `small` mission exemption*, plus both table rows), `CLAUDE.md` (new **One mission, one tier** bullet and two amended ones), `.claude/skills/ship/SKILL.md` step 4, `.claude/skills/delivery-review/SKILL.md` (new *When this skill does not run*), and `docs/agentic-development.md` (three passages). |
| G2 | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` — each run on its own, all exit 0. Two reds appeared and both are recorded flakes of this machine, not of this branch, which touches no Rust: one `quantick-app` bin test that passed on rerun, and `quantick-feed-mt5 --test bridge_paging`, whose Python was then run directly (`python bridge/mt5/tests/test_paging.py` — 31 checks, all passed). |
| G3 | `sh .claude/hooks/guardrails_test.sh` → **72 passed, 0 failed** (39 before this branch). Neutered to `exit 0` → **18 passed, 30 failed**, the suite still running to completion, then restored and re-run green. Six mutation runs besides, each reintroducing one defect a review found: every one turns red exactly the case written for it and no other — the branch-identity check, the unmeasurable size, the binary skip (two cases), the drift check that had stopped firing, the absence vocabulary, and the snippet format. |

**G1 and G4 are open here on purpose.** Both are verdicts of the `arch-review`
that runs immediately after this commit, so neither can exist while this file is
being written — the same reason `mission` keeps `delivery-review` and the PR out
of the criteria entirely. Their evidence is the review's own output, quoted in
the PR body. This is the closest thing on this branch to a criterion that cannot
be graded when the grading happens, and it is named rather than ticked.

**A note on the tier this branch ran at.** `high`, and the diff proves it was
right: 555 insertions and 27 deletions, 582 changed lines against a `small`
ceiling of 300. Had it declared `small`, the hook it adds would itself have
refused the exemption.

## Review rounds

**arch-review round 1**, step 0 (`code-review`) at `xhigh`: 15 findings, all
confirmed on re-reading, all fixed on this branch. Two were reproduced against
the branch's own hook rather than inferred, and both were the same class of
mistake — a mechanism that *looked* fail-closed and was not:

1. **The tier file carried no branch identity.** A worktree reused for a second
   branch inherited a `small` declaration and opened its PR with no
   delivery-review, in a live run. Fixed by the `<branch> <tier>` format; the
   one-field format is refused rather than migrated. Pinned by *a second branch
   in the same worktree does not inherit the tier*, and by a mutation run that
   removes the branch check and turns exactly that case red.
2. **`changed_lines` failed open, not closed.** It parsed `git diff
   --shortstat`, which git translates: under a localised install the English
   pattern matches nothing, the sum is 0, and 0 reads as an empty diff — the
   exemption granted to a branch of any size. Fixed by `LC_ALL=C git diff
   --numstat`, with an unparseable count refused outright. Pinned by *a small
   mission whose size cannot be measured pays in full*, and by the second
   mutation.

The rest were doc contradictions the tier introduced and three test defects:
the absence check could pass on an empty string when the gate had stopped
denying at all; the tier-vocabulary drift check matched bare substrings, so
`max` was satisfied by "maximum"; and the over-ceiling fixture would have
written at the filesystem root had its `worktree add` failed silently.

**arch-review round 2**, step 0 at `xhigh`: 15 findings, all confirmed, all
fixed. Two reproduced against the hook again, and both were the first round's
fix overshooting:

1. **A binary file voided the exemption.** `--numstat` prints `-` for both
   counts on a binary, and round 1 read any non-numeric count as *unmeasurable*
   — so a `small` mission shipping an icon, a font or a screenshot paid the full
   review and was told its size could not be measured, a message that points at
   an absent remote it does have. A binary contributes no lines because it has
   none; only an unparseable numstat is still fail-closed.
2. **The reminder told a two-line branch it had outgrown its tier.** The
   unmeasurable case was folded into the "outgrown" message, whose remedy is to
   raise the tier — a move this very branch makes deliberately irreversible.
   Three situations now get three messages, which the comment above them was
   already arguing for.

The rest were a drift check that had quietly stopped firing (the tier file kept
`written` non-empty, so a review skill could lose its own recording snippet with
the suite green), a comment promising a two-field check the code did not make,
an absence check with a two-word vocabulary, a duplicated snippet nothing
compared, and four documents the tier had left contradicting the gate —
including `CLAUDE.md` counting "the three rules above" with four bullets above
it, and the hooks README opening with the one-field format the hook refuses.

**On the leading-word parse.** Round 2 argued that `/mission small fonts are
unreadable` silently buys a skipped gate, and that a self-issued echo is a weak
mitigation for it. That is fair and it is not fully closed: `--small` is now
accepted for an ambiguous objective, and the echo names *what the tier drops*
rather than only the word it read, so the expensive misparse is the loudest one.
A parser that could tell an adjective from a tier was judged more machinery than
the failure warrants. Recorded here as a residual the trader can revisit.

## The request as received

Quoted verbatim, in the trader's own words and untranslated, under `CLAUDE.md`'s
exemption for a marked and attributed quotation. It is left in the original
because `delivery-review` re-derives the asks from *this* text and grades the
ledger above against it; a translation would put the mission's own reading of
the request into the reviewer's evidence, which is the one thing the section
exists to prevent.

> **Source:** the trader, session of 2026-08-31.
>
> alterar o /mission para aceitar small medium high max  como parmetro para
> evitar menor processamento e tudo. Tipo small é para tarefas pequeans que nao
> precisa de um code review lonog e tal e nem emsmo confirmar com um delivery
> review. Quero algo mais enxuto para tarefas menores serem entregues logo
