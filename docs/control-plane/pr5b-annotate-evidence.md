# PR 5b evidence: the annotate and notify tier

**Branch:** `feat/control-annotate`, cut from `origin/feat/control-events` (PR 5a).

**Plan:** [PR 5b](../mcp-control-plane-development-plan.md) · **Roadmap:** §5.3.

This is the first tier that writes, and deliberately the one that cannot lose
the trader's work. It is also what makes the loop bidirectional: until it
ships, an assistant can read the chart and has no way to answer on it.

## Rate class and tier

A human or an agent acting — never per trade, never per frame. Nothing in this
change runs on the aggregator, the depth path or the render path:

| Path | Rate | What this change does there |
| --- | --- | --- |
| Aggregator, tick ingest | per trade | nothing |
| Book state, depth projection | per depth update | nothing |
| Renderer, per-frame view | ~60 Hz | two hook fields checked per frame while a hook is pending (`Option::take`); the author chip formats one string only on the frames a *selected* object has an author; the context bar's slot list gains one entry for such an object |
| Actions, notifications, script attach | one per gesture or per remote call | the work of this change |

The gateway's drain now owns `&mut ControlAccess` for the pass rather than
borrowing four of its fields, which is what lets an action run through the same
`invoke_local_action` the hotkey uses. Reads take the same path they did.

## What the trader grants, and what it opens

The panel gained a second section, in the words a trader would use: everything
above it lets an assistant *read* the window, everything in it lets an
assistant *put something in it*. The profile follows the scopes — grant any
`annotate.*` scope and the ceiling becomes `annotator`; grant none and every
action of this tier is refused before dispatch, however the client asks.

| Scope | Opens | Default |
| --- | --- | --- |
| `annotate` | the tier's floor; alone it opens nothing | prompt |
| `annotate.attention` | `attention.mark.create` (PR 5a) | prompt |
| `annotate.chart` | label, arrow, zone, and removing what an operator placed | prompt |
| `annotate.notification` | popup and toast | prompt |
| `annotate.sound` | the platform's alert sound | prompt, **sensitive** |
| `annotate.script` | compile and attach a Quantick Pine indicator | prompt, **sensitive** |

## Acceptance

| Plan / roadmap criterion | Evidence |
| --- | --- |
| 1. The same handler serves the interface and the agent | `the_same_handler_places_the_traders_object_and_the_agents` — the trader's note hook and a remote `annotate.label.create` place the same tool on the same pane through `Drawings::place_with`; only the attribution differs |
| 2. An agent's annotation is visibly attributed and removed by the trader in one action | `the_trader_takes_back_every_object_an_assistant_placed_in_one_action`; the author is shown in the inspector (`Placed by …`), the object manager row (an `assistant` chip) and the context bar (a robot chip with the name in its tooltip); the sweep is one button, one undo entry, with a toast |
| 3. A failed compile returns `PineError` code, span and notes as structured data | `a_script_that_does_not_compile_answers_with_spans_and_codes` — `error.context.details.diagnostics[]` carries code, start, end, line, column, message, notes; committed as `indicator-script-diagnostic-v1.schema.json` |
| 4. A successful attach is readable and detach restores the prior state exactly | `attaching_a_script_and_detaching_it_leaves_the_pane_as_it_was` — the pane's indicator kinds are identical before the attach and after the detach. Reading it back through an `indicators` *snapshot scope* waits for roadmap 5.1, which is where that scope lands; stated as a gap below |
| 5. No capability in the tier discards user-created state or affects a position | `an_operator_cannot_remove_an_object_the_trader_drew` — `annotate.remove` refuses an object with no author, whatever id it names; the tier registers no order, no layout and no cockpit capability |
| 6. Notification flood tests prove the rate and burst limits; no sound without the scope | `a_notification_flood_is_refused_before_the_trader_is_buried` (burst 2, third refused `control.backpressure` with the retry delay in its next step) and `a_client_without_the_sound_scope_cannot_make_a_sound` (asking for the scope at the handshake is not being granted it) |
| 7. Every action uses the PR 5a control trace | Every action, local or remote, goes through `invoke_local_action`, which writes the intent line before the handler and the result line after. `the_trace_records_what_was_resolved_not_what_was_asked` proves the new `resolve` step: a mark with no target records the bar it resolved, so a rerun marks that bar rather than wherever the pointer is |
| 8. Observer reaches none of it | `an_observer_reaches_no_action_of_the_annotate_tier` — all nine capabilities refused `control.permission_denied` on a default (reads-only) grant, and nothing reached the chart |
| 9. A `ui-harness` hook for every new surface | `QUANTICK_CONTROL_SCOPES`, `QUANTICK_CONTROL_ANNOTATE`, `QUANTICK_CONTROL_NOTIFY`, registered in the skill's table; `an_assistants_object_and_interruption_arrive_from_a_launch` proves they reach the tier as an agent, not as the trader |

