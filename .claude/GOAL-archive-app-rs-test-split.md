# Mission — split the test module out of `app.rs` into a file per subsystem

**Objective:** begin the first refactor of the oversized `crates/app/src/app.rs`
by moving its 474-test `#[cfg(test)] mod tests` into `src/app/tests/`, one file
per subsystem, taking the largest number of lines out of the file that can be
taken quickly and safely.

**Why it matters:** `app.rs` is 34,064 lines. Two thirds of that — 24,180 lines
— is a single flat test module, so every production edit to the file that the
whole desktop app hangs off is made inside a file no editor, reviewer or agent
can navigate. Removing the test module is the largest single reduction
available and the only one that touches no production line at all. It also
leaves the next batch of missions working against a ~9,900-line file instead of
a 34,000-line one, which is the point of doing it first.

**Tier:** `medium`. The mission started at `small` and was raised in its first
turn, before any edit: relocating 24,180 lines is roughly 48,000 changed lines
against `origin/main`, and the `small` exemption from `delivery-review` stops at
300. The work carries no trader call on money, safety or irreversibility and no
design invention, so it does not earn `high`; it earns the full gate table and a
conformance review, which is `medium`.

## Request ledger

| | Ask | Verbatim fragment |
| --- | --- | --- |
| **R1** | Begin the first refactor of `app.rs`, which is oversized. | *"começar o primeiro refactor do app.rs que ta gigante"* |
| **R2** | In this first mission, prioritise what can be broken out **fastest** right now. | *"priorizar o que da para quebrar mais rapido agora"* |
| **R3** | Take out the **largest possible number of lines**. | *"com maior numeor de linhas possivel"* |
| **R4** | Treat this as the first of a batch of missions — leave the ground prepared for the ones that follow. | *"NEssa primeira leva de msisão"* |
| **R5** | Split the tests into separate files by category, the way a C# solution keeps tests in their own files — if Rust's culture allows it. | *"Quebrar em arquivos separados por tipos de testes, classificar os testes em categorias e criar arquivos para cada"* |

R2 and R3 are separate asks that together form the selection rule: not merely a
large extraction, and not merely a quick one, but the largest one available
quickly. R5 arrived mid-turn and is what fixed the shape of the answer.

## Decisions taken by the trader

- **D1** — Granularity: roughly ten files, one per subsystem, mirroring the
  subsystems `app.rs` already has — `control_plane`, `panes_layout`,
  `drawings`, `feeds_sources`, `chart_view`, `input_ui`, `paper_trading`,
  `orderflow`, `layers`, `indicators` — with `mod.rs` carrying the shared test
  harness. Chosen over four coarse files and over twenty fine ones.
- **D2** — Scope of this batch: the test module only. No production line is
  relocated, so the size ratchet's production ceiling for `app.rs` does not
  move in this mission; tightening it is the second batch's work, which then
  starts from a navigable file.

## Assumptions

- **S1** — "app.rs" is `crates/app/src/app.rs`, the only file by that name and
  the top entry of `crates/guards/size-baseline.txt`. Nothing else in the repo
  answers to the description the request gives it.
- **S2** — Rust's answer to R5 is a child-module split, not an integration-test
  split. `crates/app/tests/*.rs` compiles as a separate crate and sees only
  `quantick-app`'s public API, while these 474 tests use `use super::*` and
  reach private items of `QuantickApp`. Child modules under `src/app/tests/`
  keep that access unchanged and widen no visibility, which is both the
  idiomatic Rust answer and the only one that compiles.
- **S3** — "largest number of lines" is measured as lines removed from
  `app.rs`, not lines deleted from the repository. A refactor relocates.
- **S4** — Behaviour is unchanged and no test is rewritten, renamed, deleted or
  added. The only edits to test bodies are the import adjustments a module move
  forces.
- **S5** — *Wanted to ask, budget spent on D1 and D2:* whether "lines" meant the
  file as opened or the production lines the size guard counts, since the guard
  deliberately ignores test lines. Reading taken: the file as opened, which is
  what the request describes and what D2 then confirmed.
- **S6** — *Wanted to ask, budget spent:* whether the shared harness should be
  its own `harness.rs` rather than living in `tests/mod.rs`. Reading taken:
  `mod.rs`, because every category file already does `use super::*` to reach
  the parent, so the harness costs no second import there.

## Acceptance criteria

