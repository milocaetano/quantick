# Drawing copy/paste verification on latest main

## Verdict

**FAIL in the real Windows desktop application.** Ctrl+C and Ctrl+V do not copy and paste chart drawings on the latest fetched Quantick `origin/main` at verification time.

The focused egui unit tests pass, but they bypass the production Windows event adapter and therefore produce a false-positive result for these shortcuts.

## Verified revision

- Verification date: 2026-09-13
- Worktree branch: `docs/verify-copy-paste-latest`
- Runtime source revision: `d3d4b23d3741ec82204b2c0f2c77b23a98908ac4`
- Fetched `origin/main`: `d3d4b23d3741ec82204b2c0f2c77b23a98908ac4`
- PR #420 merge commit: `d8de9a1b557d3965c923feb8998bfd226e9dd067`
- Ancestry check: `git merge-base --is-ancestor d8de9a1b origin/main` exited 0.

The fresh executable and focused tests therefore covered the fetched main tip, and that tip contains the merged implementation from PR #420.

## Live Windows evidence

The executable was built immediately before launch from the isolated worktree with `CARGO_TARGET_DIR=C:\quantick-agent-target\verify-copy-paste-latest`. Its write time was 2026-09-13 22:43:23 local time.

The visible app used isolated scratch paths for every workspace store and staged one selected horizontal-line drawing through the UI harness. The live Binance feed connected, and `APP_HEALTH_SUMMARY` reported 59–60 fps with no worker backlog.

Before the shortcut, the structured health summary reported `drawings=1`. The trader pressed Ctrl+C and Ctrl+V in that window and reported that the drawing was not copied. Subsequent health summaries continued to report `drawings=1`. The real desktop path therefore failed independently of pixel interpretation.

## Automated evidence and the false positive

All four focused tests passed locally:

| Test filter | Result | Intended coverage |
| --- | --- | --- |
| `ctrl_c_then_ctrl_v_pastes_the_copied_drawing_not_the_current_selection` | PASS, 1 passed, 0 failed | Copy snapshot, selection change, paste normalization, and one undo entry. |
| `drawing_clipboard_targets_the_focused_pane_and_repeated_pastes_do_not_overlap` | PASS, 1 passed, 0 failed | Focused-pane targeting and repeated offsets. |
| `ctrl_v_before_copy_is_a_no_op` | PASS, 1 passed, 0 failed | Empty internal clipboard. |
| `duplicate_names_the_two_key_copy_paste_workflow` | PASS, 1 passed, 0 failed | Shortcut discovery text. |

The first three tests inject `egui::Event::Key` for C or V with `Modifiers::COMMAND` directly into `egui::RawInput`. That exercises Quantick's frame handler, but not the native `egui-winit` adapter that creates the real raw input.

## Root cause

`crates/app/src/app/drawing_input.rs` reads `input.key_pressed(Key::C)` and `input.key_pressed(Key::V)` when the command modifier is held.

In `egui-winit` 0.29.1, `State::on_keyboard_input` handles a real native key press before it reaches Quantick:

1. Ctrl+C pushes `egui::Event::Copy` and returns.
2. Ctrl+V reads the operating-system clipboard, may push `egui::Event::Paste`, and returns.
3. The generic `egui::Event::Key` push occurs only after those early returns.

Quantick's drawing handler does not inspect `Event::Copy` or `Event::Paste`, so its `key_pressed(C/V)` checks are false in the production desktop path. The tests synthesize the event that the native adapter deliberately does not emit, which explains why they pass while the real app fails.

## Scope and performance classification

This mission verifies and documents the defect; it does not fix runtime code. The branch changes evidence Markdown only and changes no runtime, test, fixture, schema, dependency, runtime/build configuration, hook, script, or generated input. Performance impact is **none** across per-trade, per-depth, per-frame, and rare paths.

## Local validation

- Worktree arming: `cargo build -p quantick-guards` — PASS.
- App pre-edit check: `cargo check -p quantick-app --all-targets` — PASS.
- Quantick guards after the evidence edit: `cargo test -p quantick-guards` — PASS, 228 tests passed and 0 failed.
- Diff hygiene: `git diff --check` — PASS.
- Changed relative links: none.

Durable architecture and AI reviews, exact-head CI, and the final mission status are recorded on PR #459. Earlier PASS reports on that PR are superseded by the live reproduction and corrected-head reviews.
