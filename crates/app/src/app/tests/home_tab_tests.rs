//! Home driven by real input: the strip's first tab and its shortcuts.
use super::*;
use quantick_control_handlers::home_tab::HomeTabHost as _;

const WINDOW: egui::Vec2 = egui::vec2(1400.0, 760.0);
const CTRL_SHIFT: egui::Modifiers = egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT);

fn frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    run_frame_sized(app, ctx, WINDOW, Vec::new(), egui::Modifiers::NONE)
}

fn press(app: &mut QuantickApp, ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) {
    run_frame_sized(
        app,
        ctx,
        WINDOW,
        vec![key_press_with(key, modifiers)],
        modifiers,
    );
    frame(app, ctx);
}

fn on_home(app: &QuantickApp) -> bool {
    app.chrome.title_bar.surface.is_home()
}

fn home_rect(app: &QuantickApp) -> egui::Rect {
    app.chrome
        .title_bar
        .strip
        .home
        .expect("the Home tab is painted")
}

fn chip_rect(app: &QuantickApp, index: usize) -> egui::Rect {
    let tabs = &app.chrome.title_bar.strip.tabs;
    tabs.iter()
        .find(|(at, _)| *at == index)
        .expect("chip is painted")
        .1
}

fn preset_layout(index: usize) -> CanvasLayout {
    CanvasLayout::from_preset(&crate::canvas_layout::LAYOUT_PRESETS[index]).unwrap()
}

fn two_tabs(app: &mut QuantickApp, ctx: &egui::Context) -> TabEnds {
    let second = open_second_tab(app, ctx, "ETHUSDT");
    frame(app, ctx);
    second
}

/// The first painted image, which is the Home tab's logo.
fn logo_mesh(output: &egui::FullOutput) -> &egui::Mesh {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => Some(mesh),
            _ => None,
        })
        .expect("the logo is painted")
}

fn logo_tint(output: &egui::FullOutput) -> egui::Color32 {
    logo_mesh(output).vertices[0].color
}

/// The painted galley whose whole text is `wanted`.
fn painted_text<'a>(output: &'a egui::FullOutput, wanted: &str) -> &'a egui::epaint::TextShape {
    fn walk<'a>(shape: &'a egui::Shape, wanted: &str) -> Option<&'a egui::epaint::TextShape> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => Some(text),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| walk(shape, wanted)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|clipped| walk(&clipped.shape, wanted))
        .unwrap_or_else(|| panic!("{wanted:?} is painted"))
}

/// The wordmark's two halves' colours: (`Quan`, `tick`).
fn wordmark_colors(output: &egui::FullOutput) -> (egui::Color32, egui::Color32) {
    (
        painted_text(output, "Quan").fallback_color,
        painted_text(output, "tick").fallback_color,
    )
}

#[test]
fn both_shortcuts_go_home_and_a_preset_does_not() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    press(&mut app, &ctx, egui::Key::Num1, egui::Modifiers::CTRL);
    assert!(on_home(&app), "Ctrl+1");
    app.chrome.title_bar.surface.show_market();
    press(&mut app, &ctx, egui::Key::H, CTRL_SHIFT);
    assert!(on_home(&app), "Ctrl+Shift+H");
    app.chrome.title_bar.surface.show_market();

    let before = app.active_tab().layout;
    let second = preset_layout(1);
    assert_ne!(before, second);
    press(&mut app, &ctx, egui::Key::Num2, egui::Modifiers::CTRL);
    assert_eq!(app.active_tab().layout, before, "Ctrl+2 left the presets");
    assert!(!on_home(&app));
    press(&mut app, &ctx, egui::Key::Num2, CTRL_SHIFT);
    frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().layout,
        second,
        "Ctrl+Shift+2 is the preset"
    );
    assert!(!on_home(&app));

    let first = preset_layout(0);
    press(&mut app, &ctx, egui::Key::Num1, CTRL_SHIFT);
    frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().layout,
        first,
        "Ctrl+Shift+1 is the first preset"
    );
    assert!(!on_home(&app), "Ctrl+Shift+1 does not also fire Ctrl+1");
}

