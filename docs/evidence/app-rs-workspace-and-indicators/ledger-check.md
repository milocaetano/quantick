# The mission brief's evidence ledger, re-measured

Every row was checked against `origin/main` at `e0ae2ac` in this worktree
before the first edit, rather than trusted. Nine rows confirmed as written;
one carried an off-by-three that would have moved a line it should not have.

| # | Claim | Verdict |
| --- | --- | --- |
| 1 | `app.rs` 7,501 lines, 7,499 production; ceiling 7,499; `!budget 59547` | **Confirmed.** `wc -l` gave 7,501; `size-baseline.txt:99` gave `!budget 59547` and `:101` gave `crates/app/src/app.rs 7499`. |
| 2 | Five siblings declared at `app.rs:21-25`, each `use super::…` plus `impl QuantickApp` | **Confirmed.** `demo_hooks`, `drawing_input`, `health`, `layout_wiring`, `workspace_restore`; `workspace_restore.rs:29` shows the `pub(super)` the moved methods now use. |
| 3 | Workspace store, `app.rs:3657-4757`, 1,101 lines, 24 named methods | **Corrected.** Every method is where the row says. The *range end* is wrong: `note_workspace` closes at `:4753` and `:4754` is the closing brace of the `impl QuantickApp` opened at `:500`. The row's `:4757` is the first menu-bar constant, and the row's own warning — "must not move" — is what the correction protects. Moved `:3650-4753` (the doc comment above `capture_workspace` starts at `:3650`), leaving `:4754` behind to close the impl. |
| 4 | Indicator manager, `app.rs:2666-3400` plus `:1676-1740`; `SCRIPT_RELOAD_POLL_INTERVAL` used only here | **Confirmed.** Every method at the line given. The constant at `:75` had exactly two mentions in the crate: its definition and one read at `:3290`, inside `poll_script_files`. It travelled. |
| 5 | Chart layers, `app.rs:3401-3657`, seven methods | **Confirmed.** All seven present at the lines given. |
| 6 | The paper branch edits `app.rs` only in `new_with_workspace`, the trades-dir pickers and `persist_*`, `adopt_tab`, one line of `draw_menu_bar`, and `arm_strategy_instance`; none overlaps | **Confirmed.** See `paper-overlap.md` for the hunk-to-function mapping, run twice. |
| 7 | The paper branch also rewrites 63 lines of `size-baseline.txt`; whichever lands second re-runs `--tighten` and resolves `!budget` by hand | **Confirmed and inherited.** This branch changes 13 lines of that file, 2 of them the `!budget` and the `app.rs` ceiling. Both branches touch `!budget`, so the second to land re-runs `--tighten`. Noted in the PR body. |
| 8 | Tests reach these methods through `QuantickApp` only; no `super::` or `crate::app::` free item named in `app/tests/` | **Confirmed for the method calls, incomplete as a prediction.** No test file names a free item by path. But the twelve test files reach `app.rs`'s *imports* through `use super::*`, and two of those imports lost their last production reader to this move. See "The one deviation" below. |
| 9 | A child module sees the parent's private fields and methods, so a moved method needs no visibility change to read `self.*` | **Confirmed, and one thing the row does not say.** Reading `self.*` needs nothing. But `app.rs` *calling* a moved method does: 46 of them gained `pub(super)`, the same visibility `workspace_restore.rs` already uses. `widened.txt` lists them. |
| 10 | No hook read moves; `QUANTICK_TOOL_FAVORITES` is read at `:806` inside `new_with_workspace`, which stays | **Confirmed.** The `declare_hooks!` slice and its 146 `QUANTICK_*` names stay in `app.rs`. Five `QUANTICK_*` strings crossed into the new files; all five are comment text or assertion messages travelling with their bodies, and `generated.txt` shows the registry and the capability inventory byte-identical. |

## The one deviation from the brief's scope

Scope point 4 says `app/tests/*.rs` change nothing. Five lines were added to
`crates/app/src/app/tests/mod.rs`: two `use` statements and a three-line
comment. No test body, name or assertion changed anywhere, and no test was
added or removed — the suite runs the same 1,894 tests.

The cause is mechanical. `CandlePreset` and `IndicatorEvent` were imported by
`app.rs` and reached the twelve test files through their one `use super::*`.
The indicator manager took the last *production* reader of each out of
`app.rs`, and the workspace has `warnings = "deny"` (`Cargo.toml:174`), so
leaving the imports in `app.rs` is a hard build error while removing them
takes the names away from the tests.

The repo has already answered this exact question once. `tests/mod.rs:30-33`
binds `DrawingsDemo` there for the same reason, with a comment saying so,
after PR #295's cut took its last production reader out of `app.rs`. The two
new bindings sit beside it under the same comment. `ChartLayer` is a third
case and is handled differently, in `app.rs`: its last remaining reader is
`heatmap_lamp_on`, which is itself a test-only method *in `app.rs`*, so a
binding in `tests/mod.rs` would not reach it. Its import is gated `#[cfg(test)]`
in place instead.
