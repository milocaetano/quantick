# Independent alert API consumer evidence

An independent agent that did not implement the alert UI or observer exercised
the published alert schema and capability catalog against the native app on
2026-11-10. This is an API consumer check, not a mouse usability study or trader
interview. Native input automation was unavailable.

## Historical executable and isolation

- SHA-256: `4AD510ED32DDDD5A62182C8AD42159D2E621C00B7E815099DCD28534F61FD4C9`.
- Executable modification time: `2026-11-10T08:41:56.5812726Z`.
- Source checkout HEAD: `e2d22d296bf281801d12d7a62111ff13fc33ebb1`, with
  uncommitted implementation changes. HEAD alone does **not** identify the
  executable's full source. No build-time source manifest was available;
  the binary hash identifies this tested artifact exactly.
- Consumer instance PID 34336; separate read-only instance PID 27652. Both
  were closed after the checks. Each used fresh isolated stores and the same
  synthetic `ALERTTEST` replay; no user stores or live trades were used.
- Raw request-result evidence and launch manifests are outside Git under
  `C:/src/quantick-worktrees/alerts-perf-evidence/consumer-current/` and
  `C:/src/quantick-worktrees/alerts-perf-evidence/consumer-denied/`.

## Historical observed results

| Check | Native transport result |
| --- | --- |
| Validate schema-built price rule | `valid: true` |
| Create and repeat identical idempotency key | Both return id 1, revision 1; list contains exactly one rule |
| List/read/history | `monitoring`, null reason, empty events |
| Edit with revision 1 | Revision 2; edited name reads back |
| Pause with stale revision 1 | `control.revision_conflict`, actual revision 2 |
| Pause with revision 2 | Revision 3; read reports `paused` |
| Resume with revision 3 | Revision 4; read reports `monitoring` |
| Invalid operator | `control.invalid_request`, schema enum failure at `/spec/conditions/0/operator` |
| Delete with revision 4 | Revision 5; read reports `control.resource_gone` |
| Create on read-only instance | `control.permission_denied` |

The no-fire rule uses `GreaterThan`, value `1000000`, `PerBarClose`, no
expiration and no notification channels. The source is the exact replay CSV
path and aggregation is `tick:10`. Successful native calls, explicit errors and
the identical create retry are recorded; no transport timeout was encountered
or invented for this pass.

## Historical durable drawing consumer

The consumer created an agent-owned trend line with the published annotation
API, read its tab/pane/drawing selector from `analysis.drawings`, resolved it
through `alerts.target.resolve`, and created a rule with the returned durable
workspace/origin/pane/drawing/incarnation identity. Readback retained that
identity. It honestly reported `suspended` / `OutsideTimeDomain`: the finite
line's endpoints were older bars than the advancing replay. This proves
selector-to-durable-target consumption, not active geometry monitoring or
reopen behavior.

One initial annotation call included an idempotency key although that
capability forbids keys; the native boundary refused it. The corrected call
omitted the key and succeeded. Alert mutations, which require keys, used them.

## Historical limits

This pass does not establish interactive keyboard/mouse usability, denied
scope recovery UI, network-loss retry behavior, hydration transitions, or
durable drawing reopening. Compilation ran concurrently, so its health logs
are not the matched baseline performance evidence. Final-binary reruns and
simulated persona review are recorded separately below and in the trader report.

## Final committed-binary refresh

On 2026-11-10 the independent consumer exercised immutable
`quantick-alerts-485a77bb.exe`, SHA256
`457E82EE5087E63C6DFEB0A12852F0C53F4C29ABA5316526BA456718B4960122`,
mapped to commit `485a77bb802255c0dc420137e39997ac7fda1051` and tree
`43a4ed82a0025b6b17b6c4d6fb8f36a3b6f2e0e5` by
`.artifacts/validation/harness-485a77bb.json`. Runtime describes the application
commit as `unknown`; the external immutable build manifest supplies provenance.

Fresh isolated runs `consumer-485a-denied` (PID 31060) and
`consumer-485a-granted` (PID 36104) live under
`C:/src/quantick-worktrees/alerts-perf-evidence/`. Both used the synthetic
240,000-print ALERTTEST tape, no user stores, no orders and no requested
notification channels. Both windows were closed and process absence verified.
Launch manifests, full MCP request/results and the granted socket transcript
remain untracked in those directories. No source edits or builds were performed
by this consumer.

The consumer called describe before operating, read the published spec and its
`x-operator-targets`/cadence vocabulary, and built a Value/GreaterThan rule with
canonical string price `1000000`, `PerBarClose`, Never expiration and all local
channels disabled. Snapshot readback established ALERTTEST and `tick:10` on
the running replay; the source recording was the isolated fixture's exact path.

| Final case | Observed result |
| --- | --- |
| Schema-built validate and invalid `OR` operator | Valid true; invalid request points to operator enum |
| Create and identical-key retry through MCP | Same id 1/revision 1; list contains one rule and journal one create receipt |
| Edit, stale pause, pause, resume | Revisions 2, conflict with actual 2, revision 3 paused, revision 4 monitoring |
| Delete and clear without removal scope | Both `control.permission_denied`; definition remains monitoring |
| Explicit removal grant over local socket | Handshake includes `cockpit.alert.delete`; describe marks removal irreversible with explicit permanent-loss descriptions |
| Stale delete with removal grant | `control.revision_conflict`, actual definition revision 4 |
| Create once-only firing fixture and retry | Same id 2/revision 1; one native evaluator event at exact price `100.8341` |
| Delete id 2 and identical-key retry | Both id 2/revision 2; read returns `control.resource_gone` |
| History after definition deletion | Entire event result equals the pre-delete result, including message/spec/revision/price |
| Clear history and identical-key retry | Both null id/revision 0; history empty; id 1 remains monitoring at revision 4 |
| Mutation receipts | Exactly one `alerts.changed` receipt for each create, delete and clear key, naming the original request ID |

The firing event reports `Delivered` with every channel disabled. This means
the native delivery path completed, not that the consumer heard audio or saw a
notification. Equality/count assertions over saved responses passed. All
retries stayed on their original connection; no timeout or reconnection was
fabricated. Definition `payload.expected_revision` is distinct from envelope
module revisions; clear history explicitly has no definition revision guard.

## Adapter reachability and remaining limits

The connected MCP adapter asks only for `cockpit` and `cockpit.layout` above
its annotation scopes. Even the second app's explicit removal grant therefore
did not give that MCP connection `cockpit.alert.delete`: a delete remained
permission-denied. This is an observed adapter reachability limitation, not
proof that granting the app scope alone enables MCP deletion.

For granted evidence the consumer used the published length-prefixed JSON
local protocol, authenticated to the same disposable instance, explicitly
requested the removal scope, and called `control.describe` before mutations.
This is the actual admitted native transport, not a Rust-internal test or UI
automation. Its script logs no bearer token. The concrete current path is a
local-protocol client requesting `cockpit.alert.delete` plus the application's
grant. An optional explicit MCP opt-in remains follow-up work; silently adding
the sensitive scope to every MCP connection would not be an appropriate fix.

This final pass satisfies the independent supported-schema/admitted-transport
consumer check for C8; it adds no further unmet C8 criterion. It does not rerun
the historical durable drawing binding/reopen cases or establish every UI
criterion. C11 native keyboard/mouse, focus, scrolling, confirmation and
notification interactions remain unobserved. Simulated trader feedback is in
`alerts-trader-review.md`; it is not a market interview.
