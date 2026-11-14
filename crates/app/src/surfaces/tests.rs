use super::*;
use quantick_workspace::market_list::PaletteIntent;

#[test]
fn borrowed_domain_rows_reborrow_in_catalog_order() {
    use quantick_ui_model::{Slot, borrowed_first, borrowed_second};
    fn pass(
        registry: &mut Surfaces,
        ctx: &egui::Context,
        slot: Slot,
        mut env: Option<&mut BorrowedSurfaceEnv<'_>>,
    ) -> (SurfaceResponse, egui::Rect) {
        let mut response = SurfaceResponse::default();
        let mut canvas = egui::Rect::NOTHING;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                response = registry.draw_slot_unscoped_with_env(
                    slot,
                    &mut ViewContext { ctx, ui: None },
                    env.as_deref_mut(),
                );
                egui::CentralPanel::default()
                    .frame(egui::Frame::none())
                    .show(ctx, |ui| {
                        canvas = ui.max_rect();
                    });
            },
        );
        (response, canvas)
    }
    let mut registry = Surfaces::default();
    registry
        .dock
        .open_tab(quantick_workspace::dock::DockTab::Session);
    registry
        .borrowed_first
        .update(borrowed_first::Intent::Enable);
    registry
        .borrowed_second
        .update(borrowed_second::Intent::Enable);
    let mut paper = crate::paper_trading::PaperTrading::new();
    let ctx = egui::Context::default();
    for initial in [false, true] {
        let mut settings = paper.cmd_trading();
        settings.enabled = initial;
        paper.set_cmd_trading(settings);
        let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
            paper: &mut paper,
            replay: None,
            timezone: Default::default(),
        });
        let (response, canvas) = pass(&mut registry, &ctx, Slot::LateEdgePanels, Some(&mut env));
        assert_eq!(
            paper.cmd_trading().enabled,
            initial,
            "both owner mutations occurred"
        );
        assert_eq!(registry.borrowed_first.model_state.observed, Some(initial));
        assert_eq!(
            registry.borrowed_second.model_state.observed,
            Some(!initial)
        );
        assert_eq!(registry.borrowed_first.view.immediate, Some(initial));
        assert_eq!(registry.borrowed_second.view.immediate, Some(!initial));
        assert_eq!(
            response.model_effects,
            vec![
                HostEffect::BorrowedFirst(borrowed_first::Effects(Some((initial, "session")))),
                HostEffect::BorrowedFirst(borrowed_first::Effects(Some((initial, "returned")))),
                HostEffect::BorrowedSecond(borrowed_second::Effects(Some((!initial, "session")))),
                HostEffect::BorrowedSecond(borrowed_second::Effects(Some((!initial, "returned")))),
            ],
            "session then returned response, once per row; host applies after pass"
        );
        let paper_output = response.paper_panel.unwrap();
        assert_eq!(paper_output.navigate_to_trade, Some((11, 12)));
        assert_eq!(
            paper_output.journal_edit,
            Some(quantick_paper::report::session_edit::JournalEdit::Delete {
                session: quantick_paper::report::sessions::SessionKey::parse("PROBE/first.csv")
                    .unwrap(),
            })
        );
        assert!(paper_output.close_replay && paper_output.open_replay_browser);
        assert!(paper_output.pick_trades_dir && paper_output.cmd_trading_changed);
        assert!(paper_output.order_strategies_changed && paper_output.risk_settings_changed);
        let dock_body = egui::containers::panel::PanelState::load(
            &ctx,
            egui::Id::new((ctx.viewport_id(), "dock_body", 0usize)),
        )
        .expect("real Dock row runs between the paper probes")
        .rect;
        let first = registry.borrowed_first.view.painted.unwrap();
        let second = registry.borrowed_second.view.painted.unwrap();
        assert_eq!(
            first,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(37.0, 600.0))
        );
        assert_eq!(
            second,
            egui::Rect::from_min_max(egui::pos2(37.0, 0.0), egui::pos2(dock_body.left(), 29.0))
        );
        assert_eq!(
            canvas,
            egui::Rect::from_min_max(egui::pos2(37.0, 29.0), egui::pos2(dock_body.left(), 600.0))
        );
    }
    let presets = crate::drawings::presets::PresetStore::load_from(std::path::Path::new(
        "missing-borrowed-probe-presets.toml",
    ));
    let drawings = DrawingEnv {
        pane_id: 42,
        selected: None,
        chart_area: None,
        automatic_bar_area: None,
        focused_chart_area: None,
        candles_right: None,
        legends: None,
        auto_range: None,
        selected_bbox: None,
        selected_band: None,
        tab: 0,
        side: crate::pane::PaneSide::Flow,
        drawing_tool_armed: false,
        toolbox_dock: Default::default(),
        authored_objects: 0,
        manager_rows: &[],
        presets: &presets,
    };
    let mut wrong = BorrowedSurfaceEnv::Drawings(&drawings);
    for env in [None, Some(&mut wrong)] {
        // Seed stale observations independently before each absent/wrong facet.
        let mut paper_env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
            paper: &mut paper,
            replay: None,
            timezone: Default::default(),
        });
        pass(
            &mut registry,
            &ctx,
            Slot::LateEdgePanels,
            Some(&mut paper_env),
        );
        assert!(registry.borrowed_first.model_state.observed.is_some());
        assert!(registry.borrowed_second.model_state.observed.is_some());
        let (response, canvas) = pass(&mut registry, &ctx, Slot::LateEdgePanels, env);
        assert_eq!(response, SurfaceResponse::default());
        assert_eq!(
            canvas,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))
        );
        assert!(registry.borrowed_first.model_state.observed.is_none());
        assert!(registry.borrowed_second.model_state.observed.is_none());
        assert!(registry.borrowed_first.view.immediate.is_none());
        assert!(registry.borrowed_second.view.immediate.is_none());
        assert!(registry.borrowed_first.view.painted.is_none());
        assert!(registry.borrowed_second.view.painted.is_none());
    }
    let BorrowedSurfaceEnv::Drawings(env) = &wrong else {
        unreachable!()
    };
    assert_eq!(env.pane_id, 42);
    // Borrowed-only rows do not intercept ordinary/default views, even when
    // the pass carries a facet they do not support.
    for has_env in [false, true] {
        registry.first.view.staged = Some([(3, 10), (2, 20)]);
        let before = registry.first.model_state;
        let (response, _) = pass(
            &mut registry,
            &ctx,
            Slot::Overlay,
            has_env.then_some(&mut wrong),
        );
        assert_eq!(registry.first.view.painted, Some(before + 3));
        assert_eq!(registry.first.model_state, before + 5);
        let first_effects: Vec<_> = response
            .model_effects
            .into_iter()
            .filter(|effect| matches!(effect, HostEffect::First(_)))
            .collect();
        assert_eq!(
            first_effects,
            vec![
                HostEffect::First((before + 3, 10)),
                HostEffect::First((before + 5, 20))
            ]
        );
    }
    let before = paper.cmd_trading();
    let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
        paper: &mut paper,
        replay: None,
        timezone: Default::default(),
    });
    let (response, canvas) = pass(&mut registry, &ctx, Slot::StatusLine, Some(&mut env));
    assert_eq!(response, SurfaceResponse::default());
    assert_eq!(paper.cmd_trading(), before);
    assert!(registry.borrowed_first.view.painted.is_none());
    assert!(registry.borrowed_second.view.painted.is_none());
    assert_eq!(
        canvas,
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))
    );
}

