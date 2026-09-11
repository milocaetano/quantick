# Mission: `QuantickApp`'s 56 fields regroup by owner

Group the fields whose only readers are one or two sidecars into six owner
structs declared in the sidecar that owns each, so `QuantickApp` carries at
most 30 fields and every read becomes `self.<owner>.<field>` and nothing else.

**Why it matters.** Five sidecar cuts took `app.rs` from 9,241 lines to 824
without changing one `self.x`, so the seventeen modules under `app/` still
share one 56-field namespace and a reader of `indicator_manager.rs` cannot
tell which fields are its own. This is the first `app` mission whose diff is
not a move: it converts physical redistribution into conceptual decoupling.

**Tier:** `medium`. 785 path rewrites across eighteen production files and two
test trees, with a normalised-diff proof and one byte-identical capture claim,
but no trader-visible change, no new behaviour and no design call — bigger
than `small`'s ceiling, well short of the interrogation `high` would earn.

## Request ledger

| # | Ask | Source |
| --- | --- | --- |
| R1 | Six owner structs, each `pub(super)` with `pub(super)` fields, declared in the sidecar that owns it: `IndicatorState`, `ControlState`, `ChromeState`, `HistorySettings`, `HealthCounters`, `AlertState` | scope 1 |
| R2 | `QuantickApp` keeps the twelve core fields and six panel structs flat and gains six owner fields — "56 fields become at most 30" | scope 2 |
| R3 | The constructor builds each owner struct where the flat fields were built, in the same order | scope 2 |
| R4 | "Every read is a path change and nothing else" — no accessor methods for the groups, no field changes type, visibility beyond `pub(super)`, or default | scope 3 |
| R5 | Tests change paths only; the destructuring at `app/tests/mod.rs:436` lists the owner structs | scope 4 |
| R6 | The normalised-diff proof in the PR body, with the `sed` command quoted and its `--stat` | scope 5 |
| R7 | The coupling number in the PR body: per-owner `grep -l` module list, and the per-sidecar distinct-field count of ledger #14 before and after | scope 6 |
| R8 | Baselines: `--tighten` on whatever shrank; `--report` shows `struct.wide.app::QuantickApp` at 30 or fewer | scope 7 |
| R9 | "each carries its `file:line` so the mission re-checks it instead of trusting it" — verify every evidence-ledger claim before acting | preamble |
| R10 | Generated hook registry and capability inventory unchanged; workspace round-trip test unchanged and green — the owner structs are in-memory only | criteria |
| R11 | `cargo test -p quantick-app` runs the same number of tests, green; four-check loop green; `cargo test -p quantick-guards` green | criteria |
| R12 | Three harness captures (`QUANTICK_LAYOUT`, `QUANTICK_CONTROL_PANEL`, `QUANTICK_INDICATORS_AUTOSTART`) byte-identical to `origin/main` under the same env, hashes in the PR body | criteria |
| R13 | Each owner struct named by at most three modules besides its owner, `workspace_save.rs` and `control_host.rs` excepted | criteria |
| R14 | Respect what is out of scope: no `Tab`/`ChartPane`, no serialising an owner struct, no accessors, builders or crate moves | out of scope |
| R15 | Re-check the parallel branches (`refactor/tab-rs-sidecars`, `refactor/gateway-rs-sidecars`) for file overlap before the PR | parallel work |
| R16 | The purpose that judges the rest: a reader of a sidecar can see which fields it reaches, so the regroup is a reduction of conceptual coupling and not another move | "Why this mission" |

## Decisions taken by the trader

- **D1** — `control` (4 modules) and `chrome` (5) exceed R13's three, measured.
  The partition is the brief's and the criterion is the brief's, and the two
  contradict. Resolution: **build the six structs exactly as the brief
  specifies and report the real numbers** with the reason for each excess. R13
  is discharged as *measured and explained*, not as *satisfied*. No
  re-partition, no seventh struct.

  *Correction, after the work.* The 4 and 5 the decision was shown were
  measured with a line-based grep, which misses the chained form
  (`self` newline `.chrome` newline `.layout_picker_open`). Re-measured across
  lines, the counts are `chrome` 6 and `control` 4, and `indicators` is 4
  rather than the 2 first reported. The decision is unaffected — it was to
  report whatever the number turned out to be — but the number is worse than
  the one that informed it, so the PR body carries both and says which grep
  produced which.
- **D2** — R12's byte-identity is measured against itself first: capture the
  `origin/main` control run twice. If `main` × `main` is byte-identical, prove
  the branch byte-identical as asked. If it is not, the criterion is
  unachievable in this environment regardless of the branch — fall back to the
  control plane's own account of what is on screen plus the hashes side by
  side, and record in the PR body why byte-identity is not measurable here.

