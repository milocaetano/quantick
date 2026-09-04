# Mission: generated truth — what is generated cannot drift

## Objective

Make every hand-maintained index in this repository generated from the code it
describes, invert the rule that tells an agent to trust the document over the
source, and correct the four recipes the code falsifies.

**Why it matters.** Six of eleven independent audit lenses converged on one
defect class: a hand-maintained index that claims currency, that the code
contradicts, with no mechanical enforcement. The aggravating factor is
`docs/control-plane/README.md` §Precedence, which tells an agent that documents
outrank source and that implementation stops until they are reconciled. An
obedient agent therefore reads a stale roadmap and either halts or
re-implements shipped code. The repository already proved the fix works —
`crates/control/examples/export_schemas.rs` emits the contract schemas and
`crates/control/tests/schema_snapshots.rs` diffs the committed copies, which is
why the read contracts cannot drift. This mission applies that pattern to the
three indexes where it was never applied.

**Tier:** `medium`. The work is multi-crate and adds a new guard module, a
generator and a startup diagnostic, so it earns more than `small`'s waived
interrogation and waived `delivery-review`. It does not touch money, order
placement, autonomy or an irreversible surface, and the design fork was settled
in two questions rather than four, so it does not reach `high`.

## Request ledger

Derived from the brief at `C:\src\mission-generated-truth.md`, quoted verbatim
in the final section.

### The judging ask

- **R1** — What is generated cannot drift: every index this mission touches is
  either derived from the code or mechanically compared against it, so the
  class of defect cannot silently return. *(A11)*

### Scope item 1 — the precedence rule

- **R2** — Invert `docs/control-plane/README.md` §Precedence: code is
  authoritative for *what is registered*; the control contract stays
  authoritative for *wire rules*. *(A1)*
- **R3** — Reconcile `docs/README.md:6-8`, which echoes the old rule. *(A1)*

### Scope item 2 — generate the capability inventory

- **R4** — Add a dump path emitting registered capability IDs, module,
  permissions and version — a `quantick-mcp` subcommand or an `examples/`
  binary in the shape of `crates/control/examples/export_schemas.rs`. *(A2)*
- **R5** — Commit the generated table as `capability-inventory.md`. *(A2)*

### Scope item 3 — guard the parity

- **R6** — A new module in `crates/guards/` — zero dependencies, ratchet
  machinery in `ratchet.rs` is the right shape — failing when the committed
  generated table and the registry diverge. *(A3, A4)*
- **R7** — The same guard fails when a `QUANTICK_*` read in `crates/app/src`
  has no row in the hook registry, or a row has no read. *(A5)*
- **R8** — A signed allowlist for non-UI config vars. *(A5)*

### Scope item 4 — make the hook registry derived

- **R9** — A `HookSpec` declaration at each hook's own definition site, as a
  registration line per the trunk rule. **D1: delivered in full, all ~130
  sites, no split.** *(A6)*
- **R10** — Regenerate the registry markdown from the specs. *(A7)*
- **R11** — Log `UNKNOWN_HOOK` at startup for any `QUANTICK_*` in the
  environment matching no spec, so a dead hook fails loudly instead of
  presenting as "the surface did not open". *(A8)*

### Scope item 5 — relocate the archaeology

- **R12** — Move `roadmap.md` §2-§5 and the six `pr*-evidence.md` files under
  `docs/control-plane/history/` with a one-line *"archaeology, not current
  state"* banner. *(A9)*
- **R13** — Do not delete them; they are correctly dated and nothing links to
  them as current. *(A9)*

### Scope item 6 — the four falsified recipes

- **R14** — The indicator docking row in `new-extension`: the recipe claims "a
  new file plus one line"; reality is a `SavedKind` variant plus ~31 call sites
  across six files. *(A10)*
- **R15** — The feeds clause in `CLAUDE.md`'s headless bullet: the rule names
  the feeds as no-async / no-clock, and the feeds hold async throughout plus
  three clock reads. *(A10)*
- **R16** — The `harness.rs` ownership claim at `ui-harness/SKILL.md:47-50`:
  it is claimed to own every launch hook; it reads 26 of 130. *(A10)*
- **R17** — The stale `-D warnings` lines, CI having deliberately dropped it.
  *(A10)*

### Constraints the brief states as asks

- **R18** — Do not compress the registry prose. The long rows explain which
  class of defect is invisible without that hook; generation must preserve
  them verbatim. *(A7)*
