# The numbers (R8, R9, R10, R13, R14, R16, R18, R19, R20)

## `pane.rs` and the ratchet (A6, A7, A8)

| | `origin/main` | branch | change |
| --- | --- | --- | --- |
| `pane.rs`, total lines | 7,773 | 5,620 | −2,153 |
| `pane.rs`, production lines (the ratchet's basis) | 7,771 | 5,618 | −2,153 |
| `size-baseline.txt` entry for `pane.rs` | 7,771 | 5,618 | tightened to match |
| `size-baseline.txt` `!budget` | 59,547 | 57,394 | **−2,153** |

Target was ≤ 5,700 lines and a budget at least 2,000 lower. Both met.
`cargo run -q -p quantick-guards -- --tighten` wrote both numbers; no ceiling
anywhere in the file was raised, so nothing had to be lowered in trade.

The four new modules are 353, 374, 744 and 802 production lines — all far under
the 1,500-line threshold, so none takes a baseline entry of its own. The rise
in `crate.lines.app` below is the honest cost of that: the lines still exist.

## The `--report` diff (A13)

`report-before.txt` was captured from `origin/main` in this worktree before the
first edit; `report-after.txt` after the last. Six metrics move, and nothing
else in the 58-line report does:

```
crate.lines.app          108188 -> 108308   (+120)
crate.lines.total        173375 -> 173495   (+120)
file.largest.pane.rs       7771 -> 5618     (-2153)
ratchet.size.budget       59547 -> 57394    (-2153)
ratchet.size.recorded     59547 -> 57394    (-2153)
ratchet.size.measured    173375 -> 173495   (+120)
app.lines.without_egui   104495 -> 104611   (+116)
```

The `+120` is the four module headers — doc comment, imports, `impl ChartPane`
wrapper — and is the only new code the branch writes. No other file's largest-
file entry, no other crate's line count, and no other ratchet moved. The
complete diff is `report.diff`.

## Generated files and tests (A11, A12)

`git diff origin/main...HEAD --stat` names six files: `pane.rs`, the four new
modules, and `size-baseline.txt`, plus this evidence directory. In particular
it does **not** name:

- `docs/control-plane/capability-inventory.md` — unchanged, byte for byte
- the generated hook registry — unchanged; brief ledger #7 predicted this and
  the check confirmed it, since every `QUANTICK_*` in `pane.rs` is a comment
- `crates/app/src/pane/tests/mod.rs` — **no diff at all**, not even a `use` line
- `crates/app/tests/panes_layout_tests.rs` — no diff
- `crates/app/tests/drawings_tests.rs` — no diff

`cargo test -p quantick-guards` is green, which is what proves the generated
files still match the code that generates them.

## Test count (A14)

Identical before and after — same tests, same outcome, none added, none lost:

```
origin/main : running 1898 tests … ok. 1894 passed; 0 failed; 4 ignored
branch      : running 1898 tests … ok. 1894 passed; 0 failed; 4 ignored
```

## Nothing out of scope moved (A16)

Each symbol extracted from `origin/main:crates/app/src/pane.rs` and from the
branch's `pane.rs`, then hashed. All identical, all still in `pane.rs`:

```
SAME  pub fn handle_navigation(   961 lines   30f89989c859f450
SAME  pub fn draw_chart(          939 lines   f125a972d493f800
SAME  fn interact_shared(         138 lines   47748c41c264708f
SAME  fn pane_divider_gesture(     33 lines   4fa1efd53066dd73
SAME  fn pane_pan_gesture(         46 lines   36f0cf4c0aa8d262
SAME  fn axis_zoom_gesture(        48 lines   4e2750c272ec174e
SAME  fn draw_dashed_vertical(     23 lines   8d94c4570f7daff2
SAME  fn paint_placement_hint(     37 lines   5041747341eea2da
SAME  fn snap_bar_to_tape(          4 lines   9ba69696702adcb6
SAME  fn magnet_price_of(          16 lines   8cee4f5833c998e9
```

`struct ChartPane` and its 77 fields never appear in the diff at all
(`git diff --cached -- crates/app/src/pane.rs | grep -c 'pub struct ChartPane'`
→ 0). No hit-test, magnet or placement arithmetic was touched: all of it is
inside the 48 methods hashed identical in `moves.md`.

## The paper branch, re-run before the PR (A17)

```
$ git diff origin/main...refactor/paper-policy-out-of-the-ticket -U0 -- crates/app/src/pane.rs
@@ -5996 +5996 @@ impl ChartPane {
-                chrome.paper.selected_trade_index(),
+                chrome.paper.account().selected_trade_index(),
```

Still one line, and still inside `draw_chart`, which this branch did not move
and did not change. On this branch that line is now `:4552`; since the
surrounding function is byte-identical, git will place the paper branch's hunk
by context without a conflict.

## The next mission's read (A18)

`pane.rs` is 5,620 lines, and the two functions the next `pane` mission exists
to reshape are contiguous, intact, and now near the middle of a file half the
size:

| | on `main` | on the branch |
| --- | --- | --- |
| `interact_shared` | `:3409-3546` | `:2660-2797` |
| `handle_navigation` | `:4254-5214` | `:2810-3770` |
| `draw_chart` | `:5223-6161` | `:3779-4717` |

Together they are 1,900 of the file's 5,620 lines — 34%, against 24% before.
That concentration is the point: what is left in `pane.rs` is much closer to
being only the pane's own two hard problems plus the state they read.