#[test]
fn borrowed_panels_use_one_catalog_dispatcher_without_root_edits() {
    borrowed_domain_rows_reborrow_in_catalog_order();
    dock::tests::dock_borrowed_pass_preserves_strip_body_and_host_effect_order();
    dock::tests::dock_registration_retains_viewport_hit_regions();
    drawing_chrome::tests::pinned_inspector_borrowed_dispatch_shares_the_floating_owner();
}

#[test]
fn edge_panels_dock_in_catalog_order_before_canvas() {
    use quantick_ui_model::{Slot, chrome_edge_first, chrome_edge_second};
    let mut registry = Surfaces::default();
    registry.update(HostIntent::ChromeEdgeFirst(Box::new(
        chrome_edge_first::Intent::Show(true),
    )));
    registry.update(HostIntent::ChromeEdgeSecond(Box::new(
        chrome_edge_second::Intent::Show(true),
    )));
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut canvas = egui::Rect::NOTHING;
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            response =
                registry.draw_slot_unscoped(Slot::EdgePanels, &mut ViewContext { ctx, ui: None });
            egui::CentralPanel::default()
                .frame(egui::Frame::none())
                .show(ctx, |ui| {
                    canvas = ui.max_rect();
                });
        },
    );
    let first = registry
        .chrome_edge_first
        .view
        .painted
        .expect("first panel painted");
    let second = registry
        .chrome_edge_second
        .view
        .painted
        .expect("second panel painted");
    assert_eq!(first.width(), 37.0);
    assert_eq!(second.height(), 29.0);
    assert_eq!(first.top(), screen.top());
    assert_eq!(first.bottom(), screen.bottom());
    assert_eq!(
        second.left(),
        first.right(),
        "the second row reserves after the first"
    );
    assert_eq!(canvas.left(), first.right());
    assert_eq!(
        canvas.top(),
        second.bottom(),
        "both reservations precede canvas"
    );
    let probe_effects: Vec<_> = response
        .model_effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                HostEffect::ChromeEdgeFirst(_) | HostEffect::ChromeEdgeSecond(_)
            )
        })
        .collect();
    assert_eq!(
        probe_effects,
        vec![
            &HostEffect::ChromeEdgeFirst(chrome_edge_first::Effects(Some(1))),
            &HostEffect::ChromeEdgeSecond(chrome_edge_second::Effects(Some(1))),
        ]
    );
    registry
        .chrome_edge_first
        .update(chrome_edge_first::Intent::Show(false));
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            response =
                registry.draw_slot_unscoped(Slot::EdgePanels, &mut ViewContext { ctx, ui: None });
            egui::CentralPanel::default()
                .frame(egui::Frame::none())
                .show(ctx, |ui| {
                    canvas = ui.max_rect();
                });
        },
    );
    assert!(registry.chrome_edge_first.view.painted.is_none());
    let second = registry
        .chrome_edge_second
        .view
        .painted
        .expect("visible panel remains");
    assert_eq!(
        second.left(),
        first.left(),
        "hidden panels release their reservation"
    );
    assert_eq!(canvas.top(), second.bottom());
    assert_eq!(registry.chrome_edge_first.model_state.value, 1);
    assert_eq!(registry.chrome_edge_second.model_state.value, 2);
    assert!(
        response
            .model_effects
            .contains(&HostEffect::ChromeEdgeSecond(chrome_edge_second::Effects(
                Some(2)
            )))
    );
    assert!(
        !response
            .model_effects
            .iter()
            .any(|effect| matches!(effect, HostEffect::ChromeEdgeFirst(_)))
    );
}