- **R19** — Re-verify every evidence-ledger line against the current tree
  before fixing it; the numbers were measured at `bc39248` and may have aged.
  *(A12)*
- **R20** — `cargo test -p quantick-guards` must stay near one second and add
  no dependency to `crates/guards/Cargo.toml`. *(A3)*

### Withdrawn

- ~~**R21** — Fix the three documented MCP tools that do not exist
  (`quantick_control`, `quantick_pine`, `quantick_replay`) and the
  undocumented `quantick_delete_everything`.~~ **Withdrawn: the evidence is
  false.** Re-verification under R19 found that the three names are Rust
  *crate* paths, not tool names — `quantick_control::limits` at
  `docs/control-plane/roadmap.md:236`, `quantick_pine::compile` at `:345`,
  `quantick_replay::context_path` at `:648`. `quantick_delete_everything` is a
  string literal inside `#[cfg(test)] mod tests` at
  `crates/mcp/src/server.rs:470`, in an assertion that an *unknown* tool name
  is rejected with `INVALID_PARAMS`. The audit lens confused crate paths with
  tool names. No scope item depended on this line, so nothing else moves.

## Decisions taken by the trader

- **D1** — Scope item 4 ships **complete, with no split**: a `HookSpec` at
  every hook definition site, the markdown regenerated from them, and the
  `UNKNOWN_HOOK` startup diagnostic with a test. The alternatives offered were
  landing only the diagnostic, or deferring the item entirely behind the
  item-3 parity guard; both were declined.
- **D2** — The 69,177 bytes of registry prose **stay in the markdown**. The
  `HookSpec` in the code declares the mechanical fields the guard compares —
  name, surface, value shape, owning module — and the generator fuses the two
  halves. Chosen over migrating the prose into doc comments (which the size
  ratchet counts as production lines, against a `harness.rs` already at 1,135
  of a 1,500 threshold) and over a new `hook_specs/` module tree. R18 is
  satisfied by preservation, not by relocation.

## Assumptions

- **S1** — The guard from R6/R7 reads the registry source **textually** — it
  parses `*_CAPABILITY_ID` constants and `QUANTICK_*` reads out of the files —
  rather than linking the real registry. Forced rather than chosen: R20
  forbids a dependency in `crates/guards/Cargo.toml`, and the registry lives in
  `crates/app`, the slowest crate in the repository.
  `crates/guards/tests/session_gap_agreement.rs` is the existing precedent for
  a guard that reads sources it cannot link.
- **S2** — "Registry" in scope items 2 and 3 means the `*_CAPABILITY_ID`
  constants under `crates/app/src/control/` — 26 of them at `bc39248` — since
  that is what the evidence ledger's line 3 cites as the code side.
- **S3** — The generated `capability-inventory.md` replaces the current file in
  place rather than living beside it. The current file's own preamble says it
  "must not become a hand-maintained second copy" of the runtime registry,
  which reads as consent to the replacement; its dated PR-0 framing goes to
  `history/` with the other archaeology under R12.
- **S4** — *Wanted to ask, budget spent on D1 and D2.* The signed allowlist of
  R8 takes the shape of the existing baselines — a line-oriented data file
  beside `size-baseline.txt` and `context-baseline.txt`, each entry carrying a
  reason — rather than a Rust `const` array. `size-baseline.txt`'s own header
  states why that shape was chosen for the ratchets, and the same two reasons
  (worktrees conflicting over prose-heavy source files, machine rewriting)
  apply here unchanged.
- **S5** — *Wanted to ask, budget spent.* The `UNKNOWN_HOOK` line of R11 goes
  to the application's existing logging sink at warn level, not to stderr
  directly and not as a panic. A dead hook should be loud, and the brief says
  "fails loudly", but a panic at startup on an unrecognised environment
  variable would make a typo unbootable, which is a worse failure than the one
  being fixed.
- **S6** — The `-D warnings` fix of R17 covers the two documents that still
  carry it, `docs/mcp-control-plane-development-plan.md:1309` and
  `docs/indicator-system-plan.md:683`. Re-verification under R19 found the
  third had already been fixed: `CLAUDE.md`, `AGENTS.md` and `CONTRIBUTING.md`
  already state one `-D warnings`-free clippy line, and `CONTRIBUTING.md:44`
  explains the absence. The acceptance criterion "one clippy command across
  `CLAUDE.md`, `CONTRIBUTING.md` and `AGENTS.md`" is therefore graded as
  already-true rather than as work, and A10 records the check.
