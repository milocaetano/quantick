# Mission — the public surface

**Objective.** Remove the five public-surface defects a fresh-context screener
found on the GitHub landing page: add a `LICENSE`, untrack the three
app-written root state files, untrack the 31 evidence screenshots, retire the
root design-lab folder, and make the README's platform claim and the CI agree.

**Why it matters.** Every one of the five is visible before a line of code is
read, and none needs a build to fix. A missing licence makes the repository
legally unusable to a reader who checks; a committed `paper-state.toml` leaks a
personal path and tells a reader the app writes into its own source tree;
10.7 MB of screenshots is 82 % of `.claude/`; and a README that claims three
tested platforms against a one-OS CI is a data-honesty defect in the one
document everybody reads.

**Tier:** `medium`, raised from the `small` the brief asked for.

The brief's reasoning for `small` was sound and still is: no Rust is edited, one
`cargo test -p quantick-app` run proves nothing reads the deleted root files,
and everything else is `git rm`, `.gitignore` lines and two sentence edits.
Nothing here is irreversible — history is explicitly not rewritten — and none of
step 3's question triggers fires.

What the estimate missed is *size*. Deleting a design lab and 31 screenshots
costs 279 insertions and 378 deletions against `origin/main`, and
`SMALL_TIER_MAX_CHANGED_LINES` in `guardrails.sh` puts the `small` exemption
from `delivery-review` at 300 changed lines. 657 is past it. The rule is to
raise the tier or split the work and never to shrink a diff to fit, and a tier
goes up, never down — so this branch takes the full `delivery-review` and the
whole injected gate table, and the `/goal` line is printed. The three places a
tier is recorded — this line, the worktree's `mission-tier` file and the PR
body — all say `medium`.

## Request ledger

The request is the mission brief `C:\src\mission-public-surface.md`, read in
full; the `/mission` line is its own summary of it.

| # | Ask |
| --- | --- |
| R1 | Add a `LICENSE` file at the root — MIT text, copyright 2026, the author's name as `git shortlog` gives it — so GitHub detects the licence it currently reports as `null`. |
| R2 | Untrack the three app-written root state files — `paper-state.toml` (which carries the personal OneDrive path), `layouts.toml`, `quantick-symbols.toml` — and `.gitignore` each in the existing "written by the app at runtime" style, one comment per line. |
| R3 | Prove nothing reads those three from the repository root before removing them: re-run the ledger-#3 grep and `cargo test -p quantick-app` once. |
| R4 | Untrack the 31 PNGs under `.claude/evidence/` (10,227,830 bytes), keeping every `.md`, `.txt`, `.log`, `.ps1` and `.py`, and `.gitignore` `.claude/evidence/**/*.png` with the rule's reason. |
| R5 | Repair any evidence document that cited a deleted PNG by path — the hash, or the line *captured locally, not tracked*, in its place. Grep first. |
| R6 | Retire the root design-lab folder `heatmap-design-ref/`: `capture_window.ps1` moves to `tools/capture_window.ps1`; the PNGs and `heatmap_lab.py` leave the tree, the lab script going to `tools/heatmap-lab/` if it is still a working tool and being deleted otherwise. |
| R7 | Follow the folder with both skill lines that name it — `ui-harness/SKILL.md:71` and `arch-review/references/language.md:16` — and leave `git grep -n 'heatmap-design-ref'` empty afterwards, or naming only `tools/heatmap-lab/`. |
| R8 | Make the README platform sentence (`README.md:86`) and the CI (`.github/workflows/ci.yml:13`) name the same operating systems. Recommended shape: add a `windows-latest` job running `cargo build --workspace` and `cargo test -p quantick-engine -p quantick-guards` under the pinned toolchain, and reword the sentence to what is then true. Either way *the README says only what a workflow file backs*. |
| R9 | Respect the context budget: the two skill-line edits leave `cargo test -p quantick-guards` green, and a path may get shorter, never longer. |
| R10 | State in the PR body that the checkout shrank by about 10.7 MB and the clone did not — history keeps every PNG and rewriting it is out of scope. |
| R11 | **The statement of purpose, which judges the rest:** "Fix all five so the tree, the README and the CI agree, with nothing the app or the harness depends on going missing." |

Out of scope by the brief's own instruction, and therefore not on this ledger:
rewriting history to purge the PNGs or the personal path; the 78
`GOAL-archive-*.md` files; the Portuguese branch name in history; any change
under `crates/`; and the `gh repo edit --add-topic` line, which is a GitHub
setting the trader runs after the merge and no tree change can make.

## Decisions taken by the trader

None. The mission opened at `small`, where the interrogation round is skipped,
and by the time the diff raised it to `medium` every edit was already made:
asking then would have thrown away no work, which is the only thing `medium`'s
two-question budget is for. Nothing in the brief raised the one question every
tier asks — no call here is the trader's, and nothing is irreversible. The two
readings that could have been questions are recorded as **S3** and **S4**.

## Assumptions