#[test]
fn close_commands_close_nothing_on_home_and_ctrl_tab_returns_to_the_same_market() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = two_tabs(&mut app, &ctx);
    let active = app.runtime.tabs.active_id();
    let at = home_rect(&app).center();
    click_sized(&mut app, &ctx, WINDOW, at);
    assert!(on_home(&app));

    press(&mut app, &ctx, egui::Key::W, egui::Modifiers::CTRL);
    assert_eq!(app.runtime.tabs.len(), 2, "Ctrl+W closes nothing on Home");
    assert!(on_home(&app));

    press(&mut app, &ctx, egui::Key::Tab, egui::Modifiers::CTRL);
    assert!(!on_home(&app));
    assert_eq!(
        app.runtime.tabs.active_id(),
        active,
        "Ctrl+Tab lands where it left"
    );
    press(&mut app, &ctx, egui::Key::Num1, egui::Modifiers::CTRL);
    press(&mut app, &ctx, egui::Key::Tab, CTRL_SHIFT);
    assert!(!on_home(&app));
    assert_eq!(app.runtime.tabs.active_id(), active);
}

#[test]
fn the_home_tab_has_no_close_target_and_a_click_inside_it_only_shows_home() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = two_tabs(&mut app, &ctx);
    let home = home_rect(&app);
    assert!(
        home.right() <= chip_rect(&app, 0).left(),
        "Home is first and overlaps no market"
    );
    for x in [home.left() + 2.0, home.center().x, home.right() - 2.0] {
        click_sized(&mut app, &ctx, WINDOW, egui::pos2(x, home.center().y));
        assert!(on_home(&app));
        assert_eq!(app.runtime.tabs.len(), 2);
        app.chrome.title_bar.surface.show_market();
    }
}

#[test]
fn hover_lifts_quan_only_and_the_logo_is_never_dimmed() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    let far = egui::pos2(1000.0, 400.0);
    let at = home_rect(&app).center();
    let idle = run_frame_sized(
        &mut app,
        &ctx,
        WINDOW,
        vec![egui::Event::PointerMoved(far)],
        egui::Modifiers::NONE,
    );
    let hovered = run_frame_sized(
        &mut app,
        &ctx,
        WINDOW,
        vec![egui::Event::PointerMoved(at)],
        egui::Modifiers::NONE,
    );
    click_sized(&mut app, &ctx, WINDOW, at);
    let active = frame(&mut app, &ctx);
    for (output, state) in [(&idle, "idle"), (&hovered, "hovered"), (&active, "active")] {
        assert_eq!(logo_tint(output), egui::Color32::WHITE, "{state}");
    }
    let brand = crate::theme::BRAND;
    assert_eq!(wordmark_colors(&idle), (crate::theme::TEXT_PRIMARY, brand));
    assert_eq!(wordmark_colors(&hovered), (egui::Color32::WHITE, brand));
    assert_eq!(wordmark_colors(&active), (egui::Color32::WHITE, brand));
}

#[test]
fn the_wordmark_sits_10_px_in_with_10_px_to_the_logo_and_14_px_after() {
    use crate::tabstrip::{
        HOME_LOGO_GAP_PX, HOME_PADDING_LEFT_PX, HOME_PADDING_RIGHT_PX, STRIP_EDGE_PX,
    };
    assert_eq!(
        (
            STRIP_EDGE_PX,
            HOME_PADDING_LEFT_PX,
            HOME_LOGO_GAP_PX,
            HOME_PADDING_RIGHT_PX
        ),
        (10.0, 10.0, 10.0, 14.0)
    );
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    let output = frame(&mut app, &ctx);
    let home = home_rect(&app);
    let logo = logo_mesh(&output).calc_bounds();
    let quan = painted_text(&output, "Quan");
    let tick = painted_text(&output, "tick");
    assert_eq!(home.left(), STRIP_EDGE_PX, "window edge to tab");
    assert_eq!(
        logo.left() - home.left(),
        HOME_PADDING_LEFT_PX,
        "tab to logo"
    );
    assert_eq!(quan.pos.x - logo.right(), HOME_LOGO_GAP_PX, "logo to text");
    assert_eq!(tick.pos.x, quan.pos.x + quan.galley.size().x, "one word");
    let after = home.right() - (tick.pos.x + tick.galley.size().x);
    assert!(
        (after - HOME_PADDING_RIGHT_PX).abs() < 0.01,
        "after the text: {after}"
    );
}