#[test]
fn early_edge_panels_dispatch_in_order_before_rail_and_dock() {
    use quantick_ui_model::{Slot, chrome_early_first, chrome_early_second, chrome_edge_first};
    let mut registry = Surfaces::default();
    registry.update(HostIntent::ChromeEarlyFirst(Box::new(
        chrome_early_first::Intent::Show(true),
    )));
    registry.update(HostIntent::ChromeEarlySecond(Box::new(
        chrome_early_second::Intent::Show(true),
    )));
    registry
        .chrome_edge_first
        .update(chrome_edge_first::Intent::Show(true));
    let ctx = egui::Context::default();
    let mut effects = Vec::new();
    let mut dock = egui::Rect::NOTHING;
    let mut canvas = egui::Rect::NOTHING;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ctx| {
            effects.extend(
                registry
                    .draw_slot_unscoped(Slot::EarlyEdgePanels, &mut ViewContext { ctx, ui: None })
                    .model_effects,
            );
            assert!(
                registry.chrome_edge_first.view.painted.is_none(),
                "the drawing rail stage has not run"
            );
            effects.extend(
                registry
                    .draw_slot_unscoped(Slot::EdgePanels, &mut ViewContext { ctx, ui: None })
                    .model_effects,
            );
            dock = egui::SidePanel::right("probe-dock")
                .frame(egui::Frame::none())
                .show(ctx, |_| {})
                .response
                .rect;
            egui::CentralPanel::default()
                .frame(egui::Frame::none())
                .show(ctx, |ui| {
                    canvas = ui.max_rect();
                });
        },
    );
    let first = registry.chrome_early_first.view.painted.unwrap();
    let second = registry.chrome_early_second.view.painted.unwrap();
    let rail = registry.chrome_edge_first.view.painted.unwrap();
    assert_eq!(first.left(), 0.0);
    assert_eq!(first.width(), 37.0);
    assert_eq!(second.left(), first.right());
    assert_eq!(second.top(), 0.0);
    assert_eq!(second.height(), 29.0);
    let drawing_rail = egui::containers::panel::PanelState::load(
        &ctx,
        egui::Id::new((ctx.viewport_id(), "drawing_toolbox_left")),
    )
    .expect("the real registered drawing rail reserves its panel")
    .rect;
    assert_eq!(drawing_rail.left(), first.right());
    assert_eq!(drawing_rail.top(), second.bottom());
    assert_eq!(
        drawing_rail.width(),
        quantick_chart_interaction::tool_rail::TOOLBOX_THICKNESS_PX
    );
    assert_eq!(rail.left(), drawing_rail.right());
    assert_eq!(rail.top(), second.bottom());
    assert_eq!(dock.top(), second.bottom());
    assert_eq!(canvas.left(), rail.right());
    assert_eq!(canvas.right(), dock.left());
    assert_eq!(canvas.top(), second.bottom());
    let probes: Vec<_> = effects
        .into_iter()
        .filter(|effect| {
            matches!(
                effect,
                HostEffect::ChromeEarlyFirst(_)
                    | HostEffect::ChromeEarlySecond(_)
                    | HostEffect::ChromeEdgeFirst(_)
            )
        })
        .collect();
    assert_eq!(
        probes,
        vec![
            HostEffect::ChromeEarlyFirst(chrome_early_first::Effects(Some(1))),
            HostEffect::ChromeEarlySecond(chrome_early_second::Effects(Some(1))),
            HostEffect::ChromeEdgeFirst(chrome_edge_first::Effects(Some(1))),
        ]
    );
}

