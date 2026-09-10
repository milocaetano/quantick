# Mission — honour the published idempotency contract

**Objective:** Honour the published idempotency contract by accepting and
deduplicating idempotency keys in the running control-plane gateway for
capabilities that declare `IdempotencyPolicy::Optional`.

**Why it matters:** Eleven `layout.*` capabilities, `feed.reconnect`,
`feed.reload` and the `trade.*` shaping family publish
`IdempotencyPolicy::Optional`, and their doc comments promise it in prose —
`crates/app/src/control/layout.rs:503` says "so a client may retry a dropped
call without wondering what the first one did". The running gateway refuses
every such key at `crates/app/src/control/contract.rs:1367`. A client that
reads the descriptor and does the correct thing receives `invalid_request`.
The published contract is not the one the gateway honours, which is the AP4
blocker and A+ gate 5 in the campaign score ledger.

**Tier:** high. The work changes a published capability contract, adds
retained state to the gateway on an authority boundary, and has to scope that
state per principal so one connection cannot read another's recorded result.
None of that is `small` or `medium` territory: a wrong scope is a data leak,
and the contract change needs the full interrogation and both reviews.

**Campaign child:** Q14 of milocaetano/quantick#330, issue
milocaetano/quantick#359. Base `origin/campaign/architecture-a`.

## Request ledger

- **R1** — Resolve item 3 from the assessment: the live gateway must stop
  refusing an `idempotency_key` on a capability whose descriptor declares
  `Optional`. Verbatim fragment: *"resolver o item 3"*.
- **R2** — Open a pull request whose base is the campaign branch, not `main`.
  Verbatim fragment: *"fazer pr pra branch"*.
- **R3** — Carry the work through to an actual merge into that branch under
  the existing authorization. Verbatim fragment: *"Faça mesmo memso o merge
  tem autorização"*.
- **R4** — *(closing statement of purpose, carried from the assessment this
  request answers)* The point of item 3 is to make the published contract
  truthful so that A+ gate 5 stops being blocked and AP4 stops being the
  weakest criterion. This is the ask that judges the others: a fix that made
  the contract honest by removing capability would satisfy R1 literally and
  fail R4.

## Decisions

- **D1** *(R1, R4)* — Honour the key in the real gateway: accept and
  deduplicate, reusing the design already written in
  `crates/control/src/fake.rs`. Rejected alternative: downgrade the
  descriptors to `Forbidden`, which resolves R1 by removing the retry
  guarantee and fails R4.
- **D2** *(R1)* — Idempotency only. `expected_revisions` stays refused:
  `crates/app/src/control/recovery.rs` documents that refusal as deliberate
  and designs around it. `dry_run` is untouched: no descriptor in the tree
  declares `dry_run_supported: true`, so refusal and contract already agree.
- **D3** *(R2, R3)* — Formal campaign child: issue #359 under #330, branch
  `fix/idempotency-contract` cut from `origin/campaign/architecture-a`,
  `mission-base` record in the worktree's private git directory, PR based
  exactly on the campaign branch, parent index updated.
- **D4** *(R3)* — This container has no `gh`, so `pr-gate` and
  `campaign_context.sh` cannot run and the PR and merge go through the GitHub
  MCP tools. Both reviews still run and both markers are still recorded; the
  PR body states plainly that the hook could not fire rather than implying it
  did.

## Assumptions

- **S1** — The `trade.*` shaping family is included even though no profile
  reaches it today. It declares the same policy through the same code path,
  and excluding it would leave a second contract lie behind. Safe to assume:
  including it changes no reachable behaviour.
- **S2** — Idempotency records are scoped per principal, mirroring
  `fake.rs`'s `IdempotencyScope`, so one connection cannot read another's
  recorded result. Safe to assume: it is the security-preserving default and
  the reference host already models it. *(Wanted to ask, had the budget been
  larger; the reading taken is the strict one.)*
- **S3** — The bounds reuse the existing `CONTROL_IDEMPOTENCY_*` constants in
  `crates/control/src/limits.rs` rather than inventing new numbers. Safe to
  assume: `CLAUDE.md` forbids hardcoded values and those constants exist for
  exactly this store.
- **S4** — A retryable failure is never recorded. Recording a `BACKPRESSURE`
  or `TIMEOUT` outcome under a key would pin the client to that failure for
  the whole retention window, which inverts the guarantee the key exists for.
  Safe to assume: `fake.rs` marks its own retention refusal non-retryable for
  the same reason.
- **S5** — Only non-parked dispatches can carry a key. Every capability
  declaring `Optional` dispatches terminally; the one parking capability,
  `events.wait`, is read-only and declares `Forbidden`. Safe to assume: it
  follows from the registry, and a parked path that ever declared `Optional`
  would fail the descriptor tests added here.
- **S6** — The deduplication guarantee is per connection rather than per
  client, because `principal_id` is minted per handshake from `random_bytes`
  and nothing else that survives a reconnect is authenticated: the bearer
  token is shared by every client of one grant, and `client_name` is
  client-supplied. Scoping by either would let one client replay another's
  result. Assumed rather than asked because the alternative is a data leak,
  which is not a preference; the narrower guarantee is stated in the module
  documentation and the PR body rather than implied.

## Acceptance criteria

- [ ] **A1** — A request carrying an idempotency key against a capability that
      declares `Optional` is accepted by the running gateway instead of
      refused.
      *Evidence:* a named test driving the real gateway contract, not the
      reference host.
      → `crates/app/src/app/tests/control_plane_tests.rs` *(R1, R4)*