#[test]
fn exactly_one_tab_is_active_and_a_chip_click_returns_to_it() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = two_tabs(&mut app, &ctx);
    let selected = |app: &QuantickApp| {
        let scene = observer_scene(app);
        scene_control_ids(&scene)
            .into_iter()
            .filter(|id| id.starts_with("tab_strip.tab.") || id == "tab_strip.home")
            .filter(|id| scene_control(&scene, id)["selected"] == true)
            .collect::<Vec<_>>()
    };
    let active = app.runtime.tabs.active_id();
    assert_eq!(selected(&app), [format!("tab_strip.tab.{active}")]);

    let at = home_rect(&app).center();
    click_sized(&mut app, &ctx, WINDOW, at);
    frame(&mut app, &ctx);
    assert_eq!(selected(&app), ["tab_strip.home"]);

    // The chip of the market behind Home: selected underneath, yet it must
    // answer a click.
    let index = app.runtime.tabs.active_index();
    let at = chip_rect(&app, index).center();
    click_sized(&mut app, &ctx, WINDOW, at);
    assert!(!on_home(&app));
    assert_eq!(app.runtime.tabs.active_id(), active);
    frame(&mut app, &ctx);
    assert_eq!(selected(&app), [format!("tab_strip.tab.{active}")]);

    let at = home_rect(&app).center();
    click_sized(&mut app, &ctx, WINDOW, at);
    frame(&mut app, &ctx);
    let at = chip_rect(&app, 0).center();
    click_sized(&mut app, &ctx, WINDOW, at);
    assert!(!on_home(&app));
    assert_eq!(app.runtime.tabs.active_index(), 0, "another chip works too");
}

#[test]
fn closing_a_market_chip_from_home_stays_on_home() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = two_tabs(&mut app, &ctx);
    app.show_home_tab().unwrap();
    frame(&mut app, &ctx);
    let active = app.runtime.tabs.active_index();
    app.apply_tab_action(TabAction::Close(active));
    frame(&mut app, &ctx);
    assert_eq!(app.runtime.tabs.len(), 1);
    assert!(on_home(&app));
}

#[test]
fn opening_a_market_or_a_new_route_leaves_home_and_the_picker_opens_from_the_page() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    app.show_home_tab().unwrap();
    let output = frame(&mut app, &ctx);
    let open = painted_text_center(&output, "Open a market").expect("the page offers a market");
    assert!(painted_text_center(&output, "Market Shelf not built yet").is_some());
    click_sized(&mut app, &ctx, WINDOW, open);
    assert!(app.surfaces.market_palette.is_open());
    assert!(on_home(&app), "the picker alone does not leave Home");

    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    frame(&mut app, &ctx);
    assert!(!on_home(&app), "a new market comes forward");
}