#[test]
fn scoped_catalog_keeps_panes_independent_and_reclaims_removed_owners() {
    let mut registry = Surfaces::default();
    registry.update_scoped(10, HostIntent::First(Box::new((3, 17))));
    registry.second.update_scoped(10, (0, 17));
    registry.update_scoped(20, HostIntent::First(Box::new((8, 17))));
    assert_eq!(*registry.first.scoped(10).0, 3);
    assert_eq!(*registry.first.scoped(20).0, 8);
    let ctx = egui::Context::default();
    registry.first.scoped(10).1.enabled = true;
    registry.first.scoped(10).1.intent = Some((5, 17));
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let target = &mut ViewContext { ctx, ui: Some(ui) };
            assert!(
                !registry
                    .draw_slot_scoped(10, quantick_ui_model::Slot::Toolbar, target)
                    .undo_drawing
            );
            assert!(
                registry
                    .draw_slot_scoped(10, quantick_ui_model::Slot::Overlay, target)
                    .undo_drawing
            );
        });
    });
    assert_eq!(*registry.first.scoped(10).0, 8);
    assert_eq!(registry.first.model_state, 0);
    registry.retain_slot_scopes(quantick_ui_model::Slot::Overlay, |scope| scope == 20);
    assert_eq!(*registry.first.scoped(10).0, 0);
    assert_eq!(*registry.first.scoped(20).0, 8);
}

#[test]
fn slot_cleanup_keeps_other_owners_and_removes_closed_windows() {
    use quantick_ui_model::Slot;
    let mut registry = Surfaces::default();
    registry.update_scoped(10, HostIntent::First(Box::new((3, 17))));
    registry.second.update_scoped(10, (7, 17));
    registry.second.update_scoped(20, (9, 17));
    registry.retain_slot_scopes(Slot::PaneHeader, |_| false);
    assert_eq!(*registry.first.scoped(10).0, 3);
    assert_eq!(*registry.second.scoped(10).0, 14);
    registry.retain_slot_scopes(Slot::Toolbar, |scope| scope == 20);
    assert_eq!(*registry.first.scoped(10).0, 3);
    assert_eq!(*registry.second.scoped(10).0, 0);
    assert_eq!(*registry.second.scoped(20).0, 18);
    registry.retain_slot_scopes(Slot::Overlay, |_| false);
    assert_eq!(*registry.first.scoped(10).0, 0);
    assert_eq!(*registry.second.scoped(20).0, 18);
}

