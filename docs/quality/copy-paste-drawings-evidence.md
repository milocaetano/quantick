# Copy and paste chart drawings evidence

This evidence covers the rare, trader-initiated drawing clipboard added on
`feat/copy-paste-drawings`, based on `origin/main` at `32fda5ec`. The change
does not touch bar construction, market-data ingestion, rendering geometry,
paper trading, or strategy state.

## Behaviour and regression oracles

The application-level tests drive the real egui keyboard input path. They
establish that Ctrl+C snapshots the selected drawing without changing the
chart, and Ctrl+V inserts that snapshot through the drawing store's shared copy
normalisation. The inserted drawing has a fresh identity, is unlocked and
unnamed, is offset visibly, becomes selected, and costs exactly one undo entry.

The matrix also changes the selection after copying, pastes into another
focused pane, pastes twice, and presses Ctrl+V with an empty clipboard. These
cases distinguish a clipboard snapshot from a retained collection index and
prove that paste does not mutate the source pane. Existing Ctrl+D duplication
and attached-strategy tests remain green; clipboard paste intentionally copies
only the drawing and cannot silently arm automation.

Focused commands, all passing:

```text
cargo test -p quantick-app ctrl_c_then_ctrl_v
cargo test -p quantick-app drawing_clipboard_targets
cargo test -p quantick-app ctrl_v_before_copy
cargo test -p quantick-app duplicate_names_the_two_key_copy_paste_workflow
cargo test -p quantick-app ctrl_d_duplicates_the_selection_offset_and_selected
cargo test -p quantick-app drawings::tests::duplicate_lands_offset_unlocked_and_selected_as_one_entry
cargo test -p quantick-app paper_trading_tests::duplicating_a_band_carries_its_armed_strategy_but_not_its_state
```

The selected-drawing bar keeps its existing Duplicate button and adds no new
control. Its tooltip now names `Ctrl+C, Ctrl+V`; a unit assertion freezes that
discoverability text. Copy also raises the existing workspace-note surface with
the next step, while paste itself is visible as the newly selected offset copy.

## Visual QA

The screenshots named below were captured locally and are intentionally not
tracked; `.gitignore` excludes `.claude/evidence/**/*.png`. Every run used the
worktree-built executable, selected a real drawing through an existing harness
hook, and pointed all persistent cockpit stores at a run-specific scratch
directory. No trader store or unrelated Quantick process was changed.

| Surface and state | Requested window | Semantic state | Health | Verdict |
| --- | --- | --- | --- | --- |
| Normal split chart, all registered drawings, horizontal line selected | 1400 x 900 pt | 22 drawings, context bar visible | 59-60 fps, no `APP_SLOW_FRAMES` or error | PASS |
| Narrow split chart, one fixed-range profile selected | 1000 x 650 pt | 1 drawing, complete context bar visible | 59 fps, worst sampled frame 17.26 ms, no `APP_SLOW_FRAMES` or error | PASS |

Pixel readings:

- `shots/normal-1.png` and `shots/normal-2.png` are 1415 x 937 physical pixels.
  The selected line and every existing context-bar action remain visible; the
  changed tooltip does not alter the bar's dimensions or chart layout.
- `shots/narrow-frvp-1.png` and `shots/narrow-frvp-2.png` are 1015 x 687
  physical pixels. The selected profile, handles, chart provenance, and full
  one-row context bar remain inside the window.
- The pair captures and advancing health summaries provide motion sanity. No
  blank, frozen, clipped, misleading, or newly occluded changed surface was
  observed. The bar retains its established overlay placement over chart
  content; this patch changes only hover copy and keyboard handling.

One deliberately dense setup was rejected as a performance capture rather than
hidden: a 1000 x 650 split window with all 22 demo drawings and the full tape
measured 36-40 fps and logged 23 slow-frame summaries. A scoped single-pane run
of those same 22 drawings reached 59-60 fps, and the realistic narrow
single-drawing split above reached 59 fps. There is no control-build comparison,
so the dense composite observation is not attributed to this patch and is not
used to claim performance. The changed code adds only two key-state booleans per
frame; drawing clones and insertions occur solely on explicit keystrokes.

The observer adapter initialized successfully against the instrumented build,
and the app logged one enabled gateway. `quantick_describe` did not answer
within 30 seconds, so scene and diagnostic reads are reported unavailable for
this pass. The screenshots plus structured `APP_HEALTH_SUMMARY` readings are the
visual verdict's evidence; no control-plane result is claimed.

## Trader UX review

- **Rafa (fast scalper): PASS.** With a mark already selected, the complete
  action is Ctrl+C then Ctrl+V. There is no pointer trip, modal surface, focus
  change, or extra confirmation. Repeated paste advances each copy instead of
  stacking it invisibly.
- **Marina (precision workflow): PASS.** Copy is read-only, paste targets the
  focused pane, the source remains unchanged, and one undo removes one pasted
  object. The clipboard holds a value snapshot, so a later selection or delete
  cannot redirect it.
- **Duda (new user): PASS.** The familiar keys work while the chart owns the
  keyboard. The existing Duplicate affordance advertises the two-key workflow,
  and the copy acknowledgement says which key completes it. Focused text inputs
  continue to own ordinary text copy and paste.

Verdict: no Blocker or Should-fix. The workflow costs exactly two keyboard
gestures and preserves existing Ctrl+D and button access for compatibility.

## Verification results

The final ordered local loop after the implementation and latest
`origin/main` fetch passed:

```text
cargo fmt --all -- --check                 PASS
cargo clippy --workspace --all-targets     PASS
cargo build --workspace                    PASS
cargo test --workspace                     PASS
```

The first Clippy attempt found two test-only `&[value.clone()]` expressions.
They were replaced with `std::slice::from_ref`, then the ordered four-command
loop was restarted from formatting and passed. The workspace test command was
run with the caller's unrelated `QUANTICK_BUBBLES` override removed from that
child shell; with the override present, two existing order-flow configuration
tests read the user's external preset and fail independently of this diff.

Architecture review, delivery review, exact-head CI, and PR review evidence are
closing gates and are published on the containing pull request. Main merge
remains exclusively the user's action.