#[test]
fn home_runs_feeds_stands_chart_keys_down_and_restores_the_same_canvas() {
    let (mut app, events, _, _) = test_app();
    let ctx = egui::Context::default();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    app.active_tab_mut().split_fraction = 0.35;
    frame(&mut app, &ctx);
    frame(&mut app, &ctx);
    let width = app.active_tab().last_canvas_width();
    let layout = app.active_tab().layout;
    let live = app.active_tab().live_trades;

    app.show_home_tab().unwrap();
    for index in 1..=3 {
        events.try_send(FeedEvent::Live(trade(index))).unwrap();
        frame(&mut app, &ctx);
    }
    assert_eq!(
        app.active_tab().live_trades,
        live + 3,
        "the feed keeps draining"
    );

    // Keys that would change or trade an unseen chart stand down.
    press(&mut app, &ctx, egui::Key::B, egui::Modifiers::SHIFT);
    assert!(app.active_tab().paper.is_flat(), "no order from Home");
    press(&mut app, &ctx, egui::Key::Num3, CTRL_SHIFT);
    frame(&mut app, &ctx);
    assert_eq!(app.active_tab().layout, layout);

    // The scene lists no pane, the cursor resolves to none, and a viewport
    // read says the geometry is unavailable.
    let scene = observer_scene(&app);
    assert_eq!(scene["active_surface"], "home");
    assert!(scene["focused_pane_id"].is_null());
    let ids = scene_control_ids(&scene);
    assert!(!ids.iter().any(|id| id.starts_with("pane.")), "{ids:?}");
    assert!(ids.iter().any(|id| id == "tab_strip.home"));
    let tab = app.active_tab();
    assert!(!crate::control::chart::viewport_snapshot(&tab.flow_pane).geometry_available);
    assert_eq!(tab.split_fraction, 0.35, "Home writes no split");
    assert_eq!(tab.last_canvas_width(), width, "nor a canvas size");

    press(&mut app, &ctx, egui::Key::Tab, egui::Modifiers::CTRL);
    frame(&mut app, &ctx);
    let tab = app.active_tab();
    assert_eq!(tab.split_fraction, 0.35);
    assert_eq!(
        tab.last_canvas_width(),
        width,
        "the canvas comes back at its size"
    );
    assert!(tab.flow_pane.frame.chart_area.is_some(), "and paints again");
    let scene = observer_scene(&app);
    assert_eq!(scene["active_surface"], "market");
    let flow = format!("pane.{}.canvas", app.active_tab().flow_pane.id);
    assert!(scene_control_ids(&scene).contains(&flow));
}

#[test]
fn nine_markets_at_1400_leave_only_the_logo_and_few_bring_the_label_back() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    let labelled = home_rect(&app).width();
    let output = frame(&mut app, &ctx);
    assert!(painted_text_center(&output, "Quan").is_some());
    assert!(labelled > crate::tabstrip::HOME_MIN_WIDTH_PX);

    let mut ends = Vec::new();
    for symbol in ["ETHUSDT"; 8] {
        ends.push(open_second_tab(&mut app, &ctx, symbol));
    }
    assert_eq!(app.runtime.tabs.len(), 9);
    let output = frame(&mut app, &ctx);
    let output_again = frame(&mut app, &ctx);
    assert_eq!(home_rect(&app).width(), crate::tabstrip::HOME_MIN_WIDTH_PX);
    assert!(painted_text_center(&output, "Quan").is_none());
    assert!(painted_text_center(&output_again, "Quan").is_none());
    assert!(app.chrome.title_bar.strip.overflow.is_some());

    // Still the same command, centred, and it closes nothing.
    let at = home_rect(&app).center();
    click_sized(&mut app, &ctx, WINDOW, at);
    assert!(on_home(&app));
    assert_eq!(app.runtime.tabs.len(), 9);
    app.chrome.title_bar.surface.show_market();

    // Closing markets, and a wider window, recompute it.
    run_frame_sized(
        &mut app,
        &ctx,
        egui::vec2(2400.0, 760.0),
        Vec::new(),
        egui::Modifiers::NONE,
    );
    assert!(home_rect(&app).width() > crate::tabstrip::HOME_MIN_WIDTH_PX);
    frame(&mut app, &ctx);
    while app.runtime.tabs.len() > 2 {
        app.apply_tab_action(TabAction::Close(0));
    }
    frame(&mut app, &ctx);
    let output = frame(&mut app, &ctx);
    assert_eq!(home_rect(&app).width(), labelled);
    assert!(painted_text_center(&output, "Quan").is_some());
}

fn count_painted(output: &egui::FullOutput, wanted: &str) -> usize {
    fn walk(shape: &egui::Shape, wanted: &str) -> usize {
        match shape {
            egui::Shape::Text(text) => usize::from(text.galley.text() == wanted),
            egui::Shape::Vec(shapes) => shapes.iter().map(|shape| walk(shape, wanted)).sum(),
            _ => 0,
        }
    }
    let shapes = output.shapes.iter();
    shapes.map(|clipped| walk(&clipped.shape, wanted)).sum()
}