- **S7** — Branch prefix `fix/`, slug `generated-truth`, worktree at
  `../quantick-worktrees/fix-generated-truth`. Repository convention; the brief
  named the branch itself.

## Acceptance criteria

- [x] **A1** — `docs/control-plane/README.md` §Precedence states that code is
      authoritative for what is registered and the control contract for wire
      rules, and `docs/README.md` no longer tells a reader the plan and
      contract outrank the code.
      *Evidence:* both sections quoted in full, before and after.
      → `.claude/evidence/generated-truth/precedence.md`. *(R2, R3)*
- [x] **A2** — A committed dump path emits every registered capability ID with
      its module, permissions and version, and `capability-inventory.md` is its
      output rather than prose. Running the dump on a clean tree reproduces the
      committed file byte for byte.
      *Evidence:* the command, its exit code, and a `git diff --exit-code` over
      the generated file showing no change.
      → `.claude/evidence/generated-truth/inventory-regen.log`. *(R4, R5)*
- [x] **A3** — `cargo test -p quantick-guards` passes, runs in under two
      seconds wall clock, and `crates/guards/Cargo.toml`'s `[dependencies]`
      table is still empty.
      *Evidence:* timed run plus the quoted `Cargo.toml` section.
      → `.claude/evidence/generated-truth/guards-timing.log`. *(R6, R20)*
- [x] **A4** — Documented-capability ↔ registered-capability delta is 0, and
      hand-editing the committed generated table reddens the guard.
      *Evidence:* a green run, then a deliberate one-line edit to the generated
      table, then the guard's failure output naming the divergence, then the
      edit reverted and the guard green again.
      → `.claude/evidence/generated-truth/parity-redness.log`. *(R6, R1)*
- [x] **A5** — Every `QUANTICK_*` read in `crates/app/src` has a registry row
      and every row has a read, the nine undocumented hooks and the
      `QUANTICK_DRAWING_MANAGER` / `QUANTICK_DRAWINGS_MANAGER` mismatch
      included; the exceptions that remain are non-UI config vars listed in a
      signed allowlist, each with a reason. Hand-editing the generated registry
      reddens the guard.
      *Evidence:* a green run, the allowlist file quoted, and the same
      edit-then-redden cycle as A4.
      → `.claude/evidence/generated-truth/hook-parity.log`. *(R7, R8)*
- [x] **A6** — Every hook the application reads carries a `HookSpec`
      declaration at its own definition site, added as a registration line
      rather than surgery on a central file.
      *Evidence:* the spec count, the read count, and their difference stated
      as zero, with the registration shape shown for one UI hook and one
      floating-surface hook.
      → `.claude/evidence/generated-truth/hookspec-coverage.md`. *(R9)*
- [x] **A7** — `hook-registry.md` is regenerated from the specs, and every row
      of prose that stood before the change stands after it, uncompressed.
      *Evidence:* the regeneration command's output, plus a byte count and a
      row count of the prose cells before and after showing no loss.
      → `.claude/evidence/generated-truth/registry-regen.log`. *(R10, R18)*
- [x] **A8** — A `QUANTICK_*` variable in the environment matching no spec
      produces a visible `UNKNOWN_HOOK` line at startup, covered by a test that
      fails if the diagnostic is removed.
      *Evidence:* the named test and its passing output.
      → `.claude/evidence/generated-truth/unknown-hook.log`. *(R11)*
- [x] **A9** — `roadmap.md` §2-§5 and the six `pr*-evidence.md` files live
      under `docs/control-plane/history/`, each carrying the one-line
      archaeology banner, none deleted; and no document outside `history/`
      states a delivery status the registry contradicts — PR 5c and PR 6 in
      particular, both of which shipped.
      *Evidence:* the file listing before and after, the banner text, and a
      grep over the non-`history/` documents for delivery-status claims with
      each surviving claim checked against the registry.
      → `.claude/evidence/generated-truth/archaeology.md`. *(R12, R13)*
- [x] **A10** — The four recipes match the code: the `new-extension` indicator
      row is verified against the last real indicator addition and lists every
      site that addition touched; `CLAUDE.md`'s headless bullet states what is
      actually true of the feeds; `ui-harness/SKILL.md` no longer claims
      `harness.rs` owns every launch hook; and one clippy command stands across
      `CLAUDE.md`, `CONTRIBUTING.md` and `AGENTS.md`, with the two stale
      `-D warnings` lines in the plan documents corrected.
      *Evidence:* the four before/after pairs, and for the indicator row the
      git archaeology of the addition it was checked against.
      → `.claude/evidence/generated-truth/recipes.md`. *(R14, R15, R16, R17)*
