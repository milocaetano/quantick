# Mission — pin the build environment

Pin the build environment — toolchain, dependency versions, lint levels and a
supply-chain audit — so that a red CI means the code changed, not the
repository. An agent session must never spend itself diagnosing a failure the
repository caused rather than the change did.

**Tier:** `medium`. The work touches every one of the seventeen crate
manifests, the CI workflow and the root manifest, and it changes what `cargo
build` treats as an error for every future session — too wide for `small`. But
it is mechanical migration plus configuration rather than open design, so it
does not earn `high`. `delivery-review` runs as a completeness pass.

## Request ledger

| | Ask |
| --- | --- |
| **R1** | Add a `rust-toolchain.toml` pinning the exact stable version CI runs today, with the components `clippy` and `rustfmt`. |
| **R2** | Move every third-party dependency to `[workspace.dependencies]` in the root `Cargo.toml`. |
| **R3** | Have all seventeen crates inherit it, "so a version and its feature set are stated once instead of six times for tokio". |
| **R4** | Add a workspace `[lints]` table carrying the same levels the CI clippy command passes on the command line, so a local `cargo clippy -p <crate>` fails on exactly what CI fails on. |
| **R5** | Add a `deny.toml`. |
| **R6** | Add a `cargo deny check advisories bans licenses` step to the CI workflow. |
| **R7** | Constraint: any lint that starts firing gets fixed, never allow-ed. |
| **R8** | Escape from R7: a lint that cannot be fixed in this branch is recorded in the lints table with a reason and a follow-up. |
| **R9** | Purpose, and the ask that judges every other: an agent session never spends itself diagnosing a failure the repository caused rather than the change did. |

## Decisions taken by the trader

- **D1** — *(R6 against R9)* The advisories check is **split** from the other
  two. `cargo deny check bans licenses` blocks in the pull-request job, because
  both are pure functions of the committed `Cargo.lock` and so can only go red
  when the code changed. `cargo deny check advisories` runs on its own
  schedule and reports without reddening an unrelated pull request. Chosen over
  running all three as one blocking step, which would let an advisory published
  overnight fail a change that never touched the dependency.
- **D2** — *(R4)* The lints table becomes the **single owner** of the levels:
  `[workspace.lints.rust] warnings = "deny"`, and the CI Lint step drops its
  `-D warnings`. Accepted consequence: `cargo check`, `cargo build` and `cargo
  test` now hard-fail on a warning too, not only `cargo clippy`. Chosen over
  stating the level in both places, which is the duplication this mission
  exists to remove.

## Assumptions

- **S1** — "the exact stable version CI runs today" is **1.98.0**, read from
  the most recent green run on `main` (run `33701431733`, which logs
  `stable-x86_64-unknown-linux-gnu … rustc 1.98.0 (88d9e12ae 2026-08-18)`).
  That is also this machine's version. Measured rather than guessed, so no
  question was owed.
- **S2** — "every third-party dependency" excludes the internal `quantick-*`
  path dependencies, which stay as `path = "../x"` in each crate. The ask says
  third-party; and `crates/pine/tests/workspace_deps.rs` enforces the one-way
  dependency graph by parsing `path = "../` lines out of the crate manifests,
  so hoisting those to the root would blind an existing architectural guard.
- **S3** — The workspace `tokio` entry carries the union of the features its
  production dependents use. `cargo build --workspace` already unifies tokio's
  features across the graph, so the compiled artifact does not change; only a
  single-crate build sees more features than before. `test-util` is the
  exception and stays a per-crate dev-dependency addition, because it swaps
  tokio's clock for a pausable one and must never reach a production build.
- **S4** — `crates/guards` gets `[lints] workspace = true` like every other
  crate. A lints table is not a dependency, so the rule that `guards` has no
  dependencies at all is untouched in substance.

  **This assumption did not survive contact intact, and the record should say
  so.** `CLAUDE.md` did not state that rule in terms of dependencies alone: it
  said "nothing may be added to its manifest", which the `[lints]` inheritance
  and `publish = false` both violate literally. Excluding `guards` from the
  lints table was not an option — with CI's `-D warnings` gone, that crate
  would have been the one crate held to no denial at all — so the rule's
  wording was changed to forbid what its own justification names: its
  `dependencies` tables stay empty. The bolded rule beside it, the comment in
  `crates/guards/Cargo.toml` and the `ALLOWED` entry in
  `crates/pine/tests/workspace_deps.rs` all justify it by build cost, and a
  `[lints]` table costs no compile. Reworded rather than reinterpreted
  silently, and called out in the pull-request body as a rule this branch
  edited.
