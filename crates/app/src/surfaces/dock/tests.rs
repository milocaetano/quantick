#[cfg(test)]
pub(crate) use cases::{
    dock_borrowed_pass_preserves_strip_body_and_host_effect_order,
    dock_registration_retains_viewport_hit_regions,
};
#[cfg(test)]
mod cases {
    use super::super::*;
    use crate::dock::holds_pointer;
    use crate::paper_trading::PaperTrading;
    use crate::surfaces::Surfaces;
    use crate::timezone::TzOffset;

    /// Lay the whole dock out for real, off-screen, on every tab — a broken
    /// nested layout, a duplicated panel id or a body that panics fails here
    /// instead of on the chart. Two frames per tab so the second one re-uses
    /// the ids the first allocated.
    #[test]
    fn the_dock_draws_every_tab_against_a_real_context() {
        let ctx = egui::Context::default();
        let mut dock = Surfaces::default();
        let mut paper = PaperTrading::new();
        for tab in DockTab::ALL {
            dock.dock.open_tab(tab);
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let response = draw_registered(
                        &mut dock,
                        ctx,
                        &mut PaperPanelEnv {
                            alerts_open: false,
                            replay: None,
                            paper: &mut paper,
                            timezone: TzOffset::default(),
                        },
                    );
                    assert!(!response.close_replay);
                });
            }
        }
        // Hidden dock: drawing is a no-op that asks nothing of the app.
        dock.dock.toggle_visible();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let response = draw_registered(
                &mut dock,
                ctx,
                &mut PaperPanelEnv {
                    alerts_open: false,
                    replay: None,
                    paper: &mut paper,
                    timezone: TzOffset::default(),
                },
            );
            assert!(!response.close_replay);
        });
    }

    /// The strip wears a badge exactly while a position is open — the one
    /// signal that survives the dock being collapsed.
    #[test]
    fn an_open_position_badges_the_trading_strip_icon() {
        let ctx = egui::Context::default();
        let mut dock = Surfaces::default();
        let mut paper = PaperTrading::new();

        let circles = |dock: &mut Surfaces, paper: &mut PaperTrading| {
            let mut count = 0;
            for _ in 0..2 {
                count = 0;
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    let _ = draw_registered(
                        dock,
                        ctx,
                        &mut PaperPanelEnv {
                            alerts_open: false,
                            replay: None,
                            paper,
                            timezone: TzOffset::default(),
                        },
                    );
                });
                for shape in output.shapes {
                    if matches!(shape.shape, egui::epaint::Shape::Circle(_)) {
                        count += 1;
                    }
                }
            }
            count
        };

        let flat = circles(&mut dock, &mut paper);
        let print = |agg_id: u64| quantick_engine::Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
            price: rust_decimal::Decimal::from(100),
            quantity: rust_decimal::Decimal::ONE,
            side: Side::Buy,
        };
        paper.seed(&print(0));
        paper.market(Side::Buy);
        paper.on_trade(&print(1));
        assert!(paper.position_summary().is_some(), "the buy filled");
        let open = circles(&mut dock, &mut paper);
        assert!(
            open > flat,
            "an open position must add the badge circle: {open} vs {flat}"
        );
    }

    fn draw_registered(
        registry: &mut Surfaces,
        ctx: &egui::Context,
        env: &mut PaperPanelEnv<'_>,
    ) -> PaperPanelEffects {
        let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
            alerts_open: false,
            paper: env.paper,
            replay: env.replay,
            timezone: env.timezone,
        });
        registry
            .draw_slot_unscoped_with_env(
                Slot::LateEdgePanels,
                &mut ViewContext { ctx, ui: None },
                Some(&mut env),
            )
            .paper_panel
            .map(|paper| *paper)
            .unwrap_or_default()
    }

    fn panel_frame(
        registry: &mut Surfaces,
        paper: &mut PaperTrading,
        ctx: &egui::Context,
        viewport: egui::ViewportId,
        events: Vec<egui::Event>,
    ) -> (SurfaceResponse, egui::FullOutput) {
        let mut response = SurfaceResponse::default();
        let output = ctx.run(panel_input(viewport, events), |ctx| {
            let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
                alerts_open: false,
                paper,
                replay: None,
                timezone: Default::default(),
            });
            response = registry.draw_slot_unscoped_with_env(
                Slot::LateEdgePanels,
                &mut ViewContext { ctx, ui: None },
                Some(&mut env),
            );
            egui::CentralPanel::default().show(ctx, |_| {});
        });
        (response, output)
    }

    fn text_position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(shape) if shape.galley.text().contains(text) => {
                    Some(shape.pos + shape.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the real panel painted {text:?}"))
    }

    fn click_panel(
        registry: &mut Surfaces,
        paper: &mut PaperTrading,
        ctx: &egui::Context,
        position: egui::Pos2,
    ) -> (SurfaceResponse, egui::FullOutput) {
        let event = |pressed| egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        panel_frame(
            registry,
            paper,
            ctx,
            egui::ViewportId::ROOT,
            vec![egui::Event::PointerMoved(position), event(true)],
        );
        panel_frame(
            registry,
            paper,
            ctx,
            egui::ViewportId::ROOT,
            vec![egui::Event::PointerMoved(position), event(false)],
        )
    }

    #[test]
    pub(crate) fn dock_borrowed_pass_preserves_strip_body_and_host_effect_order() {
        use crate::surfaces::replay_browser::ReplayBrowserExt as _;
        let ctx = egui::Context::default();
        let mut registry = Surfaces::default();
        let mut paper = PaperTrading::new();
        // Settle the real strip before hitting its painted glyph.
        panel_frame(
            &mut registry,
            &mut paper,
            &ctx,
            egui::ViewportId::ROOT,
            vec![],
        );
        let (_, output) = panel_frame(
            &mut registry,
            &mut paper,
            &ctx,
            egui::ViewportId::ROOT,
            vec![],
        );
        let session = text_position(&output, tab_icon(DockTab::Session));
        let (_, output) = click_panel(&mut registry, &mut paper, &ctx, session);
        assert_eq!(registry.dock.tab(), Some(DockTab::Session));
        let browser = text_position(&output, "Open session browser");
        let id = egui::Id::new((
            egui::ViewportId::ROOT,
            "dock_body",
            DockTab::Session.index(),
        ));
        let rect = egui::containers::panel::PanelState::load(&ctx, id)
            .unwrap()
            .rect;
        assert_eq!(
            registry.dock.model_state.width(DockTab::Session),
            rect.width()
        );
        assert!(!registry.replay_browser.browser_is_open());
        let (response, _) = click_panel(&mut registry, &mut paper, &ctx, browser);
        assert!(response.paper_panel.unwrap().open_replay_browser);
        assert!(
            !registry.replay_browser.browser_is_open(),
            "host execution waits for the full pass"
        );
        let (_, output) = click_panel(&mut registry, &mut paper, &ctx, session);
        assert_eq!(registry.dock.tab(), None, "the active strip tab collapses");
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text().contains("Open session browser"))));
        registry.dock.open_tab(DockTab::Trades);
        panel_frame(
            &mut registry,
            &mut paper,
            &ctx,
            egui::ViewportId::ROOT,
            vec![],
        );
        let (_, output) = panel_frame(
            &mut registry,
            &mut paper,
            &ctx,
            egui::ViewportId::ROOT,
            vec![],
        );
        let ticket = text_position(&output, "Open the ticket");
        click_panel(&mut registry, &mut paper, &ctx, ticket);
        assert_eq!(
            registry.dock.tab(),
            Some(DockTab::Trading),
            "the empty ledger opens the ticket after remembering its own width"
        );
    }

    #[test]
    pub(crate) fn dock_registration_retains_viewport_hit_regions() {
        let ctx = egui::Context::default();
        let mut registry = Surfaces::default();
        let mut paper = PaperTrading::new();
        for viewport in [
            egui::ViewportId::ROOT,
            egui::ViewportId::from_hash_of("dock-second"),
        ] {
            for tab in DockTab::ALL {
                registry.dock.open_tab(tab);
                for _ in 0..2 {
                    panel_frame(&mut registry, &mut paper, &ctx, viewport, vec![]);
                }
                let strip = egui::containers::panel::PanelState::load(
                    &ctx,
                    egui::Id::new((viewport, "dock_strip")),
                )
                .unwrap()
                .rect;
                let body = egui::containers::panel::PanelState::load(
                    &ctx,
                    egui::Id::new((viewport, "dock_body", tab.index())),
                )
                .unwrap()
                .rect;
                assert_eq!(strip.width(), TAB_STRIP_WIDTH);
                assert_eq!(body.right(), strip.left());
                for (position, expected) in [
                    (strip.center(), true),
                    (body.center(), true),
                    (egui::pos2(10.0, 400.0), false),
                ] {
                    let _ = ctx.run(
                        panel_input(viewport, vec![egui::Event::PointerMoved(position)]),
                        |ctx| {
                            assert_eq!(holds_pointer(&registry.dock.model_state, ctx), expected);
                        },
                    );
                }
                registry.dock.toggle_visible();
                let _ = ctx.run(
                    panel_input(viewport, vec![egui::Event::PointerMoved(body.center())]),
                    |ctx| {
                        assert!(
                            !holds_pointer(&registry.dock.model_state, ctx),
                            "hidden panels never claim Delete"
                        );
                    },
                );
            }
        }
    }

    fn panel_input(viewport: egui::ViewportId, events: Vec<egui::Event>) -> egui::RawInput {
        let mut input = egui::RawInput {
            viewport_id: viewport,
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        input.viewports.entry(viewport).or_default();
        input
    }

    #[test]
    fn hidden_dock_drains_a_real_chart_pnl_flip_once() {
        let mut paper = PaperTrading::new();
        let print = |agg_id: u64| quantick_engine::Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).unwrap() * 1000,
            price: rust_decimal::Decimal::from(100),
            quantity: rust_decimal::Decimal::ONE,
            side: Side::Buy,
        };
        paper.seed(&print(0));
        paper.market(Side::Buy);
        paper.on_trade(&print(1));
        assert!(paper.position_summary().is_some());
        let before = paper.account().pnl_primary();
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let scale = crate::chart::PriceScale::from_range(90.0, 110.0, 0.0, 400.0);
        for pressed in [true, false] {
            paper.handle_chart_input(&crate::paper_trading::ChartInput {
                chart,
                aim_area: chart,
                scale: Some(&scale),
                pointer: Some(chart.center()),
                primary_pressed: pressed,
                primary_down: pressed,
                primary_released: !pressed,
                modifiers: Default::default(),
                canvas_claimed: false,
                on_chrome: false,
                scroll_y: 0.0,
                middle_pressed: false,
                layer_visible: true,
            });
        }
        assert_ne!(
            paper.account().pnl_primary(),
            before,
            "the real chart gesture flipped the unit"
        );
        let ctx = egui::Context::default();
        let mut registry = Surfaces::default();
        registry.dock.restore(false, Some(DockTab::Trading));
        for expected in [true, false] {
            let (response, _) = panel_frame(
                &mut registry,
                &mut paper,
                &ctx,
                egui::ViewportId::ROOT,
                vec![],
            );
            assert_eq!(
                response
                    .paper_panel
                    .map(|paper| *paper)
                    .unwrap_or_default()
                    .risk_settings_changed,
                expected
            );
            assert_eq!(registry.dock.tab(), Some(DockTab::Trading));
            assert!(!registry.dock.visible());
        }
    }
}

