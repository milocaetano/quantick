# Mission: campaign model routing

Add a Routing section to `docs/campaign/workflow.md` so the campaign
coordinator chooses, records and honours the model that executes each child
task — on resume and under Codex alike.

**Tier:** small — one prose section in a normative contract plus two one-line
agreements (`state.md` field list, Codex role mapping) and a signed ratchet
raise. No code, no UI, no trader call among the asks: the trader dictated every
rule, so there was nothing to interrogate.

## Request ledger

- **R1** — A "Routing" section in `docs/campaign/workflow.md` lets the coordinator choose, per child task, which model executes it.
- **R2** — The choice is recorded so resume and Codex honour it (purpose; judges the rest).
- **R3** — Rules are stated by role so Codex can map them via `.agents/references/codex-compatibility.md`.
- **R4** — The coordinator runs on the strongest model (Fable).
- **R5** — A child mission defaults to the strong implementation model (Opus, `Agent` with model "opus").
- **R6** — Escalate a child to the strongest model when it is tier high or max, designs a new port or crate boundary, breaks a module cycle, or touches engine determinism or a hot path.
- **R7** — Escalate when an Opus child came back from two review rounds with findings "flat rather than shrinking".
- **R8** — Retrieval and measurement go to haiku, checklist application to sonnet, as `CLAUDE.md` says.
- **R9** — The coordinator writes `executor: <model> — <reason>` into each child issue before dispatch.
- **R10** — A resumed campaign reads the executor line rather than re-deciding.
- **R11** — The coordinator runs the child's interrogation (mission step 3) before dispatch and passes the answers in the brief as decisions.
- **R12** — A doubt the child meets later becomes a human-task, never a guess.
- **R13** — State only rules that decide an outcome; keep the section short (context ratchet).

## Assumptions

- **S1** — The Codex mapping gains one sentence mapping each `executor:` model name to a role. Without it "Codex honours it" has no mapping for the implementation role, which the existing three-role bullet lacks. Safe: additive, one edit to reverse.
- **S2** — `state.md`'s child field list gains `executor`, so the two campaign contracts agree on what a child issue carries. Safe: one word, within that file's ceiling.
- **S3** — "Escalate after two flat rounds" reads as re-dispatching the remaining work at *strongest*, and every escalation appends a new executor line; the latest line wins. Safe: the only reading that keeps R10 decidable.
- **S4** — A child that raises its own tier to high or max after dispatch returns to the coordinator for re-routing; otherwise R6's tier trigger could be dodged by a late raise. Safe: mirrors mission's "a tier goes up, never down".
- **S5** — "A doubt the child meets later" means a doubt that would have earned a mission step 3 question; items mission already settles as `S` assumptions (conventional defaults) stay assumptions. Safe: the reading that does not contradict mission step 3.
- **S6** — The ~1,281-byte growth is a signed ceiling raise paid from the budget's unused room, `!budget` unchanged — the precedent `ai-review` set. Safe: the guard enforces the total either way.

## Acceptance criteria

- [x] **A1** — `docs/campaign/workflow.md` has a `## Routing` section, linked from run-loop step 3, that names a model per child. *Evidence:* quoted section. → PR body. *(R1, R2)*
- [x] **A2** — Rules are phrased by role (*strongest*, *implementation*, *checklist*, *retrieval*), and the Codex mapping maps each executor model to a Codex role. *Evidence:* quoted section and `codex-compatibility.md` diff. → PR body. *(R3, R2)*
- [x] **A3** — Coordinator at *strongest*; children at *implementation* via `Agent` `model: "opus"`; the five escalation triggers; retrieval/measurement at haiku and checklists at sonnet. *Evidence:* quoted section. → PR body. *(R4, R5, R6, R7, R8)*
- [x] **A4** — The executor line is written before dispatch and resume/Codex read it, never re-decide. *Evidence:* quoted section, `state.md` field. → PR body. *(R9, R10, R2)*
- [x] **A5** — The coordinator runs the child's step 3 and passes decisions in the brief; later doubts become human tasks. *Evidence:* quoted section. → PR body. *(R11, R12)*
- [x] **A6** — The section holds operative rules only and the growth is signed in the context baseline. *Evidence:* byte count and `context-baseline.txt` diff; `cargo test -p quantick-guards` green. → PR body. *(R13)*
- [x] **G1** — Every authored artifact is English, bar this file's verbatim request. *Evidence:* `crates/guards/src/language.rs` via `cargo test`; arch-review dimension 8. → PR body.
- [x] **G2** — fmt, clippy, build and test green on latest `origin/main`; performance impact declared: none (prose and a baseline number, no runtime path). *Evidence:* the four exit codes. → PR body.
- [ ] **G3** — `arch-review` (docs change: step 0 plus dimension 8) run, every Blocker/Should-fix resolved or deferred. *Evidence:* review verdict. → PR body.

Not applicable: hot path, UI, capability, trader action and engine gates — the diff is prose plus one baseline number.

## Closing steps

- **C1** — PR open, naming the tier.

## Request as received

> Add model routing to the campaign coordinator. In docs/campaign/workflow.md, add a "Routing" section that lets the coordinator choose, per child task, which model executes it, and records that choice so resume and Codex honour it. Rules, stated by role so Codex can map them (.agents/references/codex-compatibility.md): the coordinator runs on the strongest model (Fable); a child mission defaults to the strong implementation model (Opus, via Agent with model "opus"); the coordinator escalates a child to the strongest model when the child is tier high or max, designs a new port or crate boundary, breaks a module cycle, touches engine determinism or a hot path, or when an Opus child came back from two review rounds with its findings flat rather than shrinking; retrieval and measurement go to haiku and checklist application to sonnet, as CLAUDE.md already says. The coordinator writes "executor: <model> — <reason>" into each child issue before dispatch, and a resumed campaign reads it from there rather than re-deciding. Because a subagent cannot ask the trader anything, the coordinator runs the child's interrogation (mission step 3) itself before dispatching and passes the answers in the child's brief as decisions; a doubt the child meets later becomes a human-task, never a guess. State only rules that decide an outcome; keep the section short, since the context ratchet counts skill prose.
>
> — the trader, `/mission small`, 2026-09-11
