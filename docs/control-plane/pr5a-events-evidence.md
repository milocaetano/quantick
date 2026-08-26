# PR 5a events, cursor and the pointing channel — evidence

**Branch:** `feat/control-events`

**Rate class:** Semantic changes only — never trade or frame frequency. With
local access disabled the frame loop does not enter the journal; with it
enabled, each frame compares a handful of small owned values and records an
event only when one changed.

## Result

The running instance keeps a bounded semantic event journal and publishes it
through a cursor: `events.read` pages it on the application thread like every
capture, and `events.wait` parks on the gateway side — holding a parked-waiter
slot, never a UI request slot — until the journal moves or its timeout
elapses. The human has a mark hotkey (Ctrl+M) that appends an event carrying
the fully resolved cursor target plus an optional note, through the first
registered action, `attention.mark.create`, which the action registry port
exposes to the hotkey, the `QUANTICK_CONTROL_MARK` hook and tests — and keeps
unavailable to remote observer clients. A mark taken during a replay is
appended to the recording's durable control trace and re-injected at the same
logical replay time on the next run of that recording.

## Pieces

| Piece | Where | What it is |
| --- | --- | --- |
| `EventJournal` | `crates/app/src/control/journal.rs` | Ring bounded by `CONTROL_EVENT_JOURNAL_CAPACITY` entries and `CONTROL_EVENT_JOURNAL_MAX_BYTES`; events above `CONTROL_EVENT_MAX_BYTES` keep a bounded summary; `JournalSignal` publishes `next_sequence`/`oldest_sequence` atomically and ticks a bounded(1) channel — the application thread stores two atomics and tries one send, and acquires no lock |
| `events.read`, `events.wait` | `crates/app/src/control/events.rs`, `contract.rs` | Capabilities under `observe` + `observe.events` (now in the safe default grant); a first read names `oldest` or `latest`, later reads carry the cursor; `dropped_before` reports retention loss |
| Parked waits | `gateway.rs` (`waiter_manager`, `dispatch_parked_wait`) | One manager thread per gateway run owns the parked waiters, listens to the journal tick and deadlines, wakes each waiter; the woken waiter runs the bounded read through the ordinary UI path; slots bounded by `CONTROL_MAX_PARKED_WAITERS` globally and `CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION` per connection, overflow is `control.backpressure`; a parked wait holds its request ID (a duplicate is refused), a closed connection releases its waits at the manager's next pass, a retention gap seen at park time is reported on the page, and `timed_out` is false once events landed |
| Frame emitter | `gateway.rs` (`emit_semantic_changes`) | While enabled: `workspace.tab.activated/opened/closed`, `workspace.focus.changed`, `interaction.selection.changed`, `feed.market.changed`, `feed.connection.changed`, `replay.state.changed`; the first enabled frame sets the baseline and records nothing |
| Action registry | `crates/app/src/control/actions.rs` | `ActionRegistry` (descriptor + handler + compiled schemas); `attention.mark.create` with effect `annotate`, permissions `annotate` + `annotate.attention`, module `attention`; the descriptor is registered in the same `ControlRegistry` the reads use, so `describe` and search list it; no read handler, so a remote call that had the permission would still fail closed before dispatch |
| Local invocation | `gateway.rs` (`invoke_local_action`), `app.rs` (`control_action`, `take_mark`) | One path for the hotkey, the hook, tests and a replayed trace entry: validate input, build the trusted actor (`human_ui` from this window, `automation` for a replayed entry), append the trace intent, run, validate output, append the trace result |
| Control trace | `crates/app/src/control/trace.rs` | `ControlTrace` port; `ReplayTraceFile` = `<session>.control-trace.jsonl` beside the recording, intent line then result line; `NoTrace` for a live tab; `TraceReplay::load` pairs intents with results and names unfinished intents (a run with one is not a fixture); `service_replay_trace` re-injects due entries each frame, connected or not — one walk per recording (two tabs on the same file share it), loaded once, rewound by the worker's restart/seek count and target, and extended by the actions this run records so an in-session restart replays what a fresh process would; entries replay at their recorded capability version |
| MCP tools | `crates/mcp` | `quantick_read_events`, `quantick_wait_for_change`; the adapter extends its read patience by the wait's own timeout |
| Schemas | `schemas/control/observer-events-read-input-v1`, `observer-events-wait-input-v1`, `observer-event-page-v1`, `attention-mark-input-v1`, `attention-mark-result-v1`; catalog regenerated | Committed, snapshot-tested |

## The mark's input determines the mark

`MarkInput { note?, target? }`. The hotkey resolves the pointer at the moment
of the gesture and supplies the target; the handler resolves the pointer only
when none was supplied. The input alone therefore determines the event, which
is what lets the control trace replay a mark identically without a human
present, and what lets a later agent mark a target it read from a snapshot
rather than the pointer it does not hold. `target_source` names who resolved
it: `pointer` (the human's pointer — hotkey, hook, or a caller that passed
none), `supplied` (an agent passed a target it read), `replayed` (a control
trace re-injected it). The result carries no wall-clock time — the journal
event does — so the trace's result digest depends on what was marked and on
its place in the journal, never on the clock. A replayed entry without its
recorded target is refused rather than resolved against the rerun's pointer.

## Acceptance against the plan (PR 5a)

| Criterion | Evidence |
| --- | --- |
| An agent watching `wait_for_change` observes the user take a mark and can name the bar, price and object that was marked | `gateway_wait_for_change_sees_a_human_mark_and_does_not_delay_a_concurrent_read` — the page names bar slot 20, the symbol, the note and the human actor |
| A mark taken over a footprint cell reports the cell, not only the bar | The mark carries the cursor projection's `FlowCellSnapshot` when a flow layer is on — the same projection `observer_cursor_resolves_the_exact_bar_under_the_pointer` and `semantic_pointer_resolves_the_same_heat_cell_the_renderer_projects` pin; no separate cell path exists to diverge |
| The journal never allocates per trade; market events are aggregated first | The frame emitter compares keys and records only on change; no trade-rate path touches the journal (`grep` for `journal` outside `crates/app/src/control/` finds only the frame hook) |
| An expired cursor reports `dropped_before` | `an_expired_cursor_reports_dropped_before_and_a_foreign_cursor_is_invalid` (events), `capacity_evicts_the_oldest_and_a_read_below_the_window_starts_at_the_oldest` (journal) |
| A parked waiter does not delay any other request, proved by a test that waits and reads concurrently | `gateway_wait_for_change_sees_a_human_mark_and_does_not_delay_a_concurrent_read`: with a wait parked (UI queue empty), a worker-side describe and a UI-side snapshot on another connection complete at once |
| Remote write requests remain impossible under the observer profile | `gateway_observer_cannot_create_a_mark_remotely_but_reads_the_registered_action` (`control.permission_denied`) |
| Replaying a recorded human mark injects it at the same logical replay time; an incomplete or missing trace makes the run fixture-ineligible | `a_mark_during_replay_is_traced_and_replayed_at_the_same_logical_time`; `intents_and_results_pair_up_and_an_unfinished_intent_is_reported` |

## Deferred

- Typing a note at the hotkey (an inline prompt): the hotkey marks at once;
  the note travels through the hook and the action input. The prompt is a UI
  affordance on the same action and can land alone.
- Per-event module revisions: events do not yet carry the module's revision,
  because revisions are still derived from captures (see #213's deferred
  finding); the journal sequence is the ordering token until PR 5a's successor
  makes revisions change counters.
- Indicator and drawing change events, order/position changes, health alerts:
  the emitter is one comparison per key; the remaining keys dock one line
  each when their modules register scopes.