- [x] **A11** — Each of the two generated files, and the hook parity, is
      defended by a guard that a hand edit reddens; no index this mission
      touches remains hand-maintained without a mechanical comparison behind
      it.
      *Evidence:* the three redden-on-edit demonstrations collected in one
      place with the guard names.
      → `.claude/evidence/generated-truth/cannot-drift.md`. *(R1)*
- [x] **A12** — Every line of the brief's evidence ledger is re-verified
      against the tree before it is acted on, with the result recorded — hit,
      aged, or false.
      *Evidence:* the ten lines with their re-verification verdict and the
      commands that produced it.
      → `.claude/evidence/generated-truth/ledger-recheck.md`. *(R19)*

### Injected gates

- [x] **G1** — Every artifact in English, per `CLAUDE.md`'s rule, its scope and
      its exemptions. The verbatim-request section below is the one marked,
      attributed quotation the exemption describes.
      *Evidence:* `arch-review` dimension 8 verdict and
      `cargo test -p quantick-guards` green.
      → the PR body.
- [x] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`, each run on its own
      rather than chained.
      *Evidence:* four separate runs with their exit codes.
      → `.claude/evidence/generated-truth/checks.log`.
- [x] **G3** — Performance impact declared: every touched path classified by
      rate — per-trade, per-depth, per-frame or rare — as part of the plan.
      *Evidence:* the classification table.
      → the PR body.
- [x] **G4** — `arch-review` run over `git diff origin/main...HEAD` with its
      step-0 bug pass at `low`, every Blocker and Should-fix resolved or
      deferred in the PR body.
      *Evidence:* the `arch-review-ok` marker and the verdict.
      → the PR body.
- [ ] **G5** — CI green, including `sh .claude/hooks/guardrails_test.sh` for
      the hook changes.
      *Evidence:* `gh pr checks` output.
      → the PR body.

### Not applicable, and why

- **Hot path.** The `UNKNOWN_HOOK` scan of R11 runs once at startup over the
  process environment, and the generators run offline. Nothing this mission
  adds executes per trade, per depth update or per frame, so the hot-path
  evidence gate is not injected. G3 records the classification that establishes
  this rather than assuming it; if the `HookSpec` work of R9 turns out to move
  a per-frame read, the gate is injected then.
- **User-visible surfaces.** No surface is added, removed or restyled. The
  `HookSpec` work of R9 changes how a hook is *declared*, not what it opens,
  and A5 proves the set of reachable surfaces is unchanged by construction.
  So `visual-qa` and `trader-ux-review` are not injected. `ui-harness` is
  followed in the sense that matters here — every hook keeps its registry row,
  which is that skill's rule — but no new surface needs a new hook.
- **Adds a capability.** The dump path of R4 and the guard of R6 are developer
  tooling, not a feed, bar type, indicator, layer or panel a trader reaches.
  `new-extension` is edited by this mission (R14), not followed by it.
- **Something a trader does.** Nothing here places, cancels or reads an order,
  and nothing adds an act a trader performs. The second-operator criteria are
  already satisfied by the dump path being a named command with a readable
  result.
- **Engine / determinism.** No engine code is touched. The generators must be
  deterministic — A2 and A7 require byte-identical regeneration, which is the
  same property under a different name — but no fixture-and-golden engine test
  is owed.
- **Docs/skills waiver.** Not claimed. The branch ships Rust — a guard module,
  a dump path, `HookSpec` declarations and a startup diagnostic — so
  `arch-review` takes the full shape pass, not the prose waiver. The documents
  changed alongside are graded under the same pass.

## Deviations, and why

Every one of these is a place the delivered branch differs from the plan above.
Recorded here rather than in the pull request alone, because this file is what
`delivery-review` grades and a deviation it cannot see is one nobody checks.

- **S2 was wrong about the registry's size, and the work corrected it.** The
  assumption said 26 capabilities, taken from the brief's evidence ledger. The
  registry holds **38**: twelve `layout.*` and `feed.*` capabilities used a
  bare `_ID` suffix instead of `_CAPABILITY_ID`, so the ledger's grep had not
  seen them. They are renamed, which makes the naming rule exact and lets the
  zero-dependency guard be exact with it. This is the mission's own argument
  turned on its own brief: generating the file produced the true number, where
  counting by hand had produced a wrong one twice.

- **`HookSpec` carries a name, not a value grammar.** D2 asked for name,
  surface, value shape and module. Name and module are there (module via the
  `OWNERS` table, which the guard checks against the real file). The value
  grammar is **not**, deliberately: `docs/ui-harness/hook-prose.md` already
  states it in every `Hook` cell, and a second copy kept by hand in the code
  is precisely the duplicated truth this mission exists to end. It was in the
  first draft, went unread by anything, and was removed rather than given a
  fake consumer.

- **The prose lives at `docs/ui-harness/hook-prose.md`, not beside the skill.**
  D2's ruling — prose stays in markdown, the code declares the mechanical
  facts, the generator fuses them — is delivered exactly. The *path* moved
  because the context ratchet weighs every `.md` under `.claude/skills/`: a
  second seventy-kilobyte copy there would have charged every session twice
  for the same words. A session still loads only the generated file.

- **The archaeology move took roadmap sections 1 and 6-8 as well as 2-5.**
  R12 named 2-5. Section 6 is titled "Gates for every pull request in section
  5" and cannot stand without it; sections 7 and 8 state the delivery status
  of a stack that has since closed, which is the exact claim A9 forbids
  outside `history/`; and section 1's table was the document's most-read half
  and its least true. Nothing was deleted, and `roadmap.md` remains, rewritten
  to state no delivery status of its own.

- **126 hooks, not the ~130 the brief implied, and the guard is why.** Three of
  the 129 `QUANTICK_*` names the first sweep found are not launch hooks:
  `QUANTICK_GIT_COMMIT` is read through `option_env!` at compile time, and
  `QUANTICK_FAKE_STORE` and `QUANTICK_TEST_STORE_HOME_ENV` live inside their
  modules' own `#[cfg(test)]` blocks. Each is on the guard's signed allowlist
  with its reason. The parity guard forced the question rather than letting
  three fictions be documented as hooks.

