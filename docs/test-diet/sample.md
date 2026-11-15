# App test diet: a classified sample of `crates/app` tests

Report only; no code changes. The question: how many `crates/app` tests drive
the host (A) to assert a result a lower layer (C) decided, so that coverage
multiplies per combination instead of living once with its owner?

## Method

- Population: every `#[test]` fn under `crates/app/src` on `feature/test-diet`
  at `e77bdfbcb`: **2,284 tests, 88,650 body lines** (body = the fn's own
  lines; helpers and fixtures are not counted).
- Selection: systematic PPS over all tests ordered by file then line. A test's
  size is its file's mean test body length, so each file receives a share of
  the 60 draws proportional to its total test body lines (heavier files get
  proportionally more, within one draw), and tests inside a file are equally
  likely. Seed **20261114**, n = **60**:

  ```sh
  python -I tools/test_diet/select_sample.py crates/app/src --n 60 --seed 20261114
  python -I tools/test_diet/select_sample.py crates/app/src --n 60 --seed 20261114 \
      --estimate docs/test-diet/sample.tsv
  ```

  The first command reproduces the draw; the second recomputes every number
  below from `sample.tsv` (the machine-readable copy of the table).
- Classification: one reader (no second rater) read each test, its helpers,
  and the production code it reaches, then grepped the owner crate for a test
  of the same behaviour.
- Extrapolation: ratio estimator (class share of the population, weighted by
  inverse selection probability) times the known totals, with a linearised
  with-replacement variance for the 95% interval. With n = 60 the intervals
  are wide; treat the point estimates as orders of magnitude.

### Classes and flags

- **a, domain-through-host**: the decision and the asserted result belong to
  a domain or headless crate; the app host is incidental.
- **b, wiring**: asserts that the app delivers an intent or effect to its
  owner, or connects domain to UI, control or persistence.
- **c, UI**: layout, paint or interaction that needs egui.
- **d, redundant**: the same behaviour is already asserted by another app
  test or a guard.
- **Mismatch**: the deciding layer is not the layer the test lives in
  (`partial` when only part of the assertions is).