#[test]
fn registered_view_routes_intents() {
    let mut registry = Surfaces::default();
    registry.second.view.enabled = true;
    registry.second.view.intent = Some((4, 17));
    let ctx = egui::Context::default();
    let mut effects = SurfaceResponse::default();
    let output = ctx.run(Default::default(), |ctx| {
        assert!(!registry.draw_all(ctx, &env()).undo_drawing);
        for slot in [
            quantick_ui_model::Slot::PaneHeader,
            quantick_ui_model::Slot::TitleBar,
            quantick_ui_model::Slot::MenuBar,
            quantick_ui_model::Slot::TabStrip,
            quantick_ui_model::Slot::StatusLine,
        ] {
            assert!(
                !registry
                    .draw_slot(slot, &mut ViewContext { ctx, ui: None }, &env())
                    .undo_drawing
            );
            assert_eq!(
                registry.second.model_state, 0,
                "other slots cannot consume an intent"
            );
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            effects = registry.draw_slot(
                quantick_ui_model::Slot::Toolbar,
                &mut ViewContext { ctx, ui: Some(ui) },
                &env(),
            );
        });
    });
    assert!(!output.shapes.is_empty());
    assert_eq!(registry.second.view.painted, Some(0));
    assert_eq!(registry.second.model_state, 8);
    assert_eq!(registry.first.model_state, 0);
    assert!(effects.undo_drawing);
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            assert!(
                !registry
                    .draw_slot(
                        quantick_ui_model::Slot::Toolbar,
                        &mut ViewContext { ctx, ui: Some(ui) },
                        &env()
                    )
                    .undo_drawing
            );
        });
    });
    assert_eq!(registry.second.view.painted, Some(8));
    assert_eq!(
        registry.second.model_state, 8,
        "painting does not run update"
    );
}

fn env() -> SurfaceEnv<'static> {
    SurfaceEnv::quiet(Instant::now())
}
fn drive(surface: &mut dyn Surface, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
    let mut response = SurfaceResponse::default();
    response.merge(surface.draw(ctx, env));
    response
}
#[test]
fn an_outside_typed_view_delivers_boolean_and_value_requests() {
    let mut adapter = Adapter::<quantick_ui_model::first::Model, _>::new(
        first_view::View {
            enabled: true,
            intent: Some((8, 17)),
            ..Default::default()
        },
        |effect| {
            let mut response = first_view::effects(effect);
            let mut style = crate::style::ChartStyle::default();
            style.canvas.grid_enabled = !style.canvas.grid_enabled;
            response.style = Some(style);
            response
        },
    );
    assert_eq!(adapter.view.id(), "first-fixture");
    let ctx = egui::Context::default();
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(Default::default(), |ctx| {
        response = drive(&mut adapter, ctx, &env());
    });
    assert!(response.undo_drawing);
    assert_eq!(response.model_effects, vec![HostEffect::First((8, 17))]);
    assert_eq!(
        response.style.unwrap().canvas.grid_enabled,
        !crate::style::ChartStyle::default().canvas.grid_enabled
    );
    assert_eq!(adapter.view.painted, Some(0));
    adapter.view.intent = Some((1, 18));
    let _ = ctx.run(Default::default(), |ctx| {
        response = drive(&mut adapter, ctx, &env());
    });
    assert!(
        !response.undo_drawing,
        "a value request does not imply Undo"
    );
    assert_eq!(response.model_effects, vec![HostEffect::First((9, 18))]);
    assert_eq!(
        response.style.unwrap().canvas.grid_enabled,
        !crate::style::ChartStyle::default().canvas.grid_enabled
    );
    let _ = ctx.run(Default::default(), |ctx| {
        assert_eq!(drive(&mut adapter, ctx, &env()), SurfaceResponse::default());
    });
}

