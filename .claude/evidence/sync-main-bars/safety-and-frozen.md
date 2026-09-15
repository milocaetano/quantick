# MS1 safety and frozen-manifest evidence

The raw inspection below was collected from the no-commit merged tree and the post-commit tree. It establishes the exact path set, the two-sided automatic merge in the overlapping control-plane test, the incoming archive identity, both-parent ancestry, the single test-only integration delta, and all seven frozen blob IDs. Full workspace tests separately exercised the feed continuity, mutation uncertainty, idempotency, ticket, replay, engine, backtest, and guard suites.

## No-commit inspection

```text
INCOMING_COUNT=23
STAGED_COUNT=23
NAMESET_DIFF_BEGIN
NAMESET_DIFF_END
ARCHIVE_MAIN_BLOB=1d422e8dd83ac5d28665ae4ef20725c91eb79973
ARCHIVE_INDEX_BLOB=1d422e8dd83ac5d28665ae4ef20725c91eb79973
docs/quality/outside-score-rubric.md INDEX=0c2583cc2a71a0e15f38ae4d6ea7cf39c2fdec76 EXPECTED=0c2583cc2a71a0e15f38ae4d6ea7cf39c2fdec76
tools/outside_score/measure.py INDEX=494ff11e6231f513d500fe935fe27fe3c2d2e2e5 EXPECTED=494ff11e6231f513d500fe935fe27fe3c2d2e2e5
.claude/skills/outside-score/SKILL.md INDEX=81d101d7e0fad73f1390977be7671545da46ec51 EXPECTED=81d101d7e0fad73f1390977be7671545da46ec51
.agents/skills/outside-score/SKILL.md INDEX=45cc224fa29f2881601e03fe462e242bd50e8521 EXPECTED=45cc224fa29f2881601e03fe462e242bd50e8521
.claude/skills/quantick-score/SKILL.md INDEX=a0d008fbab2565fcfa1fc1b6894b075bf47f3526 EXPECTED=a0d008fbab2565fcfa1fc1b6894b075bf47f3526
.agents/skills/quantick-score/SKILL.md INDEX=5489246f5748b8b8abc80696d88cb75e331d6179 EXPECTED=5489246f5748b8b8abc80696d88cb75e331d6179
docs/quality/quantick-score-rubric.md INDEX=c7fd2af3aa108abdff7b3083b4321fd941d8a813 EXPECTED=c7fd2af3aa108abdff7b3083b4321fd941d8a813
CONTROL_VS_CAMPAIGN_BEGIN
diff --git a/crates/app/src/app/tests/control_plane_tests.rs b/crates/app/src/app/tests/control_plane_tests.rs
index 4366539a..665d4a47 100644
--- a/crates/app/src/app/tests/control_plane_tests.rs
+++ b/crates/app/src/app/tests/control_plane_tests.rs
@@ -5489,7 +5489,7 @@ fn incremental_lane_dense_frame_benchmark() {
     ] {
         let ctx = egui::Context::default();
         let (mut app, events, _commands, _book) = test_app();
-        app.active_tab_mut().flow_pane.spec.set(spec.clone());
+        app.active_tab_mut().flow_pane.spec.set(spec);
         app.active_tab_mut().apply_spec_changes();
         app.active_tab_mut().apply_spec_changes();
         assert_eq!(app.active_tab().flow_pane.state.spec(), &spec);
CONTROL_VS_CAMPAIGN_END
CONTROL_VS_MAIN_BEGIN
diff --git a/crates/app/src/app/tests/control_plane_tests.rs b/crates/app/src/app/tests/control_plane_tests.rs
index 47d24704..665d4a47 100644
--- a/crates/app/src/app/tests/control_plane_tests.rs
+++ b/crates/app/src/app/tests/control_plane_tests.rs
@@ -5730,7 +5730,7 @@ fn the_same_layout_key_with_different_input_is_refused_as_a_conflict() {
 /// The record for an action is written on the response worker, after the
 /// connection loop has gone back to reading. Without a key held for the
 /// duration of the dispatch both calls would find no record and both would
-/// act. Here the second is refused while the first is still queued, retryably,
+/// act. Here the second is refused while the first is still queued,
 /// and the first goes on to create exactly one layout.
 #[test]
 fn a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on() {
@@ -5778,8 +5778,8 @@ fn a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on() {
         codes::REQUEST_IN_PROGRESS
     );
     assert!(
-        response_error(&refused).retryable,
-        "the caller is told to ask again once the first has answered"
+        !response_error(&refused).retryable,
+        "an in-flight refusal cannot promise that a future retry will find a retained result"
     );

     // The refusal above was answered before any frame ran, which is the race
@@ -5875,9 +5875,9 @@ fn a_keyed_call_that_expired_before_the_application_saw_it_leaves_its_key_free()
         "a call refused on its deadline created nothing"
     );

-    // The invited retry. `control.request_in_progress` while the settle window
-    // is still open is the contract's own instruction to ask again, so this
-    // asks again rather than treating it as the answer. Each retry is served
+    // The first response proved no dispatch, so these probes can verify that
+    // the settle window eventually releases its key. The generic in-flight
+    // refusal itself does not invite another mutation. Each retry is served
     // the moment it is queued, through the same `execute_on_ui` a frame's
     // drain calls, so its own deadline is never spent waiting for a frame
     // whose budget the frame's other work used up.
CONTROL_VS_MAIN_END
```

## Post-commit inspection

```text
AUTO_MERGE_TREE=f55b2076b4580860e057298ad141cae69b67bad5
FINAL_MERGE_TREE=bee98ce07dc2c440f59f0c8272e48170623cd390
M	crates/backtest/src/bars.rs
28	0	crates/backtest/src/bars.rs
FROZEN_BEGIN
docs/quality/outside-score-rubric.md HEAD=0c2583cc2a71a0e15f38ae4d6ea7cf39c2fdec76 EXPECTED=0c2583cc2a71a0e15f38ae4d6ea7cf39c2fdec76
tools/outside_score/measure.py HEAD=494ff11e6231f513d500fe935fe27fe3c2d2e2e5 EXPECTED=494ff11e6231f513d500fe935fe27fe3c2d2e2e5
.claude/skills/outside-score/SKILL.md HEAD=81d101d7e0fad73f1390977be7671545da46ec51 EXPECTED=81d101d7e0fad73f1390977be7671545da46ec51
.agents/skills/outside-score/SKILL.md HEAD=45cc224fa29f2881601e03fe462e242bd50e8521 EXPECTED=45cc224fa29f2881601e03fe462e242bd50e8521
.claude/skills/quantick-score/SKILL.md HEAD=a0d008fbab2565fcfa1fc1b6894b075bf47f3526 EXPECTED=a0d008fbab2565fcfa1fc1b6894b075bf47f3526
.agents/skills/quantick-score/SKILL.md HEAD=5489246f5748b8b8abc80696d88cb75e331d6179 EXPECTED=5489246f5748b8b8abc80696d88cb75e331d6179
docs/quality/quantick-score-rubric.md HEAD=c7fd2af3aa108abdff7b3083b4321fd941d8a813 EXPECTED=c7fd2af3aa108abdff7b3083b4321fd941d8a813
FROZEN_END
ANCESTRY_CAMPAIGN=0
ANCESTRY_MAIN=0
MAIN_ARCHIVE_BLOB=1d422e8dd83ac5d28665ae4ef20725c91eb79973
```
