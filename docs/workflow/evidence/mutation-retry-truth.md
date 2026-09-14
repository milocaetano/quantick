# Mutation retry truth: validation evidence

Child R1, issue #475, campaign #472. Base: `origin/campaign/outside-eight`,
initial tip `eb7bb039434667bb150be9cdf5237e172c4198fe`.

## Preflight and scope

The coordinator performed the high-tier independent source-first completeness
pass before production edits. It derived outcomes from the retained original
and delegated sources, then reconciled map revision 1: PASS. The reviewed GOAL
blob was `c02ac24d9a68b6b19854cbefa32bc376665163d0`. The map retains the campaign's
scoring/feed outcomes under parent/sibling ownership and makes no score claim.

Pre-edit guard build and MCP/control-host all-target checks passed. The first
app check encountered an intermediate missing `Debug` implementation in the new
client bookkeeping; it was corrected and app all-target check passed before app
production edits. Later an interrupted build lost generated objects and the
guard executable under this worktree's target directory. Another intermediate
test compilation saw a stale client dependency while its source was being
edited. Neither attempt is passing evidence. The subsequent frozen targeted
run rebuilt dependencies and passed; the guard executable was rebuilt too.

The first guard run after implementation reported 47,682 UI-free app lines
against the unchanged 47,665 ceiling. The repair moved pure response-envelope
construction and atomic response-slot reservation, together with their existing
test, from the app gateway into control-host dispatch. The subsequent guard run
passed. No guard, ceiling, test assertion or measurement rule was weakened.

Before freezing, the coordinator identified that another principal can evict a
terminal key record before its TTL. The real-store regression
`another_principal_can_evict_a_terminal_record_so_in_flight_advice_is_not_retryable`
reproduces that case. Started mutation timeouts and in-flight duplicates therefore
return non-retryable uncertainty even with a key. Late-result settling and
retained-key replay remain supported. The initial keyed-timeout assertion was
strengthened from retryable to non-retryable; reservation and readback assertions
remain. The app generator build was deliberately paused for this correction.

## Behavior and executable proof

| Criterion | Implemented behavior | Executable evidence |
| --- | --- | --- |
| A1 | Atomic cancellation versus dispatch; unkeyed post-start deadline and any post-dispatch mutable transport loss return unknown/non-retryable with readback instructions | `an_unkeyed_post_start_timeout_is_unknown_and_not_retryable`, `post_execution_socket_loss_is_unknown_for_keyed_and_unkeyed_mutations`, `a_real_lost_reply_after_execution_never_invites_a_mutating_retry_even_with_a_key` |
| A2 | `quantick_invoke` validates and carries the optional key unchanged; existing runtime policy still rejects forbidden/conflicting keys | `invoke_rejects_invalid_keys_before_discovery_or_dispatch`, `generic_invoke_carries_keys_and_the_wire_host_deduplicates_and_refuses_conflicts`, existing application optional/forbidden family tests |
| A3 | Cancellation prevents later action, same-connection keyed retries remain deduplicated, oversized results retain a bounded refusal, reads use runtime metadata | `a_gateway_deadline_cancels_queued_mutation_before_any_later_dispatch`, `simultaneous_cancel_and_dispatch_have_exactly_one_winner`, `a_result_too_large_to_retain_keeps_a_non_retryable_tombstone`, `failed_or_malformed_refresh_cannot_preserve_old_read_only_claims`, `a_lost_read_reply_uses_the_runtime_read_only_descriptor_to_allow_retry` |
| A4 | Permission/identity boundaries, headless dependency direction, ticket/replay behavior and ratchets remain required | Guards and complete workspace suite; independent reviews remain separate |
| A5 | Frozen measurement paths remain unchanged; no campaign score is assigned here | Name/status diff and parent handoff |

The application tests use the real authenticated gateway and an action that
has executed while its response is withheld. The socket-loss test then revokes
that connection to close the actual socket before the response is sent, and
reconciles over a new connection. The MCP tests use the real adapter, discovery,
authentication and local transport against a second host implementation; that
host closes its socket after incrementing an observable side-effect counter.
Existing application tests prove permission and idempotency policy enforcement
against the production registry for all mutable families.

## Local validation

Toolchain: `rustc 1.98.0 (88d9e12ae 2026-08-18)`,
`cargo 1.98.0 (797e8a9bc 2026-08-05)`, `x86_64-pc-windows-msvc`.
Workspace dependency lock and runtime/build configuration are unchanged.

