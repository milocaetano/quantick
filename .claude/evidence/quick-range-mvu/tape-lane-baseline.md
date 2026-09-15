# Tape-lane baseline diagnostic, 2026-09-15

Bounded delegated #501 / #447 finding 4 check. Root owns remote writes.
This is test-only characterization, not a delivery or review pass.

## Input and ownership

- Worktree: `C:/src/quantick-worktrees/feat-quick-range-mvu`.
- HEAD: `9ff57501249f51d8f75c21f52094ec3dc3c39af2`.
- Before writes, `git status --short` showed only the existing modified
  `crates/app/src/app/tests/drawings_tests.rs` and untracked evidence directory.
  Existing source diff: 54 insertions, three prior characterization tests.
- Added only the 19-line candidate tape-lane test. Final source diff: 73
  insertions in that same test file; no production source delta.
- No process command line referenced `fix-mutation-retry-truth` before the
  run (PowerShell inspection process excluded); root granted exclusive target.
- Test source SHA256 after addition:
  `1FF640003177E800EB90623E4FA21D27853FC1981E2376B72E67B1791050BA55`.
- Candidate test function matches byte-for-byte after CRLF-to-LF normalization:
  `445EF99A9EAB9D3704EC41527436E4367EFB82EB78D3D3473C26A8D8C90CF0DE`.
  Shared `drag_quick_range` helper also matches:
  `8D0CA7CE3C3ECF4A19DCAF5FE6E712F0FD123774A6340B053DD6BB0F72F1C589`.
  No API adaptation was necessary.

## Command and result

PowerShell child environment only, from the baseline worktree:

```powershell
$env:CARGO_TARGET_DIR='C:/src/quantick-worktrees/fix-mutation-retry-truth/target'
Remove-Item Env:QUANTICK_BUBBLES -ErrorAction SilentlyContinue
cargo test -p quantick-app quick_range_never_starts_in_the_live_tape_lane -- --nocapture
```

Session 19701 initially reported compilation of baseline quantick-app.
Completed exit 0: compilation 4m 07s; 1 passed, 0 failed, 2070 filtered out;
test duration 0.12s. The exact candidate test also passes the old implementation
and does not establish failing-before evidence. All fixture assertions passed.
Executable SHA256: `7D8EBD2589B3601B583B10DF162ACED1AB8A9C5532AE7C927264A3AD7AE7F56B`.
`git diff --check` exited 0. No user configuration changed.

The fixture explicitly enables the lane, records trade 201, flushes its worker,
asserts a visible lane divider inside the chart, and drags from the lane center
to 80 logical points left of the divider. The product assertion requires no
quick-range control after release. A failure at lane setup must be reported
separately from failure at that product assertion.

## Owner-state diagnostic

Root authorized a separate test-only diagnostic after the exact test passed.
The original test and its passing receipt remain intact. The new diagnostic
uses the same fixture and gesture, reads `quick_range_paint(owner)` after each
frame and adds an ordinary idle frame. It asserts the owner has no temporary
range, so an absent action rectangle cannot hide a selected range.

Same child environment and target, command:

```powershell
cargo test -p quantick-app quick_range_tape_lane_diagnostic -- --nocapture
```

Session 21466 exit 0; compilation 3m 13s; 1 passed, 0 failed, 2071 filtered out;
test duration 0.11s. Diagnostic input file SHA256:
`DD2B81172842FCCA5B8D1F168ED0B0EFE1A0361073C4C2B5E00B0A850DB7A3EC`.
Diagnostic executable SHA256:
`B755AEBF3877D12E6BF925B9DD25AC133AFF8F0AA259E329D9A050DBFEC6821A`.
Source delta remains one test file, now 106 insertions: 54 preserved prior
lines, 19 exact candidate-test lines, 33 diagnostic lines. No runtime edit.

Actual diagnostic output:

```text
initial chart=[[60.0 88.0] - [1284.0 808.0]] divider=855.6 start=[1069.8 448.0] end=[775.6 393.0] layer=Some(LayerId { order: Background, id: 5A3E })
press: chart=Some([[60.0 88.0] - [1284.0 808.0]]) divider=Some(855.6) anchors=None control=None
move: chart=Some([[60.0 88.0] - [1284.0 808.0]]) divider=Some(855.6) anchors=None control=None
release: chart=Some([[60.0 88.0] - [1284.0 808.0]]) divider=Some(855.6) anchors=None control=None
idle: chart=Some([[60.0 88.0] - [1284.0 808.0]]) divider=Some(855.6) anchors=None control=None
```

## Source and history disposition

The baseline price band already excludes the live lane. The statement in
#447 finding 4 that the price band spans the lane is inconsistent with this
source path:

1. `pane.rs:923` obtains `bands(&areas)`; line 963 takes `&bands[0]` and line
   965 passes that price band to `handle_quick_range`.
2. `pane/drawing_paint.rs:68-73` carves those bands using
   `let history = self.drawing_area(areas.chart)` and `PriceBand.rect = history`.
3. `drawing_area`, lines 191-198, ends the rectangle at the existing lane
   divider, clamped within the chart.
4. `pane/quick_range.rs:48` requires that narrowed rectangle to contain the
   press. The diagnostic press x=1069.8 is outside its right edge x=855.6.

This protection predates quick-range itself, rather than being a new #501 fix:

- `e44bf3e8925ef468e19b1e4b55c9b74a7c7aef62`, 2026-08-06,
  `fix(app): retracement measures what was given back; drawings leave the tape`,
  introduced `drawing_area` ending at `last_lane_divider_x`, with a test
  asserting `band.right() == 880.0` for a divider at 880.
- `ada9d527f5acc533b926186b83a27ca84e79aa32`, 2026-08-07,
  `feat(app): draw on indicator panes, not only on the price plot`, introduced
  the band's `let history = self.drawing_area(areas.chart)` / `rect: history`.
- `0fc730c24f1cf644ec484b2df0122ba8fb988de7`, 2026-09-13,
  `feat(app): add quick range volume profiles`, introduced the price-band
  containment check. Its own `pane.rs` already has the narrowed carve and
  `drawing_area`, as verified with `git show` on that exact commit.
- `git merge-base --is-ancestor e44bf3e8 ada9d527`,
  `git merge-base --is-ancestor ada9d527 0fc730c2`, and
  `git merge-base --is-ancestor 0fc730c2 HEAD` each returned 0.
- `e45147ffec3f0531d750d32fbbcdd622fa7f5c80` moved the existing carve into
  `pane/drawing_paint.rs`; no new narrowing is attributed to that file move.

Conclusion: the documented stable visible-lane gesture is not a reproduced
defect on retained baseline 9ff57501. Both exact test and owner-state probe
pass. There is no failing assertion to report and no earned failing-before
repair claim for finding 4. Recommend correcting its disposition to existing
behavior verified by characterization, with the historical guard chain above.
No broader claim is made about untested same-frame lane/layout changes. Root
retains #447 and #501 reconciliation authority; no issue was closed or edited.