/// Every value-carrying request folds first-wins, not just the workspace
/// name that was the only one when the rule was written. Checked over the
/// fields together, because the failure this guards against is a new
/// field being folded with assignment — which silently lets whichever
/// surface is drawn last overwrite an earlier ask.
#[test]
fn every_valued_request_keeps_the_first_asker() {
    let mut first = SurfaceResponse {
        style: Some(crate::style::ChartStyle::default()),
        log_style_change: Some(StyleLogRequest {
            applied_preset: None,
        }),
        footprint: Some(FootprintChange::ResetToDefault),
        market: Some(PaletteIntent::Remove {
            feed_id: "first".to_owned(),
            symbol: "FIRST".to_owned(),
        }),
        test_alert: Some(crate::audio::Cue::default()),
        ..SurfaceResponse::default()
    };
    let mut contested = crate::style::ChartStyle::default();
    contested.canvas.grid_enabled = !contested.canvas.grid_enabled;
    first.merge(SurfaceResponse {
        paper_panel: None,
        model_effects: Vec::new(),
        indicator_actions: Vec::new(),
        script_editor_actions: Vec::new(),
        style: Some(contested),
        log_style_change: Some(StyleLogRequest {
            applied_preset: Some(crate::style::CandlePreset::Classic),
        }),
        footprint: Some(FootprintChange::Applied(Box::default())),
        market: Some(PaletteIntent::Remove {
            feed_id: "second".to_owned(),
            symbol: "SECOND".to_owned(),
        }),
        arm_strategy: None,
        bar_switch: None,
        test_alert: Some(crate::audio::Cue::new(
            crate::audio::AlertSound::Critical,
            None,
        )),
        undo_drawing: false,
    });
    assert_eq!(first.footprint, Some(FootprintChange::ResetToDefault));
    assert_eq!(
        first.market,
        Some(PaletteIntent::Remove {
            feed_id: "first".to_owned(),
            symbol: "FIRST".to_owned(),
        })
    );
    // The three the first version of this test set but never contested,
    // which is how a field folded with assignment instead of `or` would
    // have shipped: an appearance edit silently clobbered by whatever
    // surface happened to draw after the style panel.
    assert_eq!(
        first.style.map(|style| style.canvas.grid_enabled),
        Some(crate::style::ChartStyle::default().canvas.grid_enabled),
        "the first surface's appearance survives a later one's"
    );
    assert_eq!(
        first.log_style_change,
        Some(StyleLogRequest {
            applied_preset: None
        })
    );
    assert_eq!(first.test_alert, Some(crate::audio::Cue::default()));
}

#[test]
fn a_shipped_surface_and_an_outside_typed_view_take_the_same_path() {
    let ctx = egui::Context::default();
    let mut real = Adapter::<quantick_ui_model::agent_popup::Model, _>::new(
        agent_popup::View,
        agent_popup::effects,
    );
    let mut outside = Adapter::<quantick_ui_model::second::Model, _>::new(
        second_view::View::default(),
        second_view::effects,
    );
    assert_eq!(outside.view.id(), "second-fixture");
    assert_eq!(outside.view.apply_env_hook(&env()), None);
    assert_eq!(outside.model_state, 0);
    let _ = ctx.run(Default::default(), |ctx| {
        assert_eq!(drive(&mut real, ctx, &env()), SurfaceResponse::default());
        assert_eq!(drive(&mut outside, ctx, &env()), SurfaceResponse::default());
    });
    assert_eq!(outside.view.painted, None);
}

/// A response is built once per registered surface and once for the
/// fold, **every frame**, and returned by value each time. So its size is
/// a per-frame cost, and the way it grows is by someone adding a field
/// that carries a whole configuration by value instead of boxing it —
/// which is exactly what [`FootprintChange::Applied`] and
/// [`ArmRequest::form`] box for.
///
/// The ceiling is generous rather than exact: struct layout is the
/// compiler's business and pinning today's number would fail on a
/// harmless field reorder. It is set where an unboxed configuration
/// would break it and ordinary growth would not — 264 bytes today, and
/// one unboxed `StoredPreset` is over 300 on its own.
const RESPONSE_SIZE_CEILING: usize = 512;

/// See [`RESPONSE_SIZE_CEILING`].
#[test]
fn a_response_stays_small_enough_to_return_every_frame() {
    let size = std::mem::size_of::<SurfaceResponse>();
    assert!(
        size <= RESPONSE_SIZE_CEILING,
        "SurfaceResponse is {size} bytes, over the {RESPONSE_SIZE_CEILING} ceiling — a new \
             request is carrying a configuration by value where it should box it"
    );
}

/// Order must not decide the outcome: a later surface with nothing to ask
/// cannot clear an earlier surface's request. Folding with `|=` rather
/// than assignment is what buys that, and this pins it.
#[test]
fn a_quiet_surface_cannot_cancel_an_earlier_request() {
    let mut merged = SurfaceResponse {
        undo_drawing: true,
        ..SurfaceResponse::default()
    };
    merged.merge(SurfaceResponse::default());
    assert!(merged.undo_drawing);
}
