# Copy and paste chart drawings

Allow traders to copy and paste existing chart drawings using only Ctrl+C and Ctrl+V, so duplicating a mark does not interrupt chart work with a mouse gesture or another shortcut.

**Tier:** medium — the keyboard change remains bounded and adds no market action, but extracting its state and actions from the protected application root plus the required regression matrix exceeds the small tier's 300-line review exemption.

## Request ledger

- **R1** — Ctrl+C captures the currently selected chart drawing without mutating the chart. Source: “copiar”.
- **R2** — Ctrl+V creates an independent copy of the captured drawing. Source: “colar desenhos no grafico”.
- **R3** — The complete copy/paste workflow requires only Ctrl+C followed by Ctrl+V, with no mouse step, dialog, or additional shortcut. Source: “com apenas CTRL C + CTRL V”.

## Assumptions

- **S1** — The clipboard holds one drawing in memory for the lifetime of the application window and may be pasted into the currently focused pane or tab. This is the conventional scope for an internal chart-object clipboard and is reversible without persisted-data migration.
- **S2** — Paste preserves the drawing payload and appearance while applying the existing duplicate normalization: a fresh identity, no ambiguous copied name, unlocked geometry, a visible two-bar offset, selection of the copy, and one undo entry. Reusing established semantics avoids a second kind of duplicate.
- **S3** — Copy/paste copies the drawing only, not an armed strategy attached to its source. The request names drawings, and silently arming a strategy from a clipboard action would expand the requested action into market automation.
- **S4** — The existing Duplicate button and Ctrl+D shortcut remain compatible, but neither is required by the new workflow. “Only Ctrl+C + Ctrl+V” is read as the complete gesture requested, not as removal of existing access paths.
- **S5** — Egui's command modifier is Ctrl on Windows and Command on macOS. Tests use the repository's cross-platform command modifier while proving the requested Windows keys.

## Acceptance criteria

- [x] **A1** — With one drawing selected and no text widget focused, Ctrl+C stores a snapshot without changing the drawing collection, selection, or undo history.
      *Evidence:* a focused application interaction test that fails before the change.
      → `crates/app/src/app/tests/input_ui_tests.rs`. *(R1, R3)*
- [x] **A2** — After Ctrl+C, Ctrl+V inserts a visually equivalent drawing with a fresh identity using the established offset/unlocked/unnamed/selected semantics, as exactly one undoable edit.
      *Evidence:* application interaction and drawing-store unit tests.
      → `crates/app/src/app/tests/input_ui_tests.rs` and `crates/app/src/drawings/tests/mod.rs`. *(R2, R3)*
- [x] **A3** — Paste uses the copied snapshot rather than the current selection, works in the currently focused pane, supports repeated pastes without overlapping copies, and is a no-op before any copy.
      *Evidence:* application interaction regression tests covering selection changes, empty clipboard, pane targeting, and repeated paste.
      → `crates/app/src/app/tests/input_ui_tests.rs`. *(R1, R2, R3)*
- [x] **A4** — The selected-drawing affordance identifies Ctrl+C followed by Ctrl+V as the keyboard workflow, without adding a button, menu, dialog, or focus interruption.
      *Evidence:* context-bar unit assertion, visual QA screenshots, and trader UX verdict.
      → `crates/app/src/drawings/context_bar.rs` and `docs/quality/copy-paste-drawings-evidence.md`. *(R3)*
- [x] **G1** — Every authored repository artifact is in English.
      *Evidence:* quantick-guards language test plus manual review of prose, branch name, commits, PR title, and PR body.
      → `docs/quality/copy-paste-drawings-evidence.md`.
- [x] **G2** — Targeted tests and the final ordered `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` checks pass after rebasing on the latest `origin/main`.
      *Evidence:* command exit codes recorded at the verified head.
      → `docs/quality/copy-paste-drawings-evidence.md` and the PR body.
- [x] **G3** — Performance impact is flat at operating rates: the input path adds two boolean key checks per frame, while cloning/insertion happens only on a trader copy or paste gesture; no per-trade or per-depth path changes.
      *Evidence:* code inspection, targeted regression tests, and dense-tape `APP_HEALTH_SUMMARY` captured during visual QA with no attributable slow-frame burst.
      → `docs/quality/copy-paste-drawings-evidence.md` and the PR body.
- [x] **G4** — The changed selected-drawing flow is reachable through the existing drawing demo/select hooks and passes visual QA in normal and narrow windows with no attributable integrity, readability, occlusion, state-honesty, motion, consistency, or performance defect.
      *Evidence:* screenshots, scene/diagnostic readings where available, and a surface-by-state PASS table.
      → `docs/quality/copy-paste-drawings-evidence.md`.
- [x] **G5** — Trader UX review finds no unresolved Blocker or Should-fix for Rafa, Marina, or Duda; the flow costs exactly two keyboard gestures, steals no focus, and preserves the workspace.
      *Evidence:* persona walkthrough and verdict.
      → `docs/quality/copy-paste-drawings-evidence.md` and the PR body.
- [ ] **G6** — `arch-review` runs the medium-tier pass across all nine dimensions and resolves or validly defers every Blocker and Should-fix.
      *Evidence:* durable arch-review report and exact-head marker.
      → PR conversation, PR body, and git-dir `arch-review-ok` receipt.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

The G-AI evidence destinations are the durable PR report, the review-thread
listing, and the exact-head `ai-review-complete` receipt. They remain unchecked
before archival because the reviews run against the archived mission commit.

## Not applicable

- Engine/determinism gate — no engine, bar construction, replay clock, or deterministic market-data path changes.
- Hot-path benchmark gate — no per-trade, per-depth, or rendering work is added; the dense-tape UI health reading remains part of visual QA.
- New-extension gate — this changes the gesture for the existing drawing duplication capability and adds no feed, bar type, indicator, layer, panel, crate, or new trader action.
- New UI hook — no surface or state visible only through the new shortcut is introduced; the existing `QUANTICK_DRAWINGS_DEMO` and `QUANTICK_DRAWINGS_DEMO_SELECT` hooks reach the affected selected-drawing canvas and context bar.
- Second-operator capability expansion — drawing duplication already has a named host action, its result is enumerable in drawing snapshots, and drawing types are discoverable through `DRAWING_TOOLS`; this mission changes only the human keyboard grammar and does not add a new chart mutation.

## Closing steps

- [ ] **C1** — Run `delivery-review` over the final branch and receive PASS. Source: mission workflow; evidence in the durable review report and exact-head marker.
- [ ] **C2** — Open the pull request, publish current review evidence, obtain green final-head CI, resolve every required AI-review thread, and mark the PR ready for the trader to evaluate. Source: mission workflow and delivery contract; evidence in the PR URL, checks, review progress, and PR body.

## Request as received

Attributed trader quotation under the repository language exemption:

> $mission small permitir copiar e colar desenhos no grafico, com apenas CTRL C + CTRL V
