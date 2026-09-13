# Trader UX review — quick fixed-range volume profile

## Flow: select, decide, dismiss or convert

Rafa holds secondary drag across the range while reading the tape, releases,
and hits the one FRVP action: one gesture plus one explicit click, no dialog,
keyboard-focus theft, or live-lane occlusion. Marina gets the same ruler
grammar in the active pane; a tab change drops the temporary choice, while a
toolbar ruler and converted profile remain ordinary persistent drawings. Duda
sees a single familiar drawing-style button whose hover name is “Fixed range
volume profile”; an invalid range disables it with a direct explanation, and a
mis-click on the chart safely removes only transient state.

Evidence: `active-final-a.png`, `ready-final-a.png`, `converted-final-a.png`,
and `dismissed-final-a.png`; input/state ownership at
`crates/app/src/pane/quick_range.rs:19` and
`crates/app/src/surfaces/drawing_chrome/quick_range.rs:113`; conversion through
the registered action at `crates/app/src/app/drawing_input.rs:104`.

## Findings

- Blocker: none.
- Should-fix: none.
- Consider: a future discoverability pass could add the action name beside the
  icon at very wide widths, but the existing icon/tooltip grammar is consistent
  with drawing chrome and adding text would raise glance and occlusion cost.

Rafa can trade through it: the tape keeps moving at 60 FPS and the gesture adds
no focus-taking surface. Marina can keep her workspace: only conversion creates
a durable drawing, and the ordinary ruler regression test passes. Duda can
figure it out alone: release reveals one named, reversible action and clicking
the chart cancels safely.
