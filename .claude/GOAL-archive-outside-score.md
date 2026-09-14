# Mission: outside-score, a second scoring authority

Add an `outside-score` skill: a rubric-independent scoring authority for Quantick that grades architecture, software engineering and agent change cost with measured thresholds, a committed measurement script and a divergence report against `quantick-score`, so the trader can start a campaign on it now.

Why: on 2026-09-13 an outside reading of `d3d4b23d` scored architecture 7.5 while `quantick-score` gave the same tree 97/100. The gap sits in what `quantick-score` does not measure: UI-free code in the UI crate, encapsulation inside the app crate, function size, frame ordering by comment, capture hooks in the production binary, and what a change costs an agent. A campaign aimed at the old rubric cannot close that gap.

**Tier:** medium. It adds a skill, a rubric, a Python measurement tool with tests and a CI step, and edits a tracked campaign contract; more than 300 changed lines, and it sets the numbers a campaign will chase, so the completeness pass earns its cost.

## Request ledger

- **R1** — Create a skill that is a new scoring rubric ("uma skill que é uma nova régua").
- **R2** — The rubric grades the outside reading from this session: architecture and software engineering, including the five gaps the outside review named (UI-free code in the UI crate, encapsulation inside a crate, explicit composition, harness in production, function size).
- **R3** — It measures the trader's thesis that architecture keeps agent context and feature cost low ("A IA demora pra entender contexto e fica caro pra criar novas features").
- **R4** — A campaign can be created on it now ("para a gente criar uma campanha baseada nessa régua agora"). Statement of purpose; judges the others.
- **R5** — It embodies the devil's-advocate principle the trader stated ("Advogado do Diabo, opinião divergente com vários agentes são importantes"): independent of `quantick-score`, and it reports where the two disagree.

## Decisions

None asked: the trader is not watching in real time. See S1–S4.

## Assumptions

- **S1** — Name `outside-score`, rubric at `docs/quality/outside-score-rubric.md`, following `quantick-score`'s shape (thin `SKILL.md`, rubric outside the context ratchet's scope). Conventional default.
- **S2** — *Wanted to ask:* weights and thresholds. Reading taken: anchor 3 means a typical production system, so the 0–5 scale lines up with the grade bands; v1 is calibrated so that `d3d4b23d` lands within 0.5 of the manual reading per dimension (architecture 7.5, software engineering 8.0). A version bump changes them; reversible.
  *Revised during the work:* calibrating on Quantick itself would fit the rubric to the reading it is meant to check, so v1.0 was calibrated on three outside projects instead (ripgrep, nushell, rerun), which also showed two metrics were unfair: `pub(crate)` density rewards spelling crate-private items `pub` (now reported, not graded), and an absolute count of long functions punishes size (now per 100,000 lines). The independent baseline lands at 6.5, below the manual 7.5/8.0 reading, with the evidence cited in the report.
- **S3** — *Wanted to ask:* whether agent change cost is scored. Reading taken: yes, as a 15% dimension, because R3 is the trader's own thesis and no rubric measures it today.
- **S4** — The measurement lives in `tools/outside_score/measure.py` (Python, like the other tools) rather than in `quantick-guards --report`, so this change stays out of the guards crate while PR #450 is open against it. The rubric names porting the rows into `--report` as the path to a ratchet.

## Acceptance criteria