- **Both ratchets were raised, signed, and paid in the open.**
  `crates/guards/size-baseline.txt` +13 production lines (`paper_trading.rs`
  +11, `footprint_render.rs` +2 — one line per hook those files read);
  `crates/guards/context-baseline.txt` +935 bytes, net of 43 stale staging
  lines and 22 duplicate table headers that generation removed. Each carries
  the measured breakdown in its baseline. Nothing was extracted in return, and
  both notes say so: these are raises, not trades, and a reviewer should
  dispute them if the guarantee is not worth the lines.

- **R17 had a fourth stale file the brief did not name**, and it is fixed:
  `.github/PULL_REQUEST_TEMPLATE.md` still prescribed `clippy -- -D warnings`.
  The brief's three were down to two, one having been fixed already (S6).

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS over the branch as shipped.
- [ ] **C2** — The pull request is open, its body naming the `medium` tier
      beside the four verification boxes.

## The request as received

Quoted verbatim and untranslated: this is the marked, attributed quotation
`CLAUDE.md`'s language rule exempts, and the ledger above must remain checkable
against the trader's own words rather than against a paraphrase of them. The
first paragraph is the session invocation; the brief it points to is
`C:\src\mission-generated-truth.md`, whose *Evidence ledger*, *Scope*,
*Acceptance criteria*, *Out of scope* and *Housekeeping* sections are the rest
of the request and are reproduced in this file's ledger above.

> medium fix/generated-truth — leia C:\src\mission-generated-truth.md por
> inteiro antes de qualquer outra coisa: é o briefing completo desta missão,
> com o ledger de evidências (cada alegação com file:line), o escopo em seis
> itens, os critérios de aceite e o que está explicitamente fora de escopo.
> Re-verifique cada linha do ledger contra a árvore atual antes de corrigir —
> os números foram medidos no commit bc39248 e podem ter envelhecido. Monte o
> ledger de pedidos a partir do arquivo.

And from the brief itself, the objective sentence it asks the mission to carry:

> every hand-maintained index in this repo that claims to describe the code has
> drifted from it, and one document instructs agents to trust the document over
> the code. Apply the pattern this repo already proved works
> (control_plane_tests.rs:4692 regenerates the 59 observer schemas and diffs
> them, which is why the read contracts cannot drift) to the three indexes
> where it was never applied.

## Out of scope, from the brief

- Any `pane.rs` or `crates/app` decomposition.
- Adding the `crates/app` library target and moving its tests out of the
  single-binary loop.
- Narrowing `quantick_describe`'s response size.
- Deleting the `GOAL-archive-*.md` files or the evidence directories.
- Any LLM-judged or nondeterministic eval gating a pull request.