- Flags, reported beside the class: `ui-free-app` (the decision is UI-free
  code that still lives in `crates/app`: not A->B->C, but it moves with its
  code), `slim` (a wiring or UI test that also re-asserts an owner's rule),
  `ignored-probe` (`#[ignore]`d measurement), `meta` (checks test sources),
  `no-app` (never builds `QuantickApp`).

## Per-test classification

| # | Test (`crates/app/src/` + file :: fn) | Lines | Class | Mismatch | Deciding layer | Asserted layer | Owner test / note | Conf. |
|---|---|---|---|---|---|---|---|---|
| 1 | `app/control_host/tests/launch.rs` :: `screenshot_permission_is_checked_before_waiting` | 18 | a [no-app] | yes | control-host launch::LaunchScenarios::prepare_evidence | control-host EvidenceCapture/EvidenceStep | control-host; no owner test for the not-granted screenshot (launch.rs tests cover scope lists and the wait budget only) | medium |
| 2 | `app/tests/bar_registry_tests.rs` :: `a_remote_bar_change_shows_on_that_panes_header` | 30 | b | no | app control handler for the bar spec, then pane header paint | painted header text | - | medium |
| 3 | `app/tests/canvas_click_tests.rs` :: `a_single_click_then_a_quick_real_double_click_still_centres` | 24 | c | no | egui click counting + app canvas gesture | pane viewport follows_live/px_per_bar | - | high |
| 4 | `app/tests/chart_view_tests.rs` :: `bar_spec_change_defers_one_frame_and_shows_the_rebuild` | 28 | b [ui-free-app] | no | app tab apply_spec_changes one-frame deferral | tab loading task, pane spec, drawings count | - | medium |
| 5 | `app/tests/control_plane_tests.rs` :: `the_channel_walks_the_gesture_a_trader_describes` | 60 | d | yes | drawings parallel_channel tool + app gesture routing | drawings draft/items, painted stroke count | app/tests/drawings_tests.rs::a_channel_being_shaped_previews_as_a_corridor; drawings/src/tools/parallel_channel.rs::the_corridor_opens_on_the_side_the_pointer_chose; drawings/src/tools/tests/mod.rs::channel_needs_three_points | medium |
| 6 | `app/tests/control_plane_tests.rs` :: `observer_per_scope_capture_cost` | 28 | b [ignored-probe] | no | app control registry capture over app state | nothing (prints timings) | - | high |
| 7 | `app/tests/control_plane_tests.rs` :: `an_observer_reaches_no_action_of_the_annotate_tier` | 62 | a | yes | control-host admission (permission) + control-schema descriptors | wire error code | control-host/tests/admission_contract.rs::a_missing_permission_is_denied_before_the_payload_is_read; app sibling control_plane_tests.rs::gateway_observer_cannot_create_a_mark_remotely_but_reads_the_registered_action | medium |
| 8 | `app/tests/control_plane_tests.rs` :: `asking_for_the_analyst_ceiling_against_an_observer_grant_lands_on_observer` | 73 | a | yes | control handshake::accept_handshake ceiling + control-host admission | control.describe result, wire error | control/tests/handshake_contract.rs::overlapping_profile_ceilings_never_grant_what_the_requested_profile_forbids; ::accepted_handshake_selects_overlap_and_downscopes_to_app_grant | medium |
| 9 | `app/tests/drawing_magnet_control_tests.rs` :: `a_tool_that_snaps_by_its_own_rule_refuses_the_object_magnet` | 61 | a | yes | drawings Drawings::set_own_magnet + control-schema drawing_magnet::no_magnet | wire refusal details, drawing magnet flag | drawings/src/tools/tests/mod.rs::set_own_magnet_refuses_what_the_bar_would_not_offer | high |
| 10 | `app/tests/drawings_tests.rs` :: `a_quote_driven_feed_says_so_where_the_side_note_goes` | 58 | b [ui-free-app] | no | app tab/feed.rs Tab::side_note | label and hover text | - (misfiled in drawings_tests.rs) | high |
| 11 | `app/tests/drawings_tests.rs` :: `a_shared_mark_is_selected_and_deleted_from_the_other_chart` | 21 | b | partial | app tab focus/delete routing + drawings pane_sync | drawings stores on both panes | - | medium |
| 12 | `app/tests/drawings_tests.rs` :: `selecting_a_drawing_raises_the_context_bar_not_the_panel` | 27 | c | no | app drawing_chrome | egui area, painted text | - | high |
| 13 | `app/tests/drawings_tests.rs` :: `an_inspector_edit_commits_on_the_pane_it_started_on` | 62 | b | no | app drawing_chrome edit gesture -> host undo on the named pane | drawings undo depth per pane | - | high |
| 14 | `app/tests/feeds_sources_tests.rs` :: `a_market_switch_leaves_no_floor_and_no_seam_behind` | 20 | b [ui-free-app] | no | app tab link reset_market_state | tab link floor and gaps | - | high |
| 15 | `app/tests/feeds_sources_tests.rs` :: `tab_shortcuts_leave_the_tabs_behind_an_open_palette_alone` | 29 | c | no | app keyboard routing with the market palette open | tab list, palette state | - | high |
| 16 | `app/tests/grid_pixel_tests.rs` :: `inherited_light_time_grid_aligns_at_each_dpi_without_moving_prices_or_excluded_grids` | 80 | c | partial | app candle paint + chart candle_pixels::crisp_line + appearance grid inheritance | painted grid shapes | - | medium |
| 17 | `app/tests/home_tab_tests.rs` :: `the_strip_and_its_tabs_paint_each_themes_tokens` | 84 | c | no | app tab strip paint + appearance tokens | painted shapes | - | high |
| 18 | `app/tests/indicator_recut_tests.rs` :: `a_context_pane_spec_change_rebuilds_it_when_only_the_flow_prefix_moves` | 56 | b | no | app tab/pane spec change -> indicator rebuild | indicator rows and columns | - | medium |
| 19 | `app/tests/input_ui_tests.rs` :: `native_clipboard_events_paste_the_copied_drawing_not_the_current_selection` | 57 | b [slim] | partial | app drawing_chrome clipboard + egui Copy/Paste routing; paste normalisation in drawings::clipboard | drawings store | slim: drawings/src/tools/tests/mod.rs::duplicate_lands_offset_unlocked_and_selected_as_one_entry; ::a_duplicate_is_a_new_object_with_no_name | medium |
| 20 | `app/tests/layers_tests.rs` :: `each_layer_switch_moves_exactly_one_owner` | 54 | b [slim] | no | app pane/layers.rs PaneLayerWrite routing each layer to its owner; defaults from config | pane layer visibility | slim: defaults half overlaps chart_layers.rs::the_shipped_default_parses_and_opens_the_flow_layers | medium |
| 21 | `app/tests/layout_readability_tests.rs` :: `drawing_text_and_forms_remain_readable_inspector` | 81 | c | no | app drawing inspector layout | painted text clip, font size, border | - | high |
| 22 | `app/tests/market_windows_tests.rs` :: `market_window_control_placement_reports_acceptance_separately_from_native_observation` | 77 | b | no | app native_windows + control-handlers native_window | egui ViewportCommand, market window observation | - | medium |
| 23 | `app/tests/objects_menu_tests.rs` :: `a_menu_ask_applies_on_its_own_chart_and_so_does_the_undo` | 42 | b | no | app drawing_chrome menu ask + undo routing to the named pane | drawings stores | - | high |
| 24 | `app/tests/pane_header_tests.rs` :: `dragging_a_sample_moves_its_own_target_and_never_the_chart` | 41 | c | no | app pane header sample drag | bubble display scale, viewport | - | high |
| 25 | `app/tests/pane_header_tests.rs` :: `the_side_headers_follow_a_dragged_divider` | 18 | c | no | app pane header layout | header control rects | - | high |
| 26 | `app/tests/panes_layout_tests.rs` :: `right_clicking_the_price_gutter_offers_inverted_chart` | 42 | c | no | app price gutter context menu | painted text | - | high |
| 27 | `app/tests/panes_layout_tests.rs` :: `ctrl_zero_reopens_a_stale_fraction_at_the_drawn_floor` | 28 | b [ui-free-app] | no | app tab canvas reopen + canvas_layout floor, reached by Ctrl+0 | canvas split fraction | - | medium |
| 28 | `app/tests/panes_layout_tests.rs` :: `replay_loaded_payload_opens_active_tab_and_clears_host_wait` | 65 | b | no | ui-model replay_browser + app host + replay loader thread | tab link replay, loading wait | - | high |
| 29 | `app/tests/paper_trading_tests.rs` :: `a_remembered_position_outranks_the_narrow_chart_auto_pin` | 34 | c | no | app drawing_chrome inspector placement | egui area, pinned flag | - | high |
| 30 | `app/tests/price_axis_control_tests.rs` :: `the_price_range_launch_hook_uses_the_same_manual_range_and_rejects_invalid_bounds` | 28 | a | yes | control-schema PriceAxisMode::parse_range + chart PriceView::set_manual_range | chart price_view manual range | control-schema; no owner test for parse_range | high |
| 31 | `app/tests/quick_range_control_tests.rs` :: `legacy_timestamp_mode_does_not_start_consuming_fallback_positions` | 22 | a | yes | control annotation::validation_bar + control-handlers annotate | wire result anchor slots | control/control-handlers; no owner test for the legacy timestamp mode found | medium |
| 32 | `app/tests/retry_readback_tests.rs` :: `every_reachable_forbidden_row_refuses_a_key_before_the_application` | 70 | a | yes | control registry idempotency policy + control-host admission | wire error | control-host/tests/admission_contract.rs::a_key_the_descriptor_forbids_is_refused; control-host/tests/capability_contract.rs::a_read_still_refuses_the_idempotency_key_its_descriptor_forbids | medium |
| 33 | `app/tests/script_editor_control.rs` :: `script_library_control_discovery_permissions_paging_and_copy_retry` | 218 | b [slim] | partial | app script editor capability wiring; stores script_library, control-host idempotency | wire results, library files, journal | slim: library and idempotency assertions belong to stores and control-host | medium |
| 34 | `app/tests/stage_order_tests.rs` :: `the_tail_before_the_canvas_never_publishes_the_offline_corner` | 19 | b | no | app frame stage order | chrome feed chip rect | - | high |
| 35 | `app/tests/tape_clock_tests.rs` :: `an_ordinary_lane_never_receives_the_new_clock` | 9 | a | yes | orderflow PaneTape::lane_now_ms | orderflow lane clock | orderflow/src/pane_tape.rs::a_pane_without_a_native_tape_still_has_no_clock | high |
| 36 | `app/tests/tape_only_scale_tests.rs` :: `tape_only_opens_on_the_beside_views_price_window` | 16 | a | yes | chart price_axis_fit::tape_only_candles_px | pane frame auto_range | chart/src/price_axis_fit.rs::tape_only_fits_the_beside_candles_and_flips_against_them; ::tape_only_measures_the_candles_of_the_beside_view_it_returns_to | medium |
| 37 | `app/tests/window_capture_tests.rs` :: `missing_and_closed_pinned_windows_are_refused_without_retargeting` | 50 | b | no | app control host window targeting + control-handlers native_window refusal | wire failure, focused window | - | medium |
| 38 | `app/tests/workspace_lifetime_tests.rs` :: `workspace_lifetime_deferred_state_waits_for_its_stable_pane_not_a_replacement` | 87 | b | no | app IndicatorViewAdapter::view_mut identity filter (retry rules in chart indicator_registry) | pane indicator views | - | high |
| 39 | `chart_layers.rs` :: `the_shipped_default_parses_and_opens_the_flow_layers` | 50 | b [ui-free-app,no-app] | no | app chart_layers.rs + crates/app/config/chart-layers.toml | parsed layer map | - | high |
| 40 | `control/retry_matrix_tests.rs` :: `every_proving_test_is_a_function_in_the_transport_suite` | 24 | b [meta,no-app] | no | app control/retry_matrix_tests (rows in control-schema, proofs in app test files) | test source text | - (guard-like) | medium |
| 41 | `indicator_legend.rs` :: `a_folded_legend_still_states_a_broken_indicator` | 20 | c [no-app] | no | app indicator_legend paint | painted text | - | high |
| 42 | `indicator_worker.rs` :: `dropping_the_lane_clears_the_samples_already_published` | 47 | a [no-app] | yes | indicator-session lane walk_lane via the app worker thread | chart IndicatorViews lane | indicator-session; no owner test in lane.rs | medium |
| 43 | `operability/mod.rs` :: `a_row_naming_an_unregistered_capability_is_drift` | 23 | a [no-app] | yes | operability::drift | Drift findings | operability; matrix.rs tests do not cover UnknownCapability | high |
| 44 | `orderflow_render/tests/flow_chrome.rs` :: `tick_chrome_keeps_live_l2_wrapping_and_header_stacks_at_the_preferred_top` | 57 | c [no-app] | no | app orderflow_render legend | painted rects and text | - | high |
| 45 | `orderflow_render/tests/mod.rs` :: `centered_flow_pies_preserve_half_and_quarter_areas_at_every_orientation` | 52 | c [no-app] | no | app orderflow_render add_sector | egui mesh area | - | high |
| 46 | `orderflow_render/tests/tape_price_tests.rs` :: `an_ordinary_btc_lane_retains_its_published_price_mapping` | 7 | c [no-app] | no | app orderflow_render bubble y mapping | painted circle centres | - | high |
| 47 | `orderflow_view/tests/dense_frame_performance.rs` :: `dense_113_second_tape_same_frame_ui_stages` | 190 | b [ignored-probe,no-app] | partial | app orderflow_view + orderflow projection | print conservation, timings | - | medium |
| 48 | `orderflow_view/tests/tape_frame_tests.rs` :: `real_worker_tape_print_to_egui_latency` | 140 | b [ignored-probe,no-app] | partial | app orderflow_view worker + orderflow projection | live mark sums, latency | - | medium |
| 49 | `orderflow_view.rs` :: `volume_dots_draw_a_fixed_tape` | 23 | a [no-app] | yes | orderflow HeatmapConfig::lane_window | lane window | orderflow/src/projection/tests/dots_tests.rs::dots_keep_the_tape_off_the_bars_but_let_it_zoom | high |
| 50 | `pane/tests/mod.rs` :: `the_tape_axis_speaks_only_when_the_tape_is_behind` | 167 | c [slim,no-app] | partial | app pane render_registry axes fit + orderflow lane_lag_label wording | painted axis text | slim: wording half duplicates orderflow/src/config/lane.rs::the_axis_says_how_old_the_newest_mark_is | medium |
| 51 | `pane/tests/mod.rs` :: `the_off_band_caret_stays_whole_inside_its_band` | 41 | c [no-app] | no | app pane bands paint_off_band_caret | painted path | - | high |
| 52 | `paper_report/tests/ledger_keys_tests.rs` :: `the_inline_rename_answers_esc_only_while_it_holds_the_keyboard` | 32 | c [no-app] | no | app paper_report ledger rename keyboard | keyboard_held, edit state | - | high |
| 53 | `paper_trading/tests/mod.rs` :: `a_selection_naming_a_missing_strategy_selects_nothing` | 12 | a [no-app] | yes | paper Account::set_order_strategies | paper account selection | paper; no owner test for an unknown name | high |
| 54 | `paper_trading/tests/mod.rs` :: `the_bracket_hook_dresses_every_rung_it_rests` | 24 | b [ui-free-app,no-app] | no | app paper_trading demo bracket hook | sim working orders brackets | - | medium |
| 55 | `replay_view.rs` :: `replay_format_paragraphs_and_csv_selection_survive_real_browser_scrolling` | 82 | c [no-app] | no | app replay_view format help | painted text, copied text | - | high |
| 56 | `surfaces/dock/tests.rs` :: `paper_report_composition_preserves_account_lifetime_and_tail_registration` | 104 | b [no-app] | no | app surface dock composition | ui-model report state per account | - | high |
| 57 | `surfaces/mod.rs` :: `borrowed_domain_rows_reborrow_in_catalog_order` | 215 | b [no-app] | no | app surface registry slot order with borrowed env | host effects order | - | high |
| 58 | `surfaces/tests/bar_switch.rs` :: `a_letter_after_the_number_lists_the_kind_that_declares_it` | 52 | a [no-app] | yes | engine bar_registry quick_matches + ui-model bar_switch | candidates, bar switch request | engine/tests/quick_switch.rs (50 Ticks (Renko)); ui-model/src/bar_switch/tests.rs::query_limits_aliases_and_empty_candidates_use_engine_rules | medium |
| 59 | `time_header.rs` :: `the_default_interval_is_one_of_the_named_presets` | 7 | b [ui-free-app,no-app] | no | app time_header DEFAULT_INTERVAL_MS vs engine TIME_PRESETS | constant membership | - | medium |
| 60 | `toolrail/tests/mod.rs` :: `unstarring_back_to_a_rail_that_fits_leaves_no_residue` | 25 | c [no-app] | no | app toolrail layout | rail view rects | - | high |

## Totals

### In the sample

| Class | Tests | Body lines | Share of sampled lines |
|---|---|---|---|
| a domain-through-host | 14 | 516 | 16% |
| b wiring | 26 | 1,754 | 54% |
| c UI | 19 | 943 | 29% |
| d redundant | 1 | 60 | 2% |
| **all** | **60** | **3,273** | 100% |

15 of 60 are a layer mismatch, exactly the (a)+(d) set; 7 more are partial.

### Extrapolated to the whole app suite (2,284 tests, 88,650 lines)

| Group | Est. tests (95% CI) | Est. body lines (95% CI) |
|---|---|---|
| a domain-through-host | 603 (314-892) | 16,713 (7,409-26,017) |
| b wiring | 916 (603-1,230) | 38,986 (25,495-52,477) |
| c UI | 738 (447-1,028) | 31,514 (17,681-45,347) |
| d redundant | 27 (0-81) | 1,437 (0-4,265) |
| **a + d** | **630 (339-921)** | **18,150 (8,507-27,792)**, about 20% of all lines (10-31%) |
| flag `ui-free-app` (inside b) | 353 (94-613) | 8,557 (1,427-15,686) |
| flag `slim` (whole bodies, not the slimmable part) | 117 (0-243) | 10,951 (0-23,215) |
| flag `ignored-probe` (inside b) | 50 (0-113) | 3,785 (0-8,338) |
| flag `no-app` | 1,036 (714-1,359) | 41,473 (27,557-55,388) |

Reading: the A->B->C pattern is real but is a minority, about a fifth of the
lines. Four fifths are wiring and UI tests that do belong in the app.

## Per file

### Top 10 files by test body lines

| File (`crates/app/src/`) | Test lines | Sampled tests | Sampled (a)+(d) / sampled lines | Est. (a)+(d) lines |
|---|---|---|---|---|
| `app/tests/control_plane_tests.rs` | 5,877 | 4 | 195 / 223 | 5,139 (likely high, see below) |
| `app/tests/drawings_tests.rs` | 5,640 | 4 | 0 / 168 | 0 sampled |
| `app/tests/panes_layout_tests.rs` | 3,612 | 3 | 0 / 135 | 0 sampled |
| `paper_trading/tests/mod.rs` | 2,680 | 2 | 12 / 36 | 893 |
| `app/tests/paper_trading_tests.rs` | 2,595 | 1 | 0 / 34 | 0 sampled |
| `app/tests/pane_header_tests.rs` | 2,514 | 2 | 0 / 59 | 0 sampled |
| `app/tests/feeds_sources_tests.rs` | 2,356 | 2 | 0 / 49 | 0 sampled |
| `pane/tests/mod.rs` | 2,297 | 2 | 0 / 208 | 0 sampled |
| `orderflow_render/tests/mod.rs` | 1,603 | 1 | 0 / 52 | 0 sampled |
| `orderflow_view.rs` (inline `cfg(test)`) | 1,495 | 1 | 23 / 23 | 1,495 |

"0 sampled" means none of that file's 1-4 draws was (a) or (d); it does not
mean the file holds none. `drawings_tests.rs` in particular re-asserts some
drawings-crate rules (test 19 is `slim` for that reason), so its true (a)+(d)
share is probably above zero.

### The three files with the most (a)+(d)

Ranked by estimated (a)+(d) lines over every file the sample touched:

1. **`app/tests/control_plane_tests.rs`**, about 5,100 lines estimated from
   3 of 4 draws. The draws were refusal and handshake rules (tests 7, 8) and a
   drawing gesture (5). A grep corroboration is lower: 14 of its 107 tests
   (841 lines) assert a refusal code or handshake field. The plausible range
   is roughly 0.8k-5k lines; it is first either way.
2. **`orderflow_view.rs`** (inline tests), about 1,500 lines from a single
   draw (test 49). One draw cannot rank a file reliably.
3. **`app/tests/retry_readback_tests.rs`**, about 1,300 lines from a single
   draw (test 32); grep shows 5 of its 16 tests (331 lines) assert refusal
   codes, so 0.3k-1.3k.

Close behind: `indicator_worker.rs` (~900, one draw) and
`paper_trading/tests/mod.rs` (~900, two draws). A cluster worth naming even
though no single file dominates: the gateway-driven control tests
(`control_plane_tests.rs`, `retry_readback_tests.rs`,
`drawing_magnet_control_tests.rs`, `price_axis_control_tests.rs`,
`quick_range_control_tests.rs`, `control_host/tests/launch.rs`) hold 7 of the
14 (a) tests in the sample. They stand up the local transport, a client and
the app to assert a rule that `control`, `control-host`, `control-schema` or
`control-handlers` decides.

## Five (a) examples and the owner API to call instead

1. **Test 30** `price_axis_control_tests.rs` ::
   `the_price_range_launch_hook_uses_the_same_manual_range_and_rejects_invalid_bounds`
   builds the app nine times to table-test a string parser. Owner API:
   `quantick_control_schema::price_axis::PriceAxisMode::parse_range(value)`
   (no owner test today) plus `quantick_chart::price_view::PriceView::set_manual_range(low, high)`.
   Keep one app case that the hook reaches `flow_pane.price_view`.
2. **Test 9** `drawing_magnet_control_tests.rs` ::
   `a_tool_that_snaps_by_its_own_rule_refuses_the_object_magnet` runs the
   gateway to see an anchored VWAP refuse its magnet. Owner APIs:
   `quantick_drawings::Drawings::set_own_magnet(id, true)` returning
   `Err(Some((_, "anchored-vwap")))` (already asserted by
   `drawings/src/tools/tests/mod.rs::set_own_magnet_refuses_what_the_bar_would_not_offer`)
   and `quantick_control_handlers::drawing_magnet::set_object(&input, |_, _| Ok(&mut drawings), journal)`
   for the refusal details, no journal on refusal and `changed` (no test in
   `control-handlers` today).
3. **Test 8** `control_plane_tests.rs` ::
   `asking_for_the_analyst_ceiling_against_an_observer_grant_lands_on_observer`
   asserts the handshake caps a requested analyst ceiling at the observer
   grant. Owner API: `quantick_control::handshake::accept_handshake(&request, &grant, &registry)`,
   already covered by
   `control/tests/handshake_contract.rs::overlapping_profile_ceilings_never_grant_what_the_requested_profile_forbids`
   and `::accepted_handshake_selects_overlap_and_downscopes_to_app_grant`.
   What only the app can prove (the access panel's grant string maps to the
   observer ceiling) is one assertion, not a gateway session.
4. **Test 35** `tape_clock_tests.rs` :: `an_ordinary_lane_never_receives_the_new_clock`.
   Owner API: `quantick_orderflow::PaneTape::lane_now_ms(native_tape: false)`,
   already asserted by
   `orderflow/src/pane_tape.rs::a_pane_without_a_native_tape_still_has_no_clock`.
   Delete candidate.
5. **Test 53** `paper_trading/tests/mod.rs` ::
   `a_selection_naming_a_missing_strategy_selects_nothing`. Owner API:
   `quantick_paper` `Account::set_order_strategies(strategies, Some("deleted"))`
   then `selected_order_strategy().is_none()`. No owner test exists; move it
   to `crates/paper/src/account/tests.rs`.

Also clear: test 49 (`HeatmapConfig::lane_window`, covered by
`orderflow/src/projection/tests/dots_tests.rs::dots_keep_the_tape_off_the_bars_but_let_it_zoom`),
test 43 (`quantick_operability::drift` with a fixture source list; the
UnknownCapability case has no owner test) and test 58
(`BUILTIN_BARS.quick_matches`, covered by `engine/tests/quick_switch.rs` and
`ui-model/src/bar_switch/tests.rs`).

## What the sample suggests

- **Delete or fold** (owner test already exists): 8 of the 14 (a) tests
  (7, 8, 9, 32, 35, 36, 49, 58) plus the (d) test 5. Where the app chooses
  the owner's input (a grant string, the registry's permission per
  capability, the pane that uses the fit), keep one thin wiring assertion
  instead of the full matrix.
