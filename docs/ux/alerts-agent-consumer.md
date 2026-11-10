# Independent alert API consumer evidence

An independent agent that did not implement the alert UI or observer exercised
the published alert schema and capability catalog against the native app on
2026-11-10. This is an API consumer check, not a mouse usability study or trader
interview. Native input automation was unavailable.

## Executable and isolation

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

## Observed results

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

## Durable drawing consumer

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

## Limits and follow-up

This pass does not establish interactive keyboard/mouse usability, denied
scope recovery UI, network-loss retry behavior, hydration transitions, or
durable drawing reopening. Compilation ran concurrently, so its health logs
are not the matched baseline performance evidence. Final-binary reruns and
simulated persona review must be recorded separately when available.