#[test]
fn only_the_active_market_shows_its_close_mark_at_rest() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let far = egui::Event::PointerMoved(egui::pos2(1000.0, 400.0));
    let at_rest = |app: &mut QuantickApp| {
        let output = run_frame_sized(app, &ctx, WINDOW, vec![far.clone()], egui::Modifiers::NONE);
        count_painted(&output, "\u{00d7}")
    };
    frame(&mut app, &ctx);
    assert_eq!(at_rest(&mut app), 1, "the last market, active, shows it");
    app.show_home_tab().unwrap();
    assert_eq!(at_rest(&mut app), 0, "no market is active on Home");
    app.chrome.title_bar.surface.show_market();

    let _second = two_tabs(&mut app, &ctx);
    assert_eq!(
        at_rest(&mut app),
        1,
        "an inactive chip waits for the pointer"
    );
    let inactive = chip_rect(&app, 0).center();
    let hovered = run_frame_sized(
        &mut app,
        &ctx,
        WINDOW,
        vec![egui::Event::PointerMoved(inactive)],
        egui::Modifiers::NONE,
    );
    assert_eq!(count_painted(&hovered, "\u{00d7}"), 2);
}

/// Each key press carries its own modifiers; the frame's may have moved on by
/// the time it is read.
#[test]
fn the_number_shortcut_reads_the_press_not_the_frame() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    let keys = |app: &mut QuantickApp, pressed, held| {
        let event = key_press_with(egui::Key::Num1, pressed);
        run_frame_sized(app, &ctx, WINDOW, vec![event], held);
        frame(app, &ctx);
    };

    // Shift already released as the frame reads it: still a preset.
    keys(&mut app, CTRL_SHIFT, egui::Modifiers::CTRL);
    assert!(!on_home(&app), "Ctrl+Shift+1 is never Home");

    // Shift went down after the press, in the same frame.
    keys(&mut app, egui::Modifiers::CTRL, CTRL_SHIFT);
    assert!(on_home(&app), "Ctrl+1 is not refused for a later Shift");
    app.chrome.title_bar.surface.show_market();

    keys(&mut app, egui::Modifiers::CTRL, egui::Modifiers::CTRL);
    assert!(on_home(&app));
    app.chrome.title_bar.surface.show_market();

    let alt_gr = egui::Modifiers::CTRL.plus(egui::Modifiers::ALT);
    keys(&mut app, alt_gr, egui::Modifiers::CTRL);
    assert!(!on_home(&app), "Ctrl+Alt+1 is AltGr typing");
}

/// egui's `wants_keyboard_input` is true for any focus, so this needs a focused
/// widget that takes no text.
#[test]
fn a_focused_button_blocks_closing_a_tab_but_not_going_home() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = two_tabs(&mut app, &ctx);
    let focus = || {
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("a focused button")));
    };

    focus();
    press(&mut app, &ctx, egui::Key::W, egui::Modifiers::CTRL);
    assert_eq!(app.runtime.tabs.len(), 2, "the Ctrl+W gate still holds");

    focus();
    press(&mut app, &ctx, egui::Key::Num1, egui::Modifiers::CTRL);
    assert!(on_home(&app), "Ctrl+1");
    app.chrome.title_bar.surface.show_market();

    focus();
    press(&mut app, &ctx, egui::Key::H, CTRL_SHIFT);
    assert!(on_home(&app), "Ctrl+Shift+H");

    focus();
    press(&mut app, &ctx, egui::Key::Tab, egui::Modifiers::CTRL);
    assert!(!on_home(&app), "and the way back is not trapped either");
}