- **S5** — The lints table carries CI-parity levels only. No `pedantic`,
  `nursery`, `missing_docs` or `unsafe_code` expansion: R4 asked for parity,
  and anything beyond it is scope nobody requested.
- **S6** — *wanted to ask* — the license allowlist in `deny.toml` is the set
  the dependency tree actually contains today, all permissive, rather than an
  aspirational policy. A copyleft or unusual license is arguably the trader's
  call; the reading taken is allow-what-is-there, and anything surprising found
  in the tree is named in the pull-request body rather than quietly allowed.
- **S7** — The `dtolnay/rust-toolchain@stable` step in CI is changed so the
  toolchain file is authoritative. R1 is meaningless if the workflow keeps
  installing a floating `stable` beside the pin.
- **S8** — `cargo-deny` is installed in CI at a pinned version. An unpinned
  install reintroduces exactly the drift R9 forbids.

## Acceptance criteria

Every criterion below is ticked against recorded evidence in
`.claude/evidence/pin-build-environment/checks.md`, which carries the
command output each one names. The two closing steps stay unticked here on
purpose: this file is archived *before* the reviews run, so neither the
`delivery-review` verdict nor the pull request exists at the moment it is
written down.

- [x] **A1** — `rust-toolchain.toml` at the repository root pins
      `channel = "1.98.0"` with the components `clippy` and `rustfmt`, and the
      CI workflow no longer installs a floating `stable` beside it.
      *Evidence:* the contents of the file, and a CI log line showing 1.98.0
      selected from it.
      → `rust-toolchain.toml`, `.github/workflows/ci.yml`, PR body. *(R1)*
- [x] **A2** — The root `Cargo.toml` carries a `[workspace.dependencies]`
      table listing every third-party dependency used anywhere in the
      workspace, and no crate manifest states a third-party version literal.
      *Evidence:* a test that fails when a crate manifest names a third-party
      version instead of inheriting it.
      → `Cargo.toml`, `crates/pine/tests/workspace_deps.rs`. *(R2, R3)*
- [x] **A3** — All seventeen crates inherit with `workspace = true` for every
      third-party dependency they use; the version and production feature set
      of `tokio` are stated once in the root instead of once per dependent.
      *Evidence:* the same test, plus each manifest read back.
      → `crates/*/Cargo.toml`. *(R3, R2)*
- [x] **A4** — The resolved dependency graph is unchanged by the migration:
      `Cargo.lock` and `cargo tree` are identical before and after the
      workspace-dependency move, feature sets included.
      *Evidence:* a recorded `cargo tree -e features` comparison showing no
      difference.
      → PR body. *(R2, R3, R9)*
- [x] **A5** — The root `Cargo.toml` carries `[workspace.lints]` with
      `warnings = "deny"`, every one of the seventeen crates carries
      `[lints] workspace = true`, and the CI Lint step no longer passes
      `-D warnings` on the command line.
      *Evidence:* the manifests, the workflow diff, and a demonstration that
      `cargo clippy -p <crate>` fails on a warning with no command-line flag.
      → `Cargo.toml`, `crates/*/Cargo.toml`, `.github/workflows/ci.yml`, PR body. *(R4, D2)*
- [x] **A6** — `deny.toml` exists at the repository root, configuring the
      advisories, bans and licenses checks.
      *Evidence:* the file, and `cargo deny check` run locally with its output
      recorded.
      → `deny.toml`, PR body. *(R5)*
- [x] **A7** — CI runs `cargo deny check bans licenses` as a blocking step in
      the pull-request job with `cargo-deny` pinned, and `cargo deny check
      advisories` on a schedule that cannot redden an unrelated pull request.
      *Evidence:* the workflow diff, and a CI run showing both.
      → `.github/workflows/ci.yml`, PR body. *(R6, D1, R9)*