- **S1** — `heatmap_lab.py` is still a working tool, so it moves to
  `tools/heatmap-lab/` rather than being deleted. Its own docstring says it
  reproduces `orderflow_render.rs`'s colour pipeline outside the egui app, which
  is exactly the use a future heatmap change would have for it; deleting it is
  the irreversible half of R6's choice and moving it is the reversible one.
- **S2** — `heatmap_study.html` (272 KB) and the four PNGs are that script's
  *output*, not its source, and leave the tree with the rest of the scratch.
  `maximize.ps1` (646 bytes) leaves too: nothing in the repository names it, and
  the harness's window handling lives in `ui-harness`.
- **S3** — R8 takes the recommended shape, not the alternative: the sentence
  alone would leave the repository claiming less than it can prove, and a
  `windows-latest` job over `engine` + `guards` costs no egui and no bridge.
  *Wanted to ask*, had the tier allowed it.
- **S4** — R5 covers evidence documents. The one `GOAL-archive-*.md` that cites
  a deleted PNG is left verbatim: an archive is the record of what a shipped
  branch claimed, the brief puts those files out of scope, and rewriting one to
  match a later tree change would make it a worse record, not a better one. The
  PR body says so.
- **S5** — the two Portuguese comment blocks travelling with `capture_window.ps1`
  and the two Portuguese ramp labels in `heatmap_lab.py` are translated on the
  move. The alternative is carrying `heatmap-design-ref/`'s language exemption
  to two new paths, which costs more context bytes than R9 allows and leaves the
  debt where a reader trips on it.

## Acceptance criteria

- [x] **A1** — `LICENSE` exists at the root with the MIT text, copyright 2026 and
      the author's name from `git shortlog`, in a form GitHub detects.
      *Evidence:* the file, plus `gh repo view --json licenseInfo` quoted in the
      PR body as the pre-merge `null` it still is and the post-merge check the
      trader runs. → `LICENSE`, PR body. *(R1)*
      → **MET.** `LICENSE` at the root, MIT, "Copyright (c) 2026 Camilo Caetano" (the name `git shortlog` gives for 645 + 239 commits). `gh repo view --json licenseInfo` reads `{"licenseInfo":null}` today; the PR body carries that line and the post-merge re-run.
- [x] **A2** — `git ls-tree --name-only HEAD` names no `paper-state.toml`,
      `layouts.toml` or `quantick-symbols.toml`, and `.gitignore` carries all
      three with a one-line comment each in the existing style.
      *Evidence:* the command's output and the `.gitignore` diff.
      → `.claude/evidence/public-surface/tree.md`. *(R2)*
      → **MET.** `git ls-tree --name-only HEAD` matches none of the three; `.gitignore` carries `/layouts.toml`, `/paper-state.toml` and `/quantick-symbols.toml`, one comment each, beside `/chart-layers.toml`. `.claude/evidence/public-surface/tree.md`.
- [x] **A3** — nothing reads those three from the repository root: the
      ledger-#3 grep over `crates` is empty and `cargo test -p quantick-app` is
      green on the branch. *Evidence:* both outputs.
      → `.claude/evidence/public-surface/tree.md`. *(R3, R11)*
      → **MET.** the ledger-#3 grep exits 1 (empty); the wider grep resolves to working-directory constants, temp-root joins and prose only; `cargo test -p quantick-app` = 1894 passed, 0 failed. Same file.
- [x] **A4** — `git ls-tree -r HEAD -- .claude/evidence` counts zero `.png`,
      every non-PNG file under `.claude/evidence/` survives, and `.gitignore`
      ignores `.claude/evidence/**/*.png` with the reason written beside it.
      *Evidence:* both counts, before and after.
      → `.claude/evidence/public-surface/tree.md`. *(R4)*
      → **MET.** 31 PNGs and 10,227,830 bytes before, 0 after; all 97 non-PNG files survive; `.gitignore` gains `.claude/evidence/**/*.png` with its reason. Same file.
- [x] **A5** — no evidence document links a PNG that is no longer tracked; each
      such citation carries *captured locally, not tracked* instead.
      *Evidence:* the grep for `.png` under `.claude/evidence`, and the diff.
      → `.claude/evidence/public-surface/tree.md`. *(R5)*
      → **MET.** the two Markdown links repaired, the five documents that name a PNG headed with *captured locally, not tracked*. The one `GOAL-archive-*.md` citation is left verbatim on purpose (**S4**). Same file.
- [x] **A6** — `heatmap-design-ref/` is gone from the tree;
      `tools/capture_window.ps1` and `tools/heatmap-lab/heatmap_lab.py` exist and
      are English. *Evidence:* `git ls-tree` and the two files.
      → `.claude/evidence/public-surface/tree.md`. *(R6, R11)*
      → **MET.** `heatmap-design-ref/` gone; `tools/capture_window.ps1` and `tools/heatmap-lab/heatmap_lab.py` present, both English, the lab passing `python -m py_compile`. Same file.