#[test]
fn a_text_field_keeps_both_home_shortcuts() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    app.surfaces.market_palette.open("");
    frame(&mut app, &ctx);
    frame(&mut app, &ctx);
    assert!(ctx.memory(|memory| memory.focused().is_some()), "the field");

    press(&mut app, &ctx, egui::Key::Num1, egui::Modifiers::CTRL);
    assert!(!on_home(&app), "Ctrl+1 while typing");
    press(&mut app, &ctx, egui::Key::H, CTRL_SHIFT);
    assert!(!on_home(&app), "Ctrl+Shift+H while typing");
}

#[test]
fn home_draws_no_toolbar_and_the_canvas_returns_at_its_full_size() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    let output = frame(&mut app, &ctx);
    let buy = app.active_tab().paper.trade_bar("").buy.face;
    assert!(painted_text_center(&output, &buy).is_some(), "on a market");
    let area = app.active_tab().flow_pane.frame.chart_area;
    assert!(area.is_some());

    app.chrome.layout_picker_open = true;
    app.show_home_tab().unwrap();
    for _ in 0..3 {
        let output = frame(&mut app, &ctx);
        assert!(painted_text_center(&output, &buy).is_none(), "on Home");
    }
    assert!(!app.chrome.layout_picker_open, "the picker went with it");
    assert!(app.active_tab().paper.is_flat());

    app.chrome.title_bar.surface.show_market();
    let output = frame(&mut app, &ctx);
    assert!(painted_text_center(&output, &buy).is_some(), "and back");
    assert_eq!(
        app.active_tab().flow_pane.frame.chart_area,
        area,
        "the first frame back is the size the canvas left at"
    );
}

/// The dock's ticket trades the active market, so it cannot stay reachable
/// while Home hides that market; it comes back with the market.
#[test]
fn home_draws_no_dock_so_its_ticket_cannot_trade_the_hidden_market() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let title = crate::dock::DockTab::Trading.title();
    app.dock.open_tab(crate::dock::DockTab::Trading);
    frame(&mut app, &ctx);
    let output = frame(&mut app, &ctx);
    assert!(painted_text_center(&output, title).is_some(), "on a market");

    app.show_home_tab().unwrap();
    for _ in 0..2 {
        let output = frame(&mut app, &ctx);
        assert!(painted_text_center(&output, title).is_none(), "on Home");
    }
    assert!(app.dock.visible(), "hidden with Home, not closed");

    app.chrome.title_bar.surface.show_market();
    let output = frame(&mut app, &ctx);
    assert!(painted_text_center(&output, title).is_some(), "and back");
}

/// Closing a replay ends a session the trader cannot see from Home, exactly as
/// Close Tab would, so the menu entry stands down the same way.
#[test]
fn close_replay_is_refused_while_home_hides_the_chart() {
    use crate::app::menu_bar::MenuCommand;
    assert!(MenuCommand::CloseReplay.needs_chart());
}

/// The toolbar used to refresh the chip label; without it a source change must
/// still reach the strip and take Home down.
#[test]
fn a_source_change_behind_home_still_relabels_the_chip_and_leaves_home() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    app.show_home_tab().unwrap();
    frame(&mut app, &ctx);
    let before = app.active_tab().chip_label().to_owned();

    app.active_tab_mut().symbol = "ETHUSDT".to_owned();
    frame(&mut app, &ctx);
    frame(&mut app, &ctx);
    assert_ne!(app.active_tab().chip_label(), before);
    assert!(!on_home(&app));
}

