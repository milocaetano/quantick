# Mission: default the MCP adapter's `quantick_invoke` to the newest registered version of each capability

When an MCP agent calls `quantick_invoke` without `capability_version`, the
adapter resolves the version to the highest one the connected instance
registers for that capability ID, read from the instance's own
`control.describe` registry; an explicit version passes through unchanged.
Why it matters: Q2 (PR #421) added version 2 of seven `layout.*` capabilities
and kept version 1 registered, and v1 now answers an honest refusal pointing
at v2. The adapter's hard-coded default of 1 sends every version-less MCP call
to that refusal instead of the working answer.

Campaign child of https://github.com/milocaetano/quantick/issues/367 (task key
Q11); task issue https://github.com/milocaetano/quantick/issues/422. Base:
`origin/campaign/lean-a-plus` at `c55a4222`; PR base exactly
`campaign/lean-a-plus`.

**Tier:** `medium` — it changes the default of a published tool (the trader
authorized it as D20), in one leaf crate with no UI and no hot path; the
campaign parent classes the risk as medium. A `low` bug pass, the full shape
pass on the touched dimensions and an inline `delivery-review` completeness
pass are owed.

## Request ledger

- **R0** — Purpose: an MCP agent that omits the version gets the working
  answer of the newest version, not v1's refusal. *"so an MCP agent that omits
  the version gets the refusal instead of the working answer"*.
- **R1** — An omitted version resolves to the newest version the connected
  instance registers for that capability, from the discovery/registry data the
  adapter already reads, not a hard-coded 1 or a hard-coded table. Issue scope
  bullet 1; D20 *"padrão = mais nova"*; coordinator D1.
- **R2** — An explicit version passes through unchanged. Issue scope bullet 1;
  D20 *"a caller that asks for v1 explicitly still gets v1"*.
- **R3** — A capability registered only at v1 still works without a version;
  an ID the registry does not list keeps today's error path. Issue scope
  bullet 3; coordinator D1.
- **R4** — The tool's published description and argument schema say
  "omitted: the newest version the instance registers"; the MCP tool
  documentation is updated; a published MCP schema snapshot, if one exists, is
  regenerated through the normal mechanism and its diff shown. Issue scope
  bullet 2, A3; coordinator D2.
- **R5** — Tests in `crates/mcp/tests/` against the fake gateway: omitted
  version picks the newest of two; explicit v1 gets v1; v1-only works without
  a version. Issue scope bullet 3, A1, A2; coordinator D3.
- **R6** — Merged into `campaign/lean-a-plus` through a PR whose base is
  exactly that branch, with the merge read back. Issue A4. The child owns the
  PR base; the merge and its readback are the coordinator's.

## Decisions

- **D1** (coordinator) — omitted version resolves to the highest version the
  connected instance registers for that ID, read from registry/discovery data
  the adapter already has; explicit version unchanged; no listed version keeps
  today's error path.
- **D2** (coordinator) — say it in the published description and argument
  schema; regenerate a published MCP schema snapshot if one exists.
- **D3** (coordinator) — fake-gateway tests: v1+v2 without a version reaches
  v2; `version: 1` reaches v1; v1-only works without a version.
- **D4** (coordinator) — tier `medium`, `code-review` at `low`, shape pass on
  the touched dimensions, `ai-review` completion, inline `delivery-review`
  completeness pass; at most three step-0 rounds.