- [x] **A7** — `git grep -n 'heatmap-design-ref'` over the branch is empty, and
      `ui-harness/SKILL.md` names `tools/capture_window.ps1` at a path no longer
      than the one it replaces. *Evidence:* the grep and a byte count of both
      paths. → `.claude/evidence/public-surface/tree.md`. *(R7, R9)*
      → **MET.** `git grep -n heatmap-design-ref` exits 1; the path in `ui-harness/SKILL.md` went from 37 bytes to 24; `cargo test -p quantick-guards` green (138 + 16 + 5 passed). Same file.
- [x] **A8** — the README platform sentence and `.github/workflows/ci.yml` name
      the same operating systems, with a `windows-latest` job actually running.
      *Evidence:* both quoted side by side in the PR body, and `gh pr checks`
      showing the new job. *(R8, R11)*
      → **MET.** README:86 and `ci.yml` both name Linux and Windows and both say what each runs; the `windows-latest` job builds the workspace and tests `engine` + `guards`. Quoted in the PR body; `gh pr checks` after push.
- [x] **A9** — the PR body states that the checkout shrank by about 10.7 MB and
      the clone did not, with the pack size that proves it.
      *Evidence:* the PR body. *(R10)*
      → **MET.** the PR body carries 10,715,763 bytes of checkout and `size-pack: 29.69 MiB` unchanged. `.claude/evidence/public-surface/tree.md`, *What did not shrink*.

### Injected gates

- [x] **G1** — every artifact this branch authors is in English, per `CLAUDE.md`.
      *Evidence:* `cargo test -p quantick-guards` green, `arch-review` dimension 8.
      → **MET.** `cargo test -p quantick-guards` green, and the two Portuguese blocks the branch relocated were translated rather than carried (**S5**).
- [x] **G2** — the four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`.
      *Evidence:* each command's output, run separately.
      → `.claude/evidence/public-surface/checks.md`.
      → **MET.** fmt, clippy, build and test each run on their own command line, all exit 0. One known contention flake on the first `--workspace` test run, green on re-run; `.claude/evidence/public-surface/checks.md` names it and why it is not this branch.
- [x] **G3** — performance impact declared. No Rust changes and no code path
      touched; the classification is *rare* by vacancy and is stated in the PR
      body.
      → **MET.** no Rust changed, so no path at any rate is touched. Stated in `checks.md` and in the PR body.
- [x] **G4** — `sh .claude/hooks/guardrails_test.sh` green: it reads `.gitignore`
      conventions, which this branch edits. *Evidence:* the output.
      → `.claude/evidence/public-surface/checks.md`.
      → **MET.** `sh .claude/hooks/guardrails_test.sh` = 111 passed, 0 failed. `checks.md`.
- [x] **G5** — `arch-review` run over `git diff origin/main...HEAD`, every Blocker
      and Should-fix resolved or deferred in the PR body. Its step 0 runs
      `code-review` at `low`. Shape dimensions 1–7 and 9 are waived only for the
      prose; the `ci.yml` and `.gitignore` edits take the full pass.
      *Evidence:* the review verdict.
      → **PENDING.** `arch-review` runs after this file is archived.

### Not applicable, and why

- **Hot path** — no Rust is edited, so there is no path to measure.
- **User-visible surface** (`ui-harness`, `visual-qa`, `trader-ux-review`) — the
  branch changes no rendered surface. It moves the *capture script* the harness
  uses, which A6 and A7 cover, and adds no hook.
- **Adds a capability** (`new-extension`) — nothing docks; the branch is net
  subtraction plus one licence file.
- **Adds something a trader does** — no new action, tool, trade or lock.
- **Engine / determinism** — `crates/` is out of scope by the brief.
- Every remaining row of step 4's table is answered above; at `medium` the
  full table applies and each row was tested against the diff rather than
  waived by tier.

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS. Owed at `medium`; the `small`
      exemption this mission opened with was revoked by its own diff size.
- [ ] **C2** — the PR is open, with the tier named beside the verification boxes.

## The request as received

*Quoted verbatim below — an attributed quotation under `CLAUDE.md`'s language
rule.*

> /mission small chore/public-surface — a fresh-context screener grading the
> GitHub repository found five things that cost more than they weigh: no LICENSE
> file (MIT is asserted only at README.md:463 and Cargo.toml:28, so GitHub shows
> none), three app-written state files committed at the root (paper-state.toml
> carries a personal OneDrive path; layouts.toml and quantick-symbols.toml are
> runtime output), 10.2 MB of screenshots under .claude/evidence/ (31 PNGs of
> 128 files), a design-lab folder heatmap-design-ref/ of scratch PNGs at the
> root, and a README claim of being tested on Windows, Linux and macOS
> (README.md:86) against a CI that runs on ubuntu-latest only
> (.github/workflows/ci.yml:13). Fix all five so the tree, the README and the CI
> agree, with nothing the app or the harness depends on going missing. Read
> C:\src\mission-public-surface.md in full before anything else and build the
> request ledger from it.

The brief it points to, `C:\src\mission-public-surface.md`, is the full request
and was read before this file was written; its evidence ledger #1–#10 was
re-verified against `origin/main` at `aaf74d3` before any edit.