[Targeted command output](../../../.claude/evidence/mutation-retry-truth/targeted.log)
records 119 passing control-host/control-local/MCP tests at source tree
`141f0b7c621b514d71cbb9f8ed12fcbba4b2fa09`. After the pure-helper move, source
tree `c3912f07a17d2e8b85d8ad5ea7cc837418bbe392` passed 50 control-host tests and
all 26 repository integration guards. These are targeted runs, not a substitute
for the ordered loop or final-head CI.

The first ordered loop at runtime tree
`000af3d138925af15efdd3070840ccf6d5629fb4` passed fmt, clippy (1m 43s) and
build (6.73s), then failed two untouched orderflow-view tests: 2,067 app tests
passed, two failed and 11 were already ignored. The inherited
`QUANTICK_BUBBLES` selected the user's `mini index sweeps` preset with
`min_quantity = 50`, filtering the fixtures' quantity-one trades. The exact
same binary failed those two in isolation, then passed all 23 orderflow-view
tests with a process-local override to this worktree's tracked
`crates/app/config/bubbles.toml`. The user's file and all test assertions are
unchanged. This is a proven test-environment correction, not a runtime repair.
Only that Quantick environment variable was inherited.

[Failed loop and isolated reproduction](../../../.claude/evidence/mutation-retry-truth/workspace-first-failure.log)
preserves the actual failure tail (the tool truncated the repetitive passing
middle); [same-binary environment diagnostic](../../../.claude/evidence/mutation-retry-truth/workspace-environment-diagnostic.log)
records the controlled recovery. A briefly attempted crate-only diagnostic
started recompiling a different feature graph and was stopped before switching
to the existing workspace binary. No interrupted attempt is passing evidence.

[Focused application retry/readback output](../../../.claude/evidence/mutation-retry-truth/retry-readback.log)
records all 17 tests passing in 2.30s using the same workspace-built binary and
tracked preset, including real post-execution socket loss, post-start timeout,
queued cancellation and the complete reachable mutable-family matrix.

Final ordered workspace validation passed on 2026-09-14, completed before
19:35 UTC, at runtime tree `000af3d138925af15efdd3070840ccf6d5629fb4`, committed
unchanged as `f23ecd52757697df5a572d08cbb9cf988660ab4b`:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Exit 0, no output |
| `cargo clippy --workspace --all-targets` | Exit 0, 1.20s |
| `cargo build --workspace` | Exit 0, 1.42s |
| `cargo test --workspace` | Exit 0; 3,897 passed, 19 pre-existing ignored across unit/integration/doc suites |

The app suite passed 2,069 tests with 11 pre-existing ignored tests. The full
output was captured without truncation; [selected verbatim output](../../../.claude/evidence/mutation-retry-truth/workspace-final.log)
retains every suite/result summary and relevant regression names. The four
commands ran in order with an immediate stop on failure. The only environment
correction was the process-local tracked bubble preset described above.

The final archival commit changes evidence and the mission record only.
Its runtime verification is reused from the code commit/tree above, not claimed
as another execution. The base remains `eb7bb039434667bb150be9cdf5237e172c4198fe`;
toolchain, lock, config, contracts, generated inputs, fixtures and code remain
identical. The seven added paths are the mission archive, this report and the
five logs in `.claude/evidence/mutation-retry-truth/`; the complete archival
commit diff is evidence-only and was inspected. All 26 guard integration tests
passed after staging the archive (1.43s), and all five relative report links
resolve. Initial trailing blank lines in four logs were removed for diff hygiene.
The PR body records the exact archival head/tree for evidence reuse.

## Cost, contracts and limits

All changed paths run per control invocation, rarely relative to market data.
No market per-trade, per-depth or unconditional per-frame work is added.
The client tracks at most 1,024 outstanding request IDs; descriptor metadata is
bounded by the framed describe response and replaced on refresh. An initial
unclassified MCP call reads the runtime description to learn safe retry advice.
Cancellation adds one atomic transition at dispatch and deadline. Existing
idempotency entry/byte/retention limits remain in force.

The optional MCP key is an additive argument. Structured error fields already
exist in the public contract; false retry advice is corrected in those fields.
No permission, profile, financial action or handshake identity was added.
Deduplication still ends with connection identity, retention or capacity eviction;
after transport loss the caller must reconcile instead of repeating a key on
a new connection. This does not implement the durable identity proposal #362.

Independent architecture, AI and delivery reports, exact-head CI and final
mission/ship reconciliation remain pending until published on the PR.