## Assumptions

- **S1** — `ChromeState` is declared in a **new `app/chrome.rs`**, not in
  `menu_bar.rs`. The brief offers the choice; measurement decides it — the ten
  fields are read by `frame`, `toolbar_wiring`, `layout_wiring`, `menu_bar`,
  `drawing_input`, `control_host` and `workspace_save`, and `menu_bar` names
  just one of them once. No module owns the group, so no module gets to host
  it. One new file plus one `mod chrome;` line, which is the trunk rule's own
  shape.
- **S2** — The brief's "1,370 `self.<field>` sites" is **785**. Its grep
  (`\bself\.[a-z_]+\b`) counts `self.method()` calls as fields. Measured on
  `origin/main` at `0c3431d` over `app.rs` plus the seventeen sidecars. The
  number is descriptive, not an ask; the work is unchanged.
- **S3** — The brief's "sixteen sidecar modules" is **seventeen**
  (`crates/app/src/app/*.rs`, `tests/` excluded). Descriptive, not an ask.
- **S4** — The branch is cut from `origin/main` at **`0c3431d`**, not the
  brief's `62c8730`: `refactor/tab-rs-sidecars` landed as #307 between the
  brief being written and the mission starting. Every claim above was
  re-measured at `0c3431d`; R15's overlap check therefore covers only
  `refactor/gateway-rs-sidecars`, plus whatever lands before the PR.
- **S5** — Owner struct field order follows the declaration order the fields
  have in `QuantickApp` today, so R3's "in the same order" is checkable by
  eye against the diff.
- **S6** — The six owner structs derive nothing they did not already have.
  `QuantickApp` derives nothing, and R4 forbids a type change; a `#[derive]`
  on a new struct would be a behaviour claim nobody asked for.