/// C35: the actual catalog row borrows each account's sole report adapter.
#[test]
fn paper_report_composition_preserves_account_lifetime_and_tail_registration() {
    use super::*;
    use crate::paper_trading::PaperTrading;
    use crate::surfaces::Surfaces;
    use quantick_ui_model::paper_report::{Intent as ReportIntent, LedgerIntent};
    let ctx = egui::Context::default();
    let mut registry = Surfaces::default();
    let mut first = PaperTrading::new();
    let mut second = PaperTrading::new();
    let first_dir = crate::scratch::ScratchDir::new("paper-report-composed-first");
    let second_dir = crate::scratch::ScratchDir::new("paper-report-composed-second");
    for (paper, dir, symbol) in [
        (&mut first, &first_dir, "FIRST"),
        (&mut second, &second_dir, "SECOND"),
    ] {
        paper.redirect_history_dir(dir.path().to_path_buf());
        paper.set_symbol(symbol);
        let text =
            quantick_sim::history::write_header(symbol, quantick_sim::history::SessionSource::Live)
                + "1000,2000,long,1,100,105,5,manual,,,,\n";
        dir.write(format!("{symbol}/saved.csv"), &text);
        let (report, env) = paper.report_parts();
        report.open(&env);
    }
    first
        .report_state_mut()
        .update(ReportIntent::Ledger(LedgerIntent::Pages(3)));
    second
        .report_state_mut()
        .update(ReportIntent::ListOpen(false));
    registry.dock.restore(false, None);
    for paper in [&mut first, &mut second] {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
                alerts_open: false,
                paper,
                replay: None,
                timezone: Default::default(),
            });
            assert_eq!(
                registry.draw_slot_unscoped_with_env(
                    Slot::FrameTailPaperPanels,
                    &mut ViewContext { ctx, ui: None },
                    Some(&mut env)
                ),
                SurfaceResponse::default()
            );
        });
    }
    assert_eq!(
        first.report_state().snapshot().unwrap().symbol,
        Some("FIRST")
    );
    assert_eq!(
        second.report_state().snapshot().unwrap().symbol,
        Some("SECOND")
    );
    assert_eq!(first.report_state().snapshot().unwrap().rows.len(), 1);
    assert_eq!(second.report_state().snapshot().unwrap().rows.len(), 1);
    assert_eq!(first.report_state().ledger_pages(), 3);
    assert_eq!(second.report_state().ledger_pages(), 1);
    assert!(first.report_state().report_list_open());
    assert!(!second.report_state().report_list_open());
    first.report_state_mut().update(ReportIntent::Close);
    registry
        .paper_tail_probe
        .update(quantick_ui_model::paper_tail_probe::Intent::Arm {
            opened: -20_000,
            closed: -10_000,
            mixed: false,
        });
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        // An absent domain leaves the independent row armed.
        assert_eq!(
            registry.draw_slot_unscoped_with_env(
                Slot::FrameTailPaperPanels,
                &mut ViewContext { ctx, ui: None },
                None
            ),
            SurfaceResponse::default()
        );
        let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
            alerts_open: false,
            paper: &mut first,
            replay: None,
            timezone: Default::default(),
        });
        let response = registry.draw_slot_unscoped_with_env(
            Slot::FrameTailPaperPanels,
            &mut ViewContext { ctx, ui: None },
            Some(&mut env),
        );
        assert_eq!(
            response.paper_panel.unwrap().navigate_to_trade,
            Some((-20_000, -10_000))
        );
    });
    assert_eq!(
        registry.paper_tail_probe.model_state.report_open,
        Some(false)
    );
    assert!(
        second.report_state().is_open(),
        "closing one account never closes another"
    );
}
