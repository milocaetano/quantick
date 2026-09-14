# Fix native drawing clipboard shortcuts on Windows

Fix native Ctrl+C and Ctrl+V handling for chart drawings on Windows while preserving text-field clipboard behavior and covering copy, paste, repeated paste, selection, and undo through the real egui event path. This restores the drawing clipboard behavior that PR #459 proved was unreachable in the desktop adapter.

**Tier:** small — the expected runtime change is localized to drawing input and its regression test, so the bounded tier applies (no interrogation, no delivery-review).

## Request ledger

- **R1** — Make native Windows Ctrl+C and Ctrl+V operate on chart drawings, using PR #459's `Event::Copy` and `Event::Paste` evidence.
- **R2** — Preserve normal copy and paste behavior when a text field has keyboard focus.
- **R3** — Add a regression test that exercises the native egui clipboard-event path rather than synthetic C/V key events.
- **R4** — Validate copy, paste, repeated pastes, resulting selection, and undo behavior.
- **R5** — Run the complete mandatory repository verification loop.
- **R6** — Open a pull request with the implementation and evidence.

## Assumptions

- **S1** — The native event path means injecting `egui::Event::Copy` and `egui::Event::Paste` into the app frame, because PR #459 identifies those as the events emitted by `egui-winit`; this is safe because it reproduces the production adapter output directly.
- **S2** — Text fields retain clipboard ownership whenever egui reports that a widget wants keyboard input; this follows the existing drawing-shortcut focus boundary and can be verified in the app test harness.
- **S3** — Repeated paste should retain the existing offset and undo semantics already implemented by the drawing store; the fix only makes the native trigger reach that behavior.
- **S4** — The touched runtime path is per-frame drawing input. The added event inspection is bounded by the frame's small input-event list; performance will be compared with a dense-tape main control because the path runs every frame.

## Acceptance criteria

- [x] **A1** — Native `Event::Copy` and `Event::Paste` copy the selected chart drawing and paste a selected duplicate.
      *Evidence:* `native_clipboard_events_paste_the_copied_drawing_not_the_current_selection` passes and asserts the copied payload, fresh identity, offset, and selection → committed test and PR body. *(R1)*
- [x] **A2** — Native clipboard events do not copy or paste drawings while a text field owns keyboard input, and egui remains able to process the same events for text editing.
      *Evidence:* `native_clipboard_events_remain_available_to_a_focused_text_field` passes, copies `chart title`, pastes `replacement`, and leaves the drawing count unchanged → committed test and PR body. *(R2)*
- [x] **A3** — The regression test injects the clipboard event variants produced by `egui-winit`, rather than C/V key events.
      *Evidence:* the committed tests inject `egui::Event::Copy` and `egui::Event::Paste` through `run_frame_with_events`; focused results pass → committed test and PR body. *(R3)*
- [x] **A4** — Copy, one paste, repeated paste, selection of each pasted drawing, and stepwise undo all produce the expected drawing state.
      *Evidence:* `drawing_clipboard_targets_the_focused_pane_and_repeated_pastes_do_not_overlap` passes with two offsets, selection `Some(1)`, then drawing counts one and zero after successive undo events → committed test and PR body. *(R4)*
- [x] **A5** — The complete mandatory fmt, clippy, build, and test loop passes on the final code tree.
      *Evidence:* fmt, workspace clippy, workspace build, and workspace tests pass; tests use the versioned `QUANTICK_BUBBLES` path to isolate the repository from a personal Windows preset → PR body. *(R5)*
- [ ] **A6** — An open pull request targets `main` from this mission branch and describes the fix and current evidence.
      *Evidence:* pull request URL → PR body and final handoff. *(R6)*
- [x] **G1** — Every authored repository and PR artifact is in English.
      *Evidence:* the language guard passes and `git diff --check` reports no finding → PR body.
- [x] **G2** — Performance impact is declared for every touched path, and the per-frame path is flat or better against a dense-tape `main` control.
      *Evidence:* three matched, alternating 8,000-trade benchmark pairs report median frame CPU 1.447 ms on `origin/main` and 1.311 ms on the candidate, 9.4% lower; the rare copy/paste edit work is unchanged → PR body.
- [ ] **G3** — `arch-review` reports PASS with every Blocker and Should-fix resolved or explicitly deferred.
      *Evidence:* durable architecture review report → PR comment and PR body.
- [x] **G4** — The affected selected-drawing and focused-text surfaces remain reachable in the UI harness and pass visual QA; the keyboard event itself stays in the native-event test because the harness forbids injected desktop input.
      *Evidence:* the existing text-note hook produced a selected drawing and focused editor at 2122×1406; two DPI-aware captures show intact controls and advancing tape, evidence `1ChMGw89tTBw6WprsligZg`, with health stabilized at 60 fps and 16.7 ms → PR comment and PR body.
- [x] **G5** — `trader-ux-review` reports no unresolved Blocker for the drawing clipboard workflow.
      *Evidence:* all three persona walks find the gesture one-step, pane-consistent, focus-safe, discoverable in the existing context-bar hint, and free of Blocker or Should-fix findings → PR comment and PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Not applicable

- Engine/determinism gate — the change stays in app input handling and does not alter bar construction or deterministic domain code.
- New-extension gate — the drawing clipboard action already exists; this repairs its native desktop trigger without adding a capability or registration point.
- New trader-action operability gate — the change repairs an existing keyboard action and does not introduce a new action.
- MT5/Python checks — no files under `tools/mt5/` or `bridge/mt5/` are touched.
- Dependency checks — `Cargo.lock` is unchanged.

## Closing steps

- [ ] **C1** — Publish the open pull request with current-head evidence.
- [ ] **C2** — Run `mission_ship_gate.sh mission <pr>` and obtain PASS after exact-head CI is green.

## Verbatim request

> User: “Corrigir o Ctrl+C e Ctrl+V de desenhos no gráfico do Quantick no Windows. Usar a evidência do PR #459: o egui-winit converte os atalhos nativos em Event::Copy e Event::Paste, enquanto o
> drawing_input.rs observa apenas Key::C e Key::V. Implementar a correção sem quebrar copiar/colar de campos de texto, adicionar um teste que cubra o caminho real dos eventos nativos, validar copiar, colar,
> colagens repetidas, seleção e undo, rodar a verificação obrigatória completa e abrir um PR.”