- **Move** (no owner test found): 1, 30, 31, 42, 43, 53, each to its owner
  crate, with one app case left where a hook or registry feeds it.
- **Slim**: 19, 20, 33, 50 re-assert owner rules beside real wiring or UI;
  dropping the owner half keeps the wiring proof.
- **Not A->B->C but movable with their code**: the `ui-free-app` tests (4, 10,
  14, 27, 39, 54, 59) assert decisions made by UI-free code inside
  `crates/app`; they shrink only when that code moves to a headless crate.
- **Out of CI already**: the three `ignored-probe` tests (6, 47, 48) are
  measurements; they cost source lines, not CI time.

The (a)+(d) estimate, about 18k lines (8.5k-28k), is the ceiling of what the
A->B->C diet can remove directly. The larger cost of the suite sits in (b)
and (c), which are legitimate app tests; making them cheaper is a fixture
question, not a classification one.

## Uncertainties

- One reader, no second rater. 26 of 60 rows are `medium` confidence, most
  on the (a)/(b) boundary: when the app supplies one argument to an owner's
  rule (a grant string, the registry's permission table, which pane uses a
  fit) I classed the test (a) if the assertion is the owner's rule, (b) if it
  is the app's choice. Tests 7, 8, 31, 32 and 36 could reasonably be (b);
  reclassing all five as (b) puts (a)+(d) at about 12.1k lines (3.9k-20.3k), 14% of
  lines instead of 20%.
- `ui-free-app` decisions are counted in (b). Counting them as (a) would
  raise (a)+(d) by roughly 8.6k lines, but their owner is still `crates/app`.
- "No owner test" means the grep by API name and behaviour found none, not a
  proof of absence.
- Per-file figures come from 1-4 draws per file and swing by the whole file
  on one draw; the corroborating greps above are cruder still (string
  matches, not reading).
- Body lines exclude helpers and fixtures; deleting tests may free helper
  lines that are not counted here.
- The census used to size the problem overcounts app builders: 20 of the 23
  sampled tests that never build `QuantickApp` are flagged as builders by its
  helper fixpoint, because helper names such as `frame`, `print` and `state`
  collide across files. From this sample, about 1,036 tests (714-1,359) do
  not build `QuantickApp`, against 266 in the census.
