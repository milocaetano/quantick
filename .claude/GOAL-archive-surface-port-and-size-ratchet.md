# Mission

Stop the `app` crate's trunk from absorbing every new capability: add a
mechanical size ratchet and an `arch-review` accumulation dimension, then
carve the `Surface` port and move exemplary surfaces onto it.

## Why (the diagnosis this mission answers)

`arch-review` dimension 1 asks whether a new capability can dock as a new file
plus one registration line. Every recent feature answered yes honestly — and
`app.rs` still grew from 108 lines to 34,030 in 37 days, monotonically. The
review measures the leaf and never the trunk: nobody asks *where the
registration lines accumulate*. They accumulate in `QuantickApp` (130 fields)
and `new_with_workspace` (1,022 lines), because there is no port for "being a
surface of the app" — so each of the 68 modules is wired by hand in four
places: a field, an init, a draw call, a hotkey.

Measured coupling says the underlying design is sound, not rotten: 9 of 21
`draw_*` surfaces touch 1-2 fields of `QuantickApp`; only `draw_frame` (14)
and `draw_menu_bar` (17) are genuinely tangled. This is wiring debt, not
architecture debt, which is why it is mechanical to repay.

`grep` for "line count | file size | god object | monolith" in
`arch-review/SKILL.md` returns zero hits. The repo already knows how to turn a
rule into a test (`language_guard.rs`, `source_encoding_guard.rs`); size has no
such guard. A rule enforced only by a skill's judgement drifts.

## Classification

Code change + hot path (per-frame draw dispatch) + user-visible + adds a
capability (the port). All four gate sets apply.

## Acceptance criteria

1. `crates/app/tests/size_guard.rs` fails when any tracked file grows past a
   versioned baseline, and passes at today's baseline.
2. `arch-review` gains an accumulation dimension: it judges where registration
   lines pile up, not only whether a new file was added.
3. `new-extension` states that a UI surface docks through the `Surface` port,
   never through a new `QuantickApp` field.
4. A `Surface` port and registry exist in the `app` crate, with a fake second
   implementation covered by a test (`new-extension`'s rule).
5. At least 3 surfaces move onto the port; `QuantickApp` loses at least 3
   fields (before/after counts recorded); `draw_frame` no longer hand-calls
   them.
6. Behaviour identical: `visual-qa` PASS on every moved surface, or defects
   explicitly accepted.
7. Per-frame performance flat or better: `APP_HEALTH_SUMMARY` fps/frame_avg
   against a `main` control run, numbers in the PR body.
8. Four checks green, run as separate commands; `arch-review` run with every
   Blocker/Should-fix resolved or deferred in the PR body; PR opened.
9. Every artifact in English.

## Order of work (risk-ascending, so the gate survives a failed port)

Items 1-3 are additive and carry zero production risk; they stop the bleeding
on their own. The port (4-5) comes second and starts with the surfaces that
touch a single field. If the port fails, it is dropped and the gate still
ships.

## Done means

The PR is open with green CI and the evidence in its body. Merging is the
trader's call, never part of the mission.