- [x] **A1** — `.claude/skills/outside-score/SKILL.md` and its Codex adapter `.agents/skills/outside-score/SKILL.md` exist, point to the rubric, and the context guard passes. *Evidence:* files; `cargo test -p quantick-guards` exit 0. → PR body. *(R1)*
- [x] **A2** — The rubric defines versioned criteria with weights, measured thresholds or anchors, formulas, grade bands and a report format, covering the five named gaps plus software engineering. *Evidence:* the rubric's criterion tables. → `docs/quality/outside-score-rubric.md`. *(R2)*
- [x] **A3** — The rubric scores agent change cost with observable next-point conditions. *Evidence:* the agent change cost table. → `docs/quality/outside-score-rubric.md`. *(R3)*
- [x] **A4** — `tools/outside_score/measure.py` prints every measured row deterministically for a tree, and its tests cover strings, raw strings, lifetimes, char literals, comments, `#[cfg(test)]` items, trait declarations and impl spread. *Evidence:* `python tools/outside_score/test_measure.py` exit 0; two runs byte-identical. → PR body. *(R2, R3)*
- [x] **A5** — The rubric forbids reading `quantick-score` material before scoring, requires a divergence table after, and makes a 9.0+ grade provisional until a different model family or a human reproduces it within 0.5. *Evidence:* the Independence section. → `docs/quality/outside-score-rubric.md`. *(R5)*
- [x] **A6** — The campaign workflow accepts `outside-score` as a baseline, as it accepts `quantick-score`. *Evidence:* `docs/campaign/workflow.md` step 4; context guard green. → PR body. *(R4)*
- [x] **A7** — A full assessment of `origin/main` at `d3d4b23d` under rubric v1.0 is committed, with measured rows, ledger, divergence table and ranked next moves, so a campaign can reuse it as its baseline. *Evidence:* the report file. → `docs/quality/outside-score/d3d4b23d.md`. *(R4, R2, R5)*
- [x] **A8** — A one-paragraph `/campaign create` prompt that targets the new rubric is given to the trader. *Evidence:* the PR body and the session hand-off. → PR body. *(R4)*

## Injected gates

- [x] **G1** — Every artifact English; `crates/guards/src/language.rs` green. Source: `CLAUDE.md`.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `cargo test --workspace` green, each run alone; `ruff check --select F tools/` clean. Source: `CLAUDE.md` verification loop.
- [x] **G3** — Performance impact declared: every touched path is rare (offline tool, docs, CI step); no per-trade, per-depth or per-frame path changes. Source: mission step 4.
- [ ] **G4** — `arch-review` over the exact diff against `origin/main`, step 0 at `low`, full shape pass for the script and CI step, every Blocker and Should-fix resolved or deferred in the PR body. Source: mission step 8.
- [ ] **G5** — Final-head CI green. Source: `CLAUDE.md`.

Not applicable: hot path, user-visible surface, new capability, trader action, engine territory — the diff touches none of them.

## Evidence recorded

- A1: both `SKILL.md` files exist; `cargo test -p quantick-guards` all suites ok; context measured 295,645 against the unchanged budget 295,668.
- A4: `python tools/outside_score/test_measure.py` ran 10 tests, OK; two runs over `d3d4b23d`'s tree byte-identical (`cmp`); `ruff check --select F tools/outside_score` clean.
- A6: step 4 of `docs/campaign/workflow.md` names both skills at 11,574 bytes, its ceiling.
- A7: `docs/quality/outside-score/d3d4b23d.md`, written by a fresh-context assessor under Independence rule 1, with the author's divergence section appended.
- A8: the prompt is in the PR body.
- G2: `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace --all-targets` exit 0; `cargo build --workspace` exit 0; `env -u QUANTICK_BUBBLES cargo test --workspace` exit 0, 3,808 passed, 0 failed, 18 ignored across 98 suites.
- G3: rare only: an offline script, docs and one CI step.

## Closing steps

- **C1** — `delivery-review` completeness pass returns PASS (medium tier, inline).
- **C2** — The PR is open with the tier, evidence and verification labels in its body.

## The request as received

Attributed quotation (the trader, 2026-09-13, Portuguese), exempt under `CLAUDE.md`'s language rule:

> consegue criar uma skill que é uma nova regua para a gnt criar uma campanha baseada nessa regua agora?

Earlier statements this mission draws on, same session:

> Só que eu to sempre questionando, eu cirei uma régua, mas questionei a regua. Isso é a diferença. Eu entendo o que é viés. Sei que IA pode enviesar assim como humanos. Adivogao do Diabo, opinão divergente com vários agentes são importantes.

> A medida que projetos de Ia cresce, eles se tornam mais lentos de desenovlver e mais dificil de se manter com muito codigos. A IA demora pra ser e entender contexto e fica caro pra criar novos features. Um projeto bem arquitetado, facilita a criaçãode novas features.
