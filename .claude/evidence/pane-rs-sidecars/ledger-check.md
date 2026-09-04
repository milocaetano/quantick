# Evidence-ledger check (R17 / A15)

Every claim in `C:\src\mission-pane-rs-sidecars.md` was re-measured against
`origin/main` at `e0ae2ac` in this worktree before any edit, rather than
trusted. All ten hold. The two notes below are refinements the check produced,
not corrections of a wrong claim.

| # | Claim | Verdict | How it was checked |
| --- | --- | --- | --- |
| 1 | 7,773 lines / 7,771 production; ceiling 7,771; `impl ChartPane` at `:1395`; `mod tests;` at `:7773` → `pane/tests/mod.rs` (2,699 lines) | **holds** | `wc -l crates/app/src/pane.rs crates/app/src/pane/tests/mod.rs`; `grep -n pane.rs crates/guards/size-baseline.txt` → `:161 … 7771`; `sed -n '1395p;7772,7773p'` |
| 2 | Context menus `:2042-2357`, five methods from `layer_checkbox` to `draw_drawing_menu_section` | **holds** | `awk` over `2035..2360` filtered to `fn ` — the five names at the stated offsets |
| 3 | Strategy badges `:2378-2709`, `badge_text_for` … `remove_strategy_for_drawing` | **holds** | same method, `2370..2712`; eleven functions, the last closing at `:2709` |
| 4 | Drawing gestures `:3332-3397` and `:3557-4241`; `interact_shared` `:3409-3546` sits between them | **holds** | `awk` over `3325..4250`; `interact_shared` found at `:3409` between the two ranges. Its side is decided in `GOAL.md` S1 — it stays |
| 5 | Axes and chrome `:6173-6455` and `:7274-7770` | **holds, refined** | `awk` over both ranges. The `impl ChartPane` block ends at `:7685`; `:7702-7770` is three *module-scope* free functions (`paint_placement_hint`, `snap_bar_to_tape`, `magnet_price_of`) outside it. The moved range is therefore `:7271-7684`, and the three stay — `GOAL.md` S3 |
| 6 | The paper branch edits `pane.rs` at one line, `:5996`, inside `draw_chart` | **holds** | `git diff origin/main...refactor/paper-policy-out-of-the-ticket -U0 -- crates/app/src/pane.rs` → one hunk, `@@ -5996 +5996 @@`, `chrome.paper.selected_trade_index()`. Re-run after the move: the line is now `:4552`, still inside `draw_chart` |
| 7 | Every `QUANTICK_*` in `pane.rs` is a comment or doc comment; no hook declared or read | **holds** | `grep -n 'QUANTICK_' crates/app/src/pane.rs` → `:1182, :1192, :1196, :1327, :6575`, all inside `//` or `///` |
| 8 | `pane/tests/mod.rs` opens `use super::*;` and calls `magnet_price_of` / `snap_bar_to_tape` by bare name; `orderflow_render.rs` calls `draw_dashed_vertical` | **holds** | `grep -rn` over `crates/app/src`. Resolved by S3: none of the three travels, so no re-export is needed and the test file is untouched |
| 9 | A child module sees the parent's private fields and methods; a moved method needs no visibility change to read `self.*` | **holds, and the converse bites** | True as stated — no moved body needed a change to reach `self`. The *reverse* direction is what costs: a private method defined in a child is private **to that child**, so the 24 moved methods still called from `pane.rs` or a sibling need `pub(super)`. See `moves.md` |
| 10 | `pane_divider_gesture` `:401`, `pane_pan_gesture` `:450`, `axis_zoom_gesture` `:533` are called only from `pane.rs` and stay | **holds** | `grep -rn` over `crates/app/src` → definitions plus four call sites, all inside `handle_navigation`. All three are byte-identical on the branch |