- **S7** — *wanted to ask, budget spent on D1 and D2.* Where a test reads a
  moved field through something that is not the app (`tab.history_reach`,
  a `Tab`'s own field of the same name), the read does **not** change. Only
  reads rooted at a `QuantickApp` become owner paths. The compiler settles
  each case; no name is rewritten blind.

## Acceptance criteria

- [ ] **A1** — Six owner structs exist, each `pub(super)` with `pub(super)`
      fields, in `indicator_manager.rs`, `control_host.rs`, `chrome.rs` (new),
      `tabs.rs`, `health.rs` and `replay_and_history.rs`.
      *Evidence:* the six declarations quoted from the diff.
      → PR body. *(R1)*
- [ ] **A2** — `QuantickApp` declares **24** fields: twelve core, six panels,
      six owners. No field is `pub`; no field changed type or default.
      *Evidence:* the field count and `git diff` filtered to `pub ` additions
      in `app.rs`. → PR body. *(R2, R4)*
- [ ] **A3** — No new method on `QuantickApp` and no accessor for any group.
      *Evidence:* `git diff` filtered to `fn ` additions across `app.rs` and
      `app/*.rs`, empty but for the six owner declarations' own code, if any.
      → PR body. *(R4, R14)*
- [ ] **A4** — The constructor at `app.rs:589` builds the six owner structs at
      the position the flat fields held, in the same order.
      *Evidence:* the constructor hunk of the diff. → PR body. *(R3, S5)*
- [ ] **A5** — The normalised diff — both sides through
      `sed -E 's/self\.(indicators|control|chrome|history|health|alerts)\./self./g'`
      — is empty outside the struct definition, the constructor and the six
      owner declarations. *Evidence:* the quoted command and its `--stat`.
      → PR body. *(R6, R4)*
- [ ] **A6** — The coupling numbers are published: per owner, the modules that
      name it; and the per-sidecar distinct-field count before and after
      (`workspace_save` 26 and `control_host` 25 on `main`).
      Per D1, `control` at 4 and `chrome` at 5 are reported with their reason,
      not hidden. *Evidence:* the grep lists and the two counts.
      → PR body. *(R7, R13, R16)*
- [ ] **A7** — Tests change paths only, and `app/tests/mod.rs:436`'s
      `let QuantickApp { … }` names the owner structs.
      *Evidence:* the test-tree diff, and A5's normalised diff extended over
      the test trees. → PR body. *(R5)*
- [ ] **A8** — `cargo test -p quantick-app` reports the **same test count** as
      `origin/main`, green; the workspace round-trip test is untouched.
      *Evidence:* both `test result:` lines, branch and main.
      → PR body. *(R10, R11)*
- [ ] **A9** — The generated hook registry and capability inventory are
      byte-unchanged. *Evidence:* `git diff --stat` over the generated files,
      empty. → PR body. *(R10)*
- [ ] **A10** — `--report` shows `struct.wide.app::QuantickApp` at 30 or fewer,
      and `--tighten` has been run on whatever shrank.
      *Evidence:* the `--report` line and the baseline diff. → PR body. *(R8)*
- [ ] **A11** — Every claim in the brief's evidence ledger #1–#15 was
      re-measured at `0c3431d` before any edit, and each divergence is
      recorded. *Evidence:* S2, S3, S4 above and the measurements in the PR
      body. → this file + PR body. *(R9)*
- [ ] **A12** — Three harness captures under `QUANTICK_LAYOUT`,
      `QUANTICK_CONTROL_PANEL` and `QUANTICK_INDICATORS_AUTOSTART`, compared to
      an `origin/main` control run under the same env, per D2 — including the
      `main` × `main` self-comparison that says whether byte-identity is
      measurable at all. *Evidence:* the six hashes and the verdict.
      → PR body. *(R12)*
- [ ] **A13** — Nothing out of scope moved: no `Tab` or `ChartPane` field, no
      owner struct serialised, no builder, no crate move.
      *Evidence:* `git diff --stat` showing the touched files.
      → PR body. *(R14)*
- [ ] **A14** — Re-checked against the open parallel branches before the PR;
      the only shared file is the size baseline, if this mission tightens one.
      *Evidence:* `git diff origin/main...refactor/gateway-rs-sidecars --stat`
      intersected with this branch's. → PR body. *(R15, S4)*
- [ ] **G1** — Every artifact in English.
      *Evidence:* `arch-review` dimension 8 verdict + `quantick-guards` green.
      → PR body.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`. Run separately, no
      chaining. *Evidence:* the four exit codes. → PR body.
- [ ] **G3** — Performance impact declared. Expected: **none at any rate** —
      a field's address inside the struct changes, its access does not; no new
      indirection, allocation or copy on any per-trade, per-depth or per-frame
      path. *Evidence:* this classification, plus A12's captures.
      → PR body.
- [ ] **G4** — `cargo test -p quantick-guards` green. *Evidence:* the run.
      → PR body.
- [ ] **G5** — `arch-review` run over `git diff origin/main...HEAD`, with its
      step 0 bug pass, every Blocker and Should-fix resolved or deferred with
      severity in the PR body. *Evidence:* the verdict + the `arch-review-ok`
      marker. → PR body.

## Not applicable, and why

- **Hot path evidence beyond G3** — no touched path changes instruction count;
  the regroup is a compile-time address change. A12's captures stand as the
  behavioural proof the brief asked for, in place of a dense-tape run.
- **`ui-harness` / `visual-qa` / `trader-ux-review`** — no surface is new or
  changed. The three hooks A12 drives already exist; A12 *uses* the harness to
  prove nothing changed, which is the opposite of registering a new hook.
- **`new-extension`** — nothing docks. No feed, bar type, indicator, layer,
  panel or crate is added; `chrome.rs` is a declaration site inside an existing
  module tree, not a capability.
- **The second operator** — no new act, tool, trade or lock. Every capability
  a script reaches today it reaches unchanged, through the same methods
  (ledger #11: nothing outside `app.rs`/`app/` names a field).
- **Engine / determinism test-first** — `crates/app` is not the engine, and no
  bar-building code is touched.
- **Docs/skills waiver** — does not apply; this is a code change and takes the
  full shape pass.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, its body carrying the tier, the four verification
  boxes and every evidence artifact named above.

## The request as received, verbatim

*An attributed quotation of the trader's request, kept in the language it was
written in per `CLAUDE.md`'s exemption for marked quotations.*

> medium refactor/app-fields-by-owner — crates/app/src/app.rs is 824 lines and `QuantickApp` (`app.rs:187-455`) still has 56 flat fields read by 1,370 `self.<field>` sites across sixteen sidecar modules; the sidecars moved, the coupling did not. Group the fields whose only readers are one or two sidecars into six owner structs — indicators (10 fields), control (6), chrome (10), history settings (4), health counters (6), alerts (2) — declared in the sidecar that owns each, leaving the twelve shared fields and the six panel structs flat: 56 fields become at most 30. Every read becomes `self.<owner>.<field>`, nothing else changes, and a grep proves each owner struct is touched by at most three modules. Read C:\src\mission-app-fields-by-owner.md in full before anything else and build the request ledger from it.

The mission brief that invocation points at, `C:\src\mission-app-fields-by-owner.md`, is the request's second half; its scope items and acceptance criteria are quoted into the ledger above as `R1`–`R16`.