- **D5** (coordinator) — `gh pr ready` once, cd-prefixed, from the Bash tool.
- **D20** (trader, https://github.com/milocaetano/quantick/issues/367#issuecomment-5650222968)
  — *"Sim, padrão = mais nova (Recommended)"*.

## Assumptions

- **S1** — "The registry the adapter already reads" is `control.describe`:
  `quantick_search_capabilities` already reads it, and it lists every
  registered `(id, version)` row (`crates/app/src/control/contract.rs`
  `describe`). The resolution asks it once per version-less call rather than
  caching it — safe because the registry is the instance's to change and a
  cache would need its own invalidation; the cost is one local round trip on
  an agent-initiated call.
- **S2** — "Today's error path" for an ID the registry does not list means
  forwarding at version 1 exactly as before, so the instance's own refusal
  (`control.capability_unknown`, `control.permission_denied`) answers
  unchanged. A describe that fails leaves version 1 as well, and the call
  itself meets and reports whatever stopped it, so a caller never receives
  describe's error for a call it did not make. When describe answers, the call
  is sent to the instance that answered it, so the version and the answer come
  from one registry. Safe: both keep every existing refusal the caller could
  see. (Revised after the step-0 bug pass noted the describe error and the
  two-routing race; tests `a_refused_describe_leaves_the_first_version_rather_than_its_error`
  and `a_resolved_version_is_sent_to_the_instance_whose_registry_chose_it`.)
- **S3** — The success result of `quantick_invoke` carries the
  `capability_version` that answered, so a caller that omitted it can read
  which version it reached (operability: a readable result). Safe: the tool
  declares no output schema, the field is additive, and data honesty asks that
  an inferred value be labelled.
- **S4** — The named tools (`quantick_get_snapshot` and the rest) stay pinned
  at version 1: their input and output schemas are the committed v1 documents
  they embed, so resolving them to a newer version would publish a schema the
  instance no longer answers with. Out of scope for D20, which names
  `quantick_invoke`.

## Acceptance criteria

- [x] **A1** — A fake-gateway test registers one capability at v1 and v2,
      invokes it through `quantick_invoke` without a version, and the gateway
      receives version 2 and the result reports `capability_version: 2`.
      *Evidence:* `cargo test -p quantick-mcp --test fake_gateway` passing,
      test name in the PR body. → `crates/mcp/tests/fake_gateway.rs`. *(R0,
      R1, R5)*
- [x] **A2** — The same test invokes it with `capability_version: 1` and the
      gateway receives version 1. *Evidence:* same test run. →
      `crates/mcp/tests/fake_gateway.rs`. *(R2, R5)*
- [x] **A3** — A v1-only capability invoked without a version reaches v1, and
      an ID the instance does not register still answers
      `control.capability_unknown`. *Evidence:* same test run plus the
      existing `paper.order.place` assertion still green. →
      `crates/mcp/tests/fake_gateway.rs`. *(R3, R5)*
- [x] **A4** — The version comes from the instance's describe document: a
      unit test over a describe document picks the highest version of the
      matching ID and `None` for an unlisted one; no version table exists in
      the crate. *Evidence:* unit test in `crates/mcp/src/tools.rs`; grep
      shows no version literal besides the fallback. → `crates/mcp/src/tools.rs`.
      *(R1)*
- [x] **A5** — The published `quantick_invoke` description and the
      `capability_version` schema say an omitted version resolves to the
      newest version the instance registers; `crates/mcp/README.md` and
      `docs/control-plane/control-contract.md` say the same; the PR body
      states whether an MCP tool-schema snapshot exists (none found:
      `grep` for the tool description outside `tools.rs`). *Evidence:* a
      unit test asserting the schema text, the doc diff, the PR body. →
      `crates/mcp/src/tools.rs`, `crates/mcp/README.md`,
      `docs/control-plane/control-contract.md`. *(R4)*
- [ ] **A6** — The PR's base is exactly `campaign/lean-a-plus`. *Evidence:*
      `gh pr view <n> --json baseRefName`. → PR body. *(R6)*

## Injected gates

- [ ] **G1** — Every artifact in English (`CLAUDE.md`). *Evidence:*
      `arch-review` dimension 8, `cargo test -p quantick-guards`. → review
      report.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES
      cargo test --workspace` green, each run separately, on the campaign
      base. *Evidence:* command exits in the PR body. → PR body.
- [x] **G3** — Performance impact declared. Touched paths: the
      `quantick_invoke` dispatch in the MCP adapter, rate **rare**
      (agent-initiated, one call per tool call); a version-less call adds one
      `control.describe` round trip on the local loopback. No per-trade,
      per-depth or per-frame path is touched. *Evidence:* this line and the PR
      body. → PR body.
- [ ] **G4** — `arch-review` (step 0 `code-review` at `low`, shape pass on
      operability, English and the leaf/headless rule) with every Blocker and
      Should-fix resolved or deferred in the PR body. *Evidence:* review
      verdict and `arch-review-ok` marker. → PR comment, git dir.
- [x] **G5** — Operable without a hand: the default is reachable by a named
      call (`quantick_invoke`), readable in its result (`capability_version`)
      and discoverable in the published schema. *Evidence:* A1, A5. → tests.
- [ ] **G6** — `ai-review` completion recorded with zero unresolved threads.
      *Evidence:* PR report and `ai-review-complete`. → PR, git dir.

## Evidence (run at `f677d4c2` on base `c55a4222`; the four checks rerun at `204d5b2c` after rebasing onto the campaign tip `eb60ed41`, again at `ac969a6d` after the step-0 repair, and at `69d57928` after the ai-review repair)

- A1, A2, A3 — `an_omitted_version_reaches_the_newest_registered_one_and_an_explicit_one_is_kept`
  in `crates/mcp/tests/fake_gateway.rs`: no version reaches v2 and reports
  `capability_version: 2`; `capability_version: 1` reaches v1; the
  v1-only `snapshot.read` answers at v1 without one; an explicit v3 is
  `control.capability_unknown`. The existing `paper.order.place` assertion
  (no version, unregistered ID, `control.capability_unknown`) stays green.
  `cargo test -p quantick-mcp`: 36 + 1 + 5 + 3 passed, 0 failed at `69d57928`.
- A4 — `an_omitted_version_is_the_highest_row_the_registry_lists_for_that_id`
  and `the_version_resolution_reads_the_keys_the_describe_contract_requires`
  (added for ai-review finding AI-Q11-1, thread `PRRT_kwDOTfuoRs6h17QB`) in
  `crates/mcp/src/tools.rs`; the only version literal left is
  `FIRST_CAPABILITY_VERSION`, the named tools' pin and the unlisted-ID
  fallback.
- A5 — `the_invoke_schema_publishes_the_newest_version_default`; the
  schema's `default: 1` is gone and its description reads "Omitted: the
  newest version the instance registers for capability_id". Docs:
  `crates/mcp/README.md` (the `quantick_invoke` row) and
  `docs/control-plane/control-contract.md` (`quantick_invoke` section). No
  MCP tool-schema snapshot exists: the tool description appears nowhere
  outside `crates/mcp/src/tools.rs` (the development plan carries an older
  one-line summary, not a snapshot), so nothing was regenerated.
- G2 — `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace
  --all-targets` exit 0; `cargo build --workspace` exit 0; `env -u
  QUANTICK_BUBBLES cargo test --workspace` exit 0, 0 failed (96 result
  lines at `f677d4c2`, 97 at `204d5b2c`, `ac969a6d` and `69d57928`; the rebase added a binary); `cargo test -p quantick-guards` green.
- G3 — rate **rare**, declared above; one extra local `control.describe`
  round trip per version-less `quantick_invoke`.
- G5 — the result's `capability_version` (A1) and the published schema (A5).

## Not applicable

- *Touches a hot path* — no per-trade, per-depth or per-frame code; the MCP
  adapter runs in its own process on agent-initiated calls.
- *Touches anything user-visible* — no UI surface; the adapter is a STDIO
  server.
- *Adds a capability* — no new capability, port or crate; the default of an
  existing tool changes.
- *Engine / determinism territory* — the engine is untouched.

## Closing steps

- **C1** — `delivery-review` completeness pass (inline, tier `medium`)
  returns PASS and records `delivery-review-ok`.
- **C2** — Draft PR opened against `campaign/lean-a-plus`, CI green at the
  final head, `gh pr ready` accepted once.

## The request as received

Attributed quotation of the coordinator's delegated request (campaign #367,
child Q11), verbatim:

> You are executing campaign child mission **Q11** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: make the MCP adapter's `quantick_invoke` default to the newest registered version of each capability when the caller omits the version. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision is reported back, never guessed.
>
> ## Decisions
> - D1: an omitted version resolves to the highest version the connected instance registers for that capability id, read from the registry/discovery data the adapter already has (no hard-coded table); an explicit version passes through unchanged; if the registry lists no versions for the id, keep today's error path.
> - D2: say it in the tool's published description and argument schema ("omitted: the newest version the instance registers"); if a published MCP schema snapshot exists, regenerate it through the repo's normal mechanism and show the diff in the PR.
> - D3: tests with the fake gateway: a capability registered at v1 and v2 is invoked without a version and reaches v2; with `version: 1` reaches v1; a v1-only capability works without a version.
> - D4: tier `medium`: `arch-review` with `code-review` at `low`, shape pass on the touched dimensions (operability, English, the leaf rule); `ai-review` completion; `delivery-review` completeness pass inline. At most three step-0 rounds.
> - D5: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved or the PR is DIRTY, stop and report. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**

Task issue #422, as filed (attributed quotation, verbatim scope and
criteria):

> ## Scope
>
> - `quantick_invoke` resolves an omitted version to the newest version the connected instance registers for that capability (from the discovery/registry the adapter already reads), not a hard-coded 1; an explicit version passes through unchanged.
> - The tool's published description and schema say so; the MCP tool documentation is updated.
> - Tests in `crates/mcp/tests/` (the fake gateway) for: omitted version picks the newest; explicit v1 still gets v1; a capability with only v1 still works.
>
> ## Acceptance criteria
>
> - [ ] A1: omitted version resolves to the newest registered version, proven by a fake-gateway test with two versions of one capability.
> - [ ] A2: explicit version is honoured, proven by a test.
> - [ ] A3: the published tool description/schema and docs state the default; any schema snapshot updated through the repo's normal regeneration with the change reviewed.
> - [ ] A4: merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, with the merge read back.