## The resolve step (the gap PR 5a left)

An action that reads live state at call time left an intent the trace could not
reproduce: replay a "mark here" with no *here* and the rerun resolves a pointer
that was somewhere else. The action port now has a resolve step that runs
before the intent line, and a second compiled schema for the resolved input —
what the trace records and what a replay feeds back. `attention.mark.create` is
the first user: the hotkey no longer pre-resolves the pointer itself (one
resolution, in the port, for every caller), and `target_source` now says who
settled the target — `pointer`, `supplied` or `replayed`.

## Second operator

Every capability here is a named call with a registered id, declared schemas, a
readable structured result and an entry in `describe`. Authorship is recorded
on the object and shown wherever the object is named. Nothing in this tier
reaches a market or safety action, and the tier's own removal refuses anything
the trader drew.

## Blast radius

- Added: `crates/app/src/control/{annotate,notify,script}.rs`,
  `crates/app/src/audio.rs`, eleven schema documents, this document.
- Edited as registration: `control/{mod,actions,contract,schema_catalog}.rs`
  (one line or one block per module), `crates/mcp/src/tools.rs` (five tools),
  `crates/mcp/src/main.rs` (the profile), `crates/control-local/src/client.rs`
  (`ConnectOptions::for_profile`), `.claude/skills/ui-harness/SKILL.md`.
- Edited as behaviour: `control/gateway.rs` (the action dispatch, the actor,
  the profile ceiling, the notification budget), `crates/app/src/app.rs` (the
  shared attach door, the notification surfaces, the author lines, the hooks),
  `crates/app/src/drawings/{mod,context_bar}.rs` (the author field and its
  chip).

## What the review closed

`arch-review`'s step 0 (`code-review` at `xhigh`) ran over the branch and
returned fifteen findings. Two were Blockers, both now closed with a test:

- **An operator could detach the trader's own indicator.** `detach_script_indicator`
  matched any slot id in `slot_kinds`. The app now records which slots an
  *operator* attached, and the detach refuses anything else with
  `control.permission_denied` — `an_operator_cannot_detach_the_traders_own_indicator`.
- **An annotation could land inside a drawing the trader was still making.**
  `Drawings::place_with` appends to a live draft of the same tool and replaces
  one of another tool, so an agent's anchor could merge into, or discard, work
  in progress. An annotation now refuses a pane with a draft on it, retryable
  and with the reason — `an_annotation_refuses_to_land_in_a_drawing_the_trader_is_still_making`.

The rest, all fixed here: a removed annotation now sweeps strategy orphans
like every other removal path; a replayed action is attributed to the operator
the trace recorded rather than to the automation replaying it; a remote
operator's action during a replay joins this run's re-injection walk, so an
in-session restart matches a fresh process; the MCP instructions state the
authority the connection actually holds instead of claiming read-only beside
five write tools; the sweep gesture covers every pane an assistant can reach;
a launch hook without a process identity reports instead of panicking; the
`observe` floor is named rather than taken from the front of a sorted set; the
gateway removes its descriptor (and its bearer token) when the waiter manager
cannot start; a tool's bare-key shortcut no longer fires under Ctrl, Cmd or
Alt, so Ctrl+M marks without also arming the ruler; and the misplaced doc
blocks, the stale "no profile carries these permissions" comment, the
`annotate` / `annotate-tier` contradiction, the misnamed `NANOS_PER_MINUTE`
and CLAUDE.md's tool list are corrected.

## Deferred, with the reason

- **Reading an attached script back through an `indicators` snapshot scope.**
  The scope itself is roadmap 5.1; until it lands, the attach result carries
  the slot id and the declared inputs, and the detach test compares the pane's
  own indicator list. The proof through the scope belongs to the 5.1 pull
  request, as the roadmap says.
- **The script compiles twice on a successful attach** — once here, to produce
  diagnostics before anything reaches the chart, and once in the indicator
  worker, which owns building. One compile per attach gesture on a cold path;
  merging them would mean the worker reporting structured diagnostics back
  across its channel, which is a larger change to a path this tier does not own.
- **`notify.sound` on a platform with no audio backend** reports
  `unavailable_reason` rather than making a noise. Windows uses the system's
  own information sound (`MessageBeep`); no audio engine is linked, and no
  platform is told the alert was heard when it was not.
- **Visual QA and the trader UX review were not run**: this session had no
  authorization to launch the desktop application. Every new surface has a hook
  (above), so the pass is a launch away; it is recorded as BLOCKED here rather
  than skipped in silence.