- [x] **A8** — Every lint this change causes to fire is fixed in the branch.
      The diff adds no `#[allow(...)]` attribute and no `"allow"` entry that
      silences a lint this change surfaced.
      *Evidence:* every `allow` occurrence in the branch diff listed and
      accounted for.
      → PR body. *(R7)*
- [x] **A9** — Any lint that could not be fixed within the branch is recorded
      in the lints table with a reason and a linked follow-up. If none, the
      pull-request body says so explicitly rather than leaving the reader to
      infer it from an absence.
      *Evidence:* the lints table, or the explicit statement.
      → `Cargo.toml`, PR body. *(R8)*
- [x] **A10** — The property R2 and R3 buy is guarded, not merely established:
      a crate that re-states a third-party version, or a new crate that never
      inherits, fails a test rather than passing silently.
      *Evidence:* the test, and its failure demonstrated against a deliberate
      violation.
      → `crates/pine/tests/workspace_deps.rs`, PR body. *(R2, R3, R9)*

### Injected gates

- [x] **G1** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own,
      never chained behind `||`.
      *Evidence:* the four exit codes, recorded separately.
      → PR body.
- [x] **G2** — Performance impact declared: every touched code path classified
      by rate (per-trade / per-depth / per-frame / rare) as part of the plan,
      not the review.
      *Evidence:* the classification.
      → PR body.
- [x] **G3** — `arch-review` run over `git diff origin/main...HEAD`, with its
      step 0 bug pass, and every Blocker and Should-fix resolved or deferred in
      the pull-request body.
      *Evidence:* the review verdict and the `arch-review-ok` marker.
      → PR body.
- [x] **G4** — Every artifact in English, per `CLAUDE.md`. Graded by
      `arch-review` dimension 8.
      *Evidence:* the review verdict.
      → PR body.

### Not applicable, and why

- **Hot-path evidence** — no logic change is intended; this mission moves
  version strings, adds configuration, and fixes whatever lints that surfaces.
  If a lint fix lands in a per-trade, per-depth or per-frame path, the
  classification in G2 says so and the row is honoured with a measurement
  rather than waived. Waived only on the condition that no hot path is touched.
- **`ui-harness` / `visual-qa` / `trader-ux-review`** — no new or changed
  surface, and no user-visible behaviour change. A lint fix inside `app` is
  behaviour-preserving or it is a bug, which `arch-review` step 0 grades.
- **`new-extension`** — no capability is added: no feed, bar type, indicator,
  layer, panel or crate.
- **The second operator** — nothing a trader *does* is added; no action, tool,
  trade or lock.
- **Engine / determinism, test-first** — no engine behaviour change is
  intended. The existing golden tests are the guard, and they must stay green;
  a fixture written before the code would have nothing to describe here.
- **Docs/skills-only waiver** — does not apply. This branch ships
  configuration, a workflow, a test and possibly source fixes, so the full
  shape pass is owed.

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS.
- [ ] **C2** — The pull request is open.

## The request as received

Quoted verbatim and in full, in the language it was written in, because the
ledger above must not become its own source of truth: an ask dropped while
*writing* the ledger is one no reviewer could ever find. This is the single
marked, attributed quotation the language rule in `CLAUDE.md` exempts; every
other line of this file is English.

> medium Pin the build environment so that a red CI means the code changed, not
> the toolchain. Four things: add a rust-toolchain.toml pinning the exact stable
> version CI runs today, with the components clippy and rustfmt; move every
> third-party dependency to [workspace.dependencies] in the root Cargo.toml and
> have all seventeen crates inherit it, so a version and its feature set are
> stated once instead of six times for tokio; add a workspace [lints] table
> carrying the same levels the CI clippy command passes on the command line, so
> a local `cargo clippy -p <crate>` fails on exactly what CI fails on; and add a
> deny.toml with a `cargo deny check advisories bans licenses` step in the CI
> workflow. Constraint: any lint that starts firing gets fixed, never allow-ed —
> if one cannot be fixed in this branch, record it in the lints table with a
> reason and a follow-up. So that an agent session never spends itself
> diagnosing a failure the repository caused rather than the change did.