#[test]
fn the_menu_seam_refuses_chart_commands_on_home_and_keeps_the_rest() {
    use super::super::menu_bar::MenuCommand;
    use crate::ui_state::PanelSection;
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = two_tabs(&mut app, &ctx);
    let preset = &crate::canvas_layout::LAYOUT_PRESETS[2];
    let layout = app.active_tab().layout;
    let legend = app.active_tab().focused_pane().legend_collapsed;
    let context = app.active_tab().context_collapsed;
    assert_ne!(layout, preset_layout(2));

    app.show_home_tab().unwrap();
    app.apply_menu_command(MenuCommand::ApplyLayoutPreset(preset));
    app.apply_menu_command(MenuCommand::ToggleLegend);
    app.apply_menu_command(MenuCommand::ToggleContextCharts);
    app.apply_menu_command(MenuCommand::PaperMarket(quantick_engine::Side::Buy));
    app.apply_menu_command(MenuCommand::OpenOrderFlowPanel(PanelSection::L2));
    frame(&mut app, &ctx);
    assert!(on_home(&app));
    assert_eq!(app.runtime.tabs.len(), 2);
    let tab = app.active_tab();
    assert_eq!(tab.layout, layout);
    assert_eq!(tab.focused_pane().legend_collapsed, legend);
    assert_eq!(tab.context_collapsed, context);
    assert!(tab.paper.is_flat());
    assert!(!tab.tape().settings_panel.is_open(), "as Ctrl+Shift+O");

    // What is not the chart's still works, and the window stays on Home.
    let dock = app.dock.visible();
    app.apply_menu_command(MenuCommand::ToggleDock);
    assert_ne!(app.dock.visible(), dock, "window commands");
    app.apply_menu_command(MenuCommand::OpenReplayBrowser);
    assert!(on_home(&app));
    app.apply_tab_action(TabAction::Activate(0));
    assert!(!on_home(&app), "tab commands");

    // The same commands do act on a market.
    app.apply_menu_command(MenuCommand::ApplyLayoutPreset(preset));
    assert_eq!(app.active_tab().layout, preset_layout(2));
}

#[test]
fn a_narrow_strip_gives_the_label_room_to_the_market_chip() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let at = |width: f32| egui::vec2(width, 760.0);
    let sized = |app: &mut QuantickApp, size: egui::Vec2| {
        run_frame_sized(app, &ctx, size, Vec::new(), egui::Modifiers::NONE)
    };
    sized(&mut app, at(400.0));
    let output = sized(&mut app, at(400.0));
    assert!(painted_text_center(&output, "Quan").is_none());
    assert!(
        chip_rect(&app, 0).width() >= 90.0,
        "the lone chip keeps room for its symbol: {:?}",
        chip_rect(&app, 0)
    );
    assert!(app.chrome.title_bar.strip.overflow.is_none());
    let narrow = at(300.0);
    let sized = |app: &mut QuantickApp| sized(app, narrow);

    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    sized(&mut app);
    let output = sized(&mut app);
    assert!(painted_text_center(&output, "Quan").is_none());
    assert_eq!(
        home_rect(&app).width(),
        crate::tabstrip::CHIP_HEIGHT,
        "a compact strip drops the margins around the logo"
    );
    assert!(app.chrome.title_bar.strip.overflow.is_some());
}

/// Across the width where two chips stop fitting, the label and the overflow
/// menu trade places in the same step.
#[test]
fn the_overflow_menu_and_the_labelled_home_never_show_together_or_both_go() {
    let (mut app, _, _, _) = test_app();
    let ctx = egui::Context::default();
    let _second = two_tabs(&mut app, &ctx);
    for width in (600..1100).step_by(10) {
        let size = egui::vec2(width as f32, 760.0);
        run_frame_sized(&mut app, &ctx, size, Vec::new(), egui::Modifiers::NONE);
        let output = run_frame_sized(&mut app, &ctx, size, Vec::new(), egui::Modifiers::NONE);
        let labelled = painted_text_center(&output, "Quan").is_some();
        let overflow = app.chrome.title_bar.strip.overflow.is_some();
        assert_ne!(labelled, overflow, "at {width} px");
    }
}

#[test]
fn the_logo_is_the_one_icon_only_scaled_at_every_zoom() {
    for zoom in [1.0, 1.5, 2.0] {
        let (mut app, _, _, _) = test_app();
        let ctx = egui::Context::default();
        ctx.set_zoom_factor(zoom);
        frame(&mut app, &ctx);
        let output = frame(&mut app, &ctx);
        let texture = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    Some(mesh.texture_id)
                }
                _ => None,
            })
            .expect("the logo is painted");
        let manager = ctx.tex_manager();
        let manager = manager.read();
        let size = manager.meta(texture).expect("the logo is loaded").size;
        assert_eq!(size, [256, 256], "assets/icon.png at {zoom}x");
    }
}