- [ ] **A1** — `crates/app/src/app.rs` drops by at least 20,000 lines, and the
      removed lines are the test module in full.
      *Evidence:* `wc -l` before and after, and the `mod tests` declaration.
      → the PR body. *(R1, R3)*
- [ ] **A2** — The tests live in `crates/app/src/app/tests/`, one file per
      subsystem per D1, with `mod.rs` carrying the shared harness and the `mod`
      declarations. No category file exceeds a third of the old module.
      *Evidence:* a listing of the directory with per-file line counts.
      → the PR body. *(R5)*
- [ ] **A3** — Every one of the 474 tests still exists and still runs: the test
      count reported by `cargo test -p quantick-app` is identical before and
      after, and none is marked ignored.
      *Evidence:* the two test-count lines, quoted.
      → the PR body. *(R1, R5)*
- [ ] **A4** — Test access to private items is preserved without widening any
      visibility: no `pub` or `pub(crate)` is added to a production item to make
      a moved test compile.
      *Evidence:* the production-side diff of `app.rs` is confined to deleting
      the test module and adding the `mod tests;` declaration.
      → the PR body, as the `git diff --stat` for `app.rs`. *(R5, S2)*
- [ ] **A5** — No production behaviour changes: no production line is edited,
      only the module declaration is added.
      *Evidence:* `arch-review` verdict plus the `app.rs` diff above.
      → the PR body. *(R1)*
- [ ] **A6** — The PR body states what came out, what is left in `app.rs`, and
      what the next batch should take, so the following missions start from a
      stated position rather than re-deriving it.
      *Evidence:* a named section in the PR body.
      → the PR body. *(R4)*
- [ ] **A7** — The extraction was the fastest large move available, and the PR
      says why the alternatives were not: an integration-test split does not
      compile (S2), and a production extraction was deferred by D2.
      *Evidence:* a named section in the PR body.
      → the PR body. *(R2)*

## Injected gates

- [ ] **G1** — Every artifact in English, per `CLAUDE.md`.
      *Evidence:* `cargo test -p quantick-guards` green; `arch-review`
      dimension 8. → the PR body.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own,
      never chained behind a shell operator that can hide a failure.
      *Evidence:* the four exit codes. → the PR body.
- [ ] **G3** — Performance impact declared. Every touched path classified by
      rate. This mission moves test code only, so no production path changes
      rate and nothing enters the shipped binary that did not before —
      `#[cfg(test)]` compiles the module out either way. Stated, not assumed.
      *Evidence:* the declaration. → the PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, with every
      Blocker and Should-fix resolved or deferred in the PR body.
      *Evidence:* the review verdict. → the PR body.
- [ ] **G5** — The repository guards green: `cargo test -p quantick-guards`
      (size ratchet, English scan, encoding check).
      *Evidence:* the exit code. → the PR body.

## Not applicable, and why

- **Hot path evidence** — no production path is touched, so there is no rate to
  measure. G3 states this rather than omitting it.
- **`ui-harness` / `visual-qa` / `trader-ux-review`** — no surface is added or
  changed. Nothing a trader can see differs by one pixel.
- **`new-extension`** — no capability is added. Nothing docks; code moves.
- **Engine / determinism test-first** — no engine code is touched, and no
  behaviour is written to test first. The 474 existing tests are the guard.
- **Docs/skills waiver** — does not apply. This is a code change and takes the
  full shape pass.
- **Size-ratchet tightening** — deliberately excluded by D2, not omitted. The
  guard counts production lines only, and this mission moves none.

## The request as received

Quoted verbatim and untranslated, per `CLAUDE.md`'s exemption for a marked and
attributed quotation: this is the trader's own request, and paraphrasing it
would put the mission's reading of the words in place of the words themselves,
which is exactly what `delivery-review` needs the section for.

Trader (milocaetano), 2026-09-02, opening request:

> começar o primeiro refactor do app.rs que ta gigante. NEssa primeira misssao
> vc vai priorizar o que da para quebrar mais rapido agora com maior numeor de
> linhas possivel.  NEssa primeira leva de msisão

Trader, same session, mid-turn, after being shown the file's shape:

> em c# a gnt tem projeot de teste sparado em arquivos. Se vc conseguir fazer o
> mesmo em rust, nao sei como é a cutlura do rust developer... Mas isso ja
> ajduaria a reduzir muito o tamanho do arquivo. Quebrar em arquivos separados
> por tipos de testes, classificar os testes em categorias e criar arquivos
> para cada algo assim

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the `medium` tier beside the four checks.