- [ ] **A2** — Replaying the same key with the same input returns the first
      recorded result and performs the effect exactly once.
      *Evidence:* a named test asserting both responses are equal and the
      application state advanced once.
      → `crates/app/src/app/tests/control_plane_tests.rs` *(R1, R4)*
- [ ] **A3** — Replaying the same key with a different input fails with
      `control.idempotency_conflict`.
      *Evidence:* a named test asserting the error code.
      → `crates/app/src/app/tests/control_plane_tests.rs` *(R1, R4)*
- [ ] **A4** — The record store is bounded and scoped: per principal, capped
      at `CONTROL_IDEMPOTENCY_MAX_ENTRIES` with oldest-first eviction, expired
      at `CONTROL_IDEMPOTENCY_RETENTION_MS`, and a record oversized for
      retention is refused rather than stored.
      *Evidence:* named unit tests for eviction, expiry, oversize and
      cross-principal isolation.
      → `crates/app/src/control/gateway/idempotency.rs` *(R1, R4)*
- [ ] **A5** — A capability declaring `Forbidden` still refuses a key, and the
      `dry_run` and `expected_revisions` refusals are unchanged.
      *Evidence:* named regression tests.
      → `crates/app/src/app/tests/control_plane_tests.rs` *(R1)*
- [ ] **A7** — A retry that races its own first call — sent because the first
      has not answered, which is the case the descriptors name — is refused
      retryably rather than acted on, and the pair leaves one effect.
      *Evidence:* a named end-to-end test that pipelines the retry, plus the
      injected-breakage record showing it fails without the reservation.
      → `crates/app/src/app/tests/control_plane_tests.rs`,
      `.claude/evidence/idempotency-contract/injected-breakage.md` *(R1, R4)*
- [ ] **A6** — The change reaches `campaign/architecture-a` through a pull
      request whose base is exactly that branch, and the merge is read back:
      merge commit, `mergedAt`, actor and base verified against the campaign
      ref.
      *Evidence:* the readback recorded as a comment on #359 and in the PR
      body.
      → milocaetano/quantick#359 *(R2, R3)*

## Injected gates

- [ ] **G1** — Every artifact English, `CLAUDE.md`'s rule and its exemptions.
      *Evidence:* `cargo test -p quantick-guards` green; `arch-review`
      dimension 8. → PR body
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace`
      green on the final head after rebasing on the current campaign base.
      *Evidence:* command output. → `.claude/evidence/idempotency-contract/`
- [ ] **G3** — Performance impact declared: every touched path classified by
      rate.
      *Evidence:* the classification, stated. → PR body
- [ ] **G4** — `arch-review` run over the exact diff against the campaign
      base, every Blocker and Should-fix resolved or deferred.
      *Evidence:* the review verdict. → PR body

## Not applicable

- **Hot path.** The gateway request path is per-control-request, which is a
  rare path — a human or an agent issuing a call, not per trade, per depth
  update or per frame. No `APP_HEALTH_SUMMARY` control run is owed. Stated
  under G3 rather than waived silently.
- **User-visible surface.** No UI surface is added or changed, so
  `ui-harness`, `visual-qa` and `trader-ux-review` do not apply. The access
  panel, the scopes it offers and the profiles that reach each capability are
  untouched.
- **Adds a capability.** `new-extension` does not apply: no feed, bar type,
  indicator, layer, panel or crate is added. The change makes existing
  registered capabilities honour a policy they already publish.
- **Adds something a trader does.** No new action, tool, trade or lock. The
  `trade.*` family stays unreachable by every grantable profile.
- **Engine / determinism territory.** The engine is not touched. The store
  itself is a `BTreeMap` keyed by an ordered scope, so its iteration and
  eviction order are deterministic, but no golden fixture is owed.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The pull request is open against `campaign/architecture-a`.

## Review outcomes

`arch-review` at `7542f7d3afe18bcb0fb59f509dcbf65f963304af`, step 0 at
`medium`: three confirmed correctness findings, all repaired on the branch —
a keyed call that could still act twice after a `control.timeout`, records
outliving the connection that could use them, and a conflict reported as
retryable while the first call was in flight. The shape pass raised two more
against the same diff, also repaired: the trunk crossing the size ratchet's
threshold, and a second convention for a poisoned lock. Nothing deferred.

`delivery-review`, full mode, independent reviewer on a fresh context:
eleven of twelve lines `DELIVERED`, with `A6` `PARTIAL` because the merge it
names had not happened when the review ran. Nothing `UNLEDGERED`, no `R`
`DROPPED`, no exclusion wrongly claimed.

### The assumption that should have been a question

The reviewer's audit of `S2` and `S6` is upheld, and it is a finding against
this mission's own interrogation rather than against the branch. The
guarantee's scope — per connection rather than per client, so a client that
reconnects starts its keys over — shapes `IdempotencyScope` and narrows a
promise the descriptors made in prose. This mission set its tier to `high`
citing exactly that risk ("a wrong scope is a data leak ... the contract
change needs the full interrogation"), and then decided the scope by
assumption anyway. `S2` even records it as *wanted to ask*.

The question step 3 should have put to the trader: **should the retry
guarantee survive a reconnect, and if it cannot without an authenticated
durable client identity, is silently narrowing the descriptor's prose
acceptable?** The reading taken is the safe one — the alternative lets one
client replay another's recorded result — and it is stated in the module
documentation, the two descriptor comments and the pull request body rather
than left implicit. It stands as taken, and it is the trader's to revisit.

## The request as received

> resolver o item 3 e fazer pr pra branch. Faça mesmo memso o merge tem
> autorização

*(Attributed quotation of the trader's request, retained verbatim under
`CLAUDE.md`'s language exemption for marked quotations.)*
