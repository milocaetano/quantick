//! The on-chart indicator legend (audit S1; spec'd in
//! `docs/ux/ui-design-model.md` §9 and never built): one row per indicator
//! on the pane, pinned to the chart's top-left — colour dot, name, last
//! value, and the eye / gear / remove controls beside it. Double-clicking
//! the row — all of it, not only the name — opens settings, the TradingView
//! gesture the audit's pain 3 found nowhere to happen. The three buttons sit
//! outside that region rather than under it, so a click on the eye is never
//! also a click on the row.
//!
//! Error and stale rows say so *on the chart*, red and amber, with the full
//! message on hover. An errored indicator used to disappear from the render
//! with its message computed and never shown (B1) — against the project's
//! data-honesty rule; this surface is where that statement now lives.
//!
//! The legend collapses to a count puck, because the corner it sits in is the
//! corner a trader reads the newest bars in — and with a position open the
//! HUD is already there. What collapses is only what is said twice: an
//! overlay's value is on its own line against the price scale, a sub-pane's
//! is in that pane's header. What is said *once* never collapses — see
//! [`survives_collapse`], which is the invariant, not a promise.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_indicators::Rgba8;

use crate::chart;
use crate::indicator_render::{COLLAPSED_CHEVRON, EXPANDED_CHEVRON, PANE_DISCLOSURE_PX};
use crate::indicator_worker::SlotId;
use crate::indicators::IndicatorView;
use crate::theme;

/// Gap between the pane's corner and the legend, in pixels.
const LEGEND_MARGIN_PX: f32 = 8.0;
/// Diameter of a row's colour dot, in pixels.
const DOT_DIAMETER_PX: f32 = 8.0;
/// How much of a hidden row's dot colour survives — enough to still name
/// the plot, faded enough to read as off.
const HIDDEN_DOT_FADE: f32 = 0.4;
/// Height of one row, in pixels: a small button plus egui's button padding,
/// which is the tallest widget in the row.
const ROW_HEIGHT_PX: f32 = 20.0;
/// Vertical gap between two rows, matching the spacing the frame sets.
const ROW_SPACING_PX: f32 = 3.0;
/// The frame's own vertical padding, top and bottom.
const FRAME_PADDING_Y_PX: f32 = 5.0;
/// How far this legend drops below the position HUD when both claim the
/// chart's top-left corner. See [`hud_offset_px`].
const HUD_OFFSET_PX: f32 = 64.0;

/// What a legend row asked of the app this frame. The caller applies each
/// against the pane the legend was drawn for — never the focused pane, so a
/// row can never act on the chart beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegendAction {
    /// Flip the render-side eye (no recompute).
    ToggleHidden(SlotId),
    /// Open the settings dialog.
    OpenSettings(SlotId),
    /// Remove the indicator.
    Remove(SlotId),
    /// Fold the legend down to its count puck, or open it back up. Carries the
    /// state it wants rather than "toggle", so the hotkey, the harness hook
    /// and the chevron all name one outcome instead of three flips that can
    /// disagree about what they started from.
    SetCollapsed(bool),
}

/// How far down this legend starts when the position HUD claims the very
/// corner of *this* pane. Zero anywhere the HUD is not painting.
///
/// One owner for one number: the app places this legend with it, and the pane
/// stacks the order-flow key below the same corner with it. What it must be
/// asked is "does the HUD paint here", not "is there a position" — the HUD
/// rides the pane that owns order entry, which is the focused one
/// (`tab.rs`: `paper_owns_input = side == focused`, and the HUD draws from
/// `focused_pane().paper_hud_anchor()`). Ask the wrong question and a split
/// with a position open moves the overlap to the other pane rather than
/// removing it: chips that never dropped under a HUD that is right on top of
/// them, and 64 px reserved on the pane where no HUD is.
pub(crate) fn hud_offset_px(hud_paints_here: bool) -> f32 {
    if hud_paints_here { HUD_OFFSET_PX } else { 0.0 }
}

/// How much of the canvas's top-left corner this legend claims, measured
/// from the corner itself — its own margin included, zero when it draws
/// nothing.
///
/// Predicted rather than measured, so whoever stacks below it (the order-flow
/// key) lands correctly on the very first frame instead of printing over a
/// legend that had not been laid out yet. `the_predicted_stack_height_covers_
/// what_the_legend_actually_draws` keeps the prediction honest against the
/// real layout.
pub(crate) fn stack_height_px(views: &[IndicatorView], collapsed: bool) -> f32 {
    if views.is_empty() {
        return 0.0;
    }
    let rows = row_count(views, collapsed) as f32;
    LEGEND_MARGIN_PX
        + FRAME_PADDING_Y_PX * 2.0
        + rows * ROW_HEIGHT_PX
        + (rows - 1.0) * ROW_SPACING_PX
}

/// Whether a row keeps its place while the legend is collapsed.
///
/// The exception, and only the exception. A healthy row repeats what the
/// chart says elsewhere, so folding it costs the trader nothing. A row that
/// is errored or stale says something no other surface says — and B1 in this
/// module's docs is what an indicator disappearing with its message unread
/// already cost once.
///
/// It is written as a predicate over the view, not as a rule the drawing
/// remembers to apply, because both the painter and [`stack_height_px`] ask
/// it: there is no code path where the collapse holds power over a broken
/// indicator, which is a stronger statement than promising not to hide one.
pub(crate) fn survives_collapse(view: &IndicatorView) -> bool {
    view.error.is_some() || view.stale.is_some()
}

/// How many rows the legend draws — the count puck included, since it is a
/// row of the same height as any other.
fn row_count(views: &[IndicatorView], collapsed: bool) -> usize {
    if !collapsed {
        return views.len();
    }
    1 + views.iter().filter(|view| survives_collapse(view)).count()
}

/// How many indicators the collapse actually folded away — what the puck
/// counts. Zero when every indicator on the pane is unhealthy: they are all
/// still on screen, and a puck claiming otherwise would be counting rows it
/// did not hide.
fn folded_count(views: &[IndicatorView]) -> usize {
    views.iter().filter(|view| !survives_collapse(view)).count()
}

/// Draw the legend over `chart_rect`. A no-op with no indicators — an empty
/// legend frame would be chart chrome with nothing to say.
pub(crate) fn draw(
    ctx: &egui::Context,
    pane_id: u64,
    chart_rect: egui::Rect,
    views: &[IndicatorView],
    preview_slot: Option<SlotId>,
    collapsed: bool,
) -> Vec<LegendAction> {
    let mut actions = Vec::new();
    if views.is_empty() {
        return actions;
    }
    egui::Area::new(egui::Id::new(("indicator_legend", pane_id)))
        .fixed_pos(chart_rect.left_top() + egui::vec2(LEGEND_MARGIN_PX, LEGEND_MARGIN_PX))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            // The HUD's card grammar, quieter: sunken fill, hairline border,
            // no rail — the rows' own colour dots carry the identity.
            egui::Frame::none()
                .fill(theme::INSET)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);
                    if collapsed {
                        // The puck leads, then the rows the collapse has no
                        // power over. Preview rides the puck rather than
                        // unfolding a row: settings being previewed is a fact
                        // about the dialog in front of the trader, not a fault
                        // in the indicator — but it is still stated here,
                        // because the dialog may be sitting behind the chart.
                        draw_puck(ui, views, preview_slot.is_some(), &mut actions);
                        for view in views.iter().filter(|view| survives_collapse(view)) {
                            draw_row(ui, view, false, false, &mut actions);
                        }
                    } else {
                        for (index, view) in views.iter().enumerate() {
                            draw_row(
                                ui,
                                view,
                                preview_slot == Some(view.slot),
                                index == 0,
                                &mut actions,
                            );
                        }
                    }
                });
        });
    actions
}

/// The collapsed legend's one row: a disclosure that opens it, and the count
/// of the indicators it folded away.
///
/// The count is the puck's whole job — "is anything running here" is the
/// question a fold has to keep answering, and the hover names them so the
/// answer never costs a click. It reads nothing back that the rows below it
/// are already saying: unhealthy indicators are listed there in full, not
/// tallied here.
fn draw_puck(
    ui: &mut egui::Ui,
    views: &[IndicatorView],
    previewing: bool,
    actions: &mut Vec<LegendAction>,
) {
    ui.horizontal(|ui| {
        if chevron(ui, Some(COLLAPSED_CHEVRON))
            .is_some_and(|response| response.on_hover_text(EXPAND_HINT).clicked())
        {
            actions.push(LegendAction::SetCollapsed(false));
        }
        let folded = folded_count(views);
        if folded > 0 {
            ui.label(
                egui::RichText::new(folded.to_string())
                    .small()
                    .color(theme::TEXT_MUTED),
            )
            .on_hover_text(folded_summary(views));
        }
        if previewing {
            ui.label(egui::RichText::new("preview").small().color(theme::ACCENT))
                .on_hover_text("showing un-applied settings — Apply keeps, Discard reverts");
        }
    });
}

/// What the puck's hover says: one line per folded indicator, named and
/// valued, so "which four?" is answered without opening anything.
fn folded_summary(views: &[IndicatorView]) -> String {
    let mut lines = Vec::new();
    for view in views.iter().filter(|view| !survives_collapse(view)) {
        let value = (!view.hidden)
            .then(|| view.columns.first().and_then(|column| column.last()))
            .flatten();
        match value {
            Some(value) => {
                lines.push(format!(
                    "{}  {}",
                    view.label(),
                    chart::compact_value(*value)
                ));
            }
            // A hidden row has no value worth quoting — the same silence its
            // expanded row keeps, for the same reason.
            None => lines.push(view.label().to_owned()),
        }
    }
    lines.join("\n")
}

/// The disclosure column every row reserves, whether or not it draws one.
///
/// `Some(glyph)` draws the control and hands back its response; `None`
/// allocates the same width and draws nothing, which is what keeps the colour
/// dots of rows 2..n in the same column as the dot of the row that leads. The
/// square is [`PANE_DISCLOSURE_PX`] — the same target the collapsed sub-pane
/// offers, because a control you have to aim at is one a trader does not use
/// mid-tape.
fn chevron(ui: &mut egui::Ui, glyph: Option<&str>) -> Option<egui::Response> {
    let size = egui::vec2(PANE_DISCLOSURE_PX, PANE_DISCLOSURE_PX);
    let Some(glyph) = glyph else {
        ui.allocate_exact_size(size, egui::Sense::hover());
        return None;
    };
    Some(ui.add_sized(
        size,
        egui::Button::new(egui::RichText::new(glyph).small().color(theme::TEXT_MUTED)).frame(false),
    ))
}

/// What the chevron's hover says on both halves of the gesture. One string so
/// the two never drift into naming different keys.
const EXPAND_HINT: &str = "show every indicator (L)";
const COLLAPSE_HINT: &str = "collapse to a count — broken ones stay (L)";

/// One legend row. Status precedence matches the toolbar menu's dot:
/// errored beats stale beats hidden — a broken indicator must never read as
/// merely hidden.
///
/// `leads` marks the row that carries the open disclosure: the first one of an
/// expanded legend. It costs the expanded state no height at all — the
/// chevron rides *in* the row rather than above it, which is the whole reason
/// this legend folds without the state a trader spends most of the session in
/// growing a header.
fn draw_row(
    ui: &mut egui::Ui,
    view: &IndicatorView,
    previewing: bool,
    leads: bool,
    actions: &mut Vec<LegendAction>,
) {
    ui.horizontal(|ui| {
        if chevron(ui, leads.then_some(EXPANDED_CHEVRON))
            .is_some_and(|response| response.on_hover_text(COLLAPSE_HINT).clicked())
        {
            actions.push(LegendAction::SetCollapsed(true));
        }
        // Everything that *names* the indicator — dot, label, value, status,
        // preview and "N off" chips — laid out as one region so the whole of
        // it answers a double click. Only the text used to, which meant the
        // gesture worked on a word and did nothing on the pixel beside it. The
        // three buttons stay outside the region rather than under it, so a
        // click on the eye is never also a click on the row.
        let identity = ui
            .scope(|ui| {
                ui.horizontal(|ui| {
                    draw_status_dot(ui, view);
                    let name_color = if view.error.is_some() {
                        theme::SELL
                    } else if view.stale.is_some() {
                        theme::ACCENT
                    } else if view.hidden {
                        theme::TEXT_MUTED
                    } else {
                        theme::TEXT_PRIMARY
                    };
                    ui.label(egui::RichText::new(view.label()).color(name_color).small());
                    // The last committed value of the first plot — the number a trader
                    // reads an overlay by. Withheld while hidden or errored: a value
                    // from a line that is not on screen answers nothing.
                    if !view.hidden
                        && view.error.is_none()
                        && let Some(value) = view.columns.first().and_then(|column| column.last())
                    {
                        ui.label(
                            egui::RichText::new(chart::compact_value(*value))
                                .monospace()
                                .small()
                                .color(theme::TEXT_MUTED),
                        );
                    }
                    if let Some(error) = &view.error {
                        ui.label(egui::RichText::new("error").small().color(theme::SELL))
                            .on_hover_text(error.to_string());
                    } else if let Some(stale) = &view.stale {
                        ui.label(egui::RichText::new("stale").small().color(theme::ACCENT))
                            .on_hover_text(stale.clone());
                    } else if previewing {
                        // The chart is showing settings the state file does not hold —
                        // said here, on the chart, not only inside the dialog that may
                        // be sitting behind it (trader-ux review). Error and stale
                        // outrank it: a broken indicator is not merely previewing.
                        ui.label(egui::RichText::new("preview").small().color(theme::ACCENT))
                            .on_hover_text(
                                "showing un-applied settings — Apply keeps, Discard reverts",
                            );
                    }
                    // A layer switched off in settings must not read like a broken
                    // indicator: a Copilot with everything off draws nothing, and a row
                    // that looks healthy over an empty chart says "bug", not "as asked"
                    // (trader-ux review). Counts the `Display:`-titled bools that are
                    // off — the same convention the settings dialog groups by.
                    let layers_off = view
                        .descriptor
                        .inputs
                        .iter()
                        .zip(view.input_values.iter())
                        .filter(|(spec, value)| {
                            crate::indicator_panel::split_section(spec.title()).0
                                == crate::indicator_panel::DISPLAY_SECTION
                                && **value == quantick_indicators::InputValue::Bool(false)
                        })
                        .count();
                    if layers_off > 0 {
                        ui.label(
                            egui::RichText::new(format!("{layers_off} off"))
                                .small()
                                .color(theme::TEXT_MUTED),
                        )
                        .on_hover_text("display layers switched off in settings");
                    }
                });
            })
            .response;
        if ui
            .interact(
                identity.rect,
                ui.id().with(("legend-row", view.slot.0)),
                egui::Sense::click(),
            )
            .on_hover_text("double-click to open settings")
            .double_clicked()
        {
            actions.push(LegendAction::OpenSettings(view.slot));
        }
        if ui
            .small_button(if view.hidden {
                icons::EYE
            } else {
                icons::EYE_SLASH
            })
            .on_hover_text("hide/show without removing (no recompute)")
            .clicked()
        {
            actions.push(LegendAction::ToggleHidden(view.slot));
        }
        if ui
            .small_button(icons::GEAR)
            .on_hover_text("settings")
            .clicked()
        {
            actions.push(LegendAction::OpenSettings(view.slot));
        }
        if ui
            .small_button(icons::TRASH)
            .on_hover_text("remove this indicator")
            .clicked()
        {
            actions.push(LegendAction::Remove(view.slot));
        }
    });
}

/// The row's colour dot: the first plot's own colour, so the row and the
/// line it names can never disagree; error and stale wear the same glyphs
/// as the toolbar menu's status dot.
fn draw_status_dot(ui: &mut egui::Ui, view: &IndicatorView) {
    if view.error.is_some() {
        ui.label(
            egui::RichText::new(icons::WARNING_CIRCLE)
                .small()
                .color(theme::SELL),
        );
        return;
    }
    if view.stale.is_some() {
        ui.label(
            egui::RichText::new(icons::WARNING)
                .small()
                .color(theme::ACCENT),
        );
        return;
    }
    // Resolved, not declared: the dot exists so the row and the line it names
    // can never disagree, which a restyled plot would break instantly.
    let color = view
        .plot_style(0)
        .map_or(theme::TEXT_MUTED, |plot| rgba(plot.color));
    let color = if view.hidden {
        color.gamma_multiply(HIDDEN_DOT_FADE)
    } else {
        color
    };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(DOT_DIAMETER_PX, DOT_DIAMETER_PX),
        egui::Sense::hover(),
    );
    ui.painter()
        .circle_filled(rect.center(), DOT_DIAMETER_PX / 2.0, color);
}

fn rgba(color: Rgba8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator_worker::IndicatorEvent;
    use crate::indicators::IndicatorViews;
    use quantick_indicators::{EvalError, IndicatorDescriptor, PlotId, PlotSpec, PlotStyle};

    fn descriptor(title: &str) -> IndicatorDescriptor {
        IndicatorDescriptor {
            title: title.to_owned(),
            short_title: None,
            overlay: true,
            plots: vec![PlotSpec {
                id: PlotId::new(0),
                title: "p0".to_owned(),
                style: PlotStyle::Line,
                base_color: Rgba8::opaque(0x26, 0xA6, 0x9A),
                width: 1.0,
                offset: 0,
                marker: None,
            }],
            inputs: Vec::new(),
            fills: Vec::new(),
        }
    }

    fn views_with(build: impl FnOnce(&mut IndicatorViews, SlotId)) -> IndicatorViews {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        build(&mut views, slot);
        views
    }

    fn painted(ctx: &egui::Context, views: &IndicatorViews) -> String {
        painted_folded(ctx, views, false)
    }

    fn painted_folded(ctx: &egui::Context, views: &IndicatorViews, collapsed: bool) -> String {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let mut text = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let actions = draw(ctx, 0, chart, views.all(), None, collapsed);
                assert!(actions.is_empty(), "no clicks, no actions");
            });
            text.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = shape.shape {
                    text.push_str(galley.galley.text());
                    text.push(' ');
                }
            }
        }
        text
    }

    /// Whoever stacks under this legend — the order-flow key at the same
    /// corner — places itself from [`stack_height_px`] before the legend has
    /// ever been laid out. The prediction therefore has to cover what the
    /// legend really draws, at every row count, or the two print over each
    /// other on the frame an indicator is added.
    #[test]
    fn the_predicted_stack_height_covers_what_the_legend_actually_draws() {
        let ctx = egui::Context::default();
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        for collapsed in [false, true] {
            assert_eq!(
                stack_height_px(&[], collapsed),
                0.0,
                "nothing drawn, nothing claimed"
            );
        }

        // The matrix the fold opened up: rows, times how many of them are
        // unhealthy, times folded or not. A collapsed legend's height is no
        // longer a function of the row count alone — an errored indicator
        // keeps its row through the fold, so the prediction has to count the
        // survivors, and this is what catches it if it ever stops.
        for rows in 1..=3_usize {
            for sick in 0..=rows {
                let mut views = IndicatorViews::new();
                for row in 0..rows {
                    let slot = views.allocate_slot("test.indicator");
                    views.apply(IndicatorEvent::rebuilt(
                        slot,
                        descriptor(&format!("EMA({row}, close)")),
                        vec![vec![101.5, 1_234.0]],
                    ));
                    if row < sick {
                        views.apply(IndicatorEvent::Error {
                            slot,
                            error: EvalError {
                                bar_index: 7,
                                message: "broken".to_owned(),
                            },
                        });
                    }
                }
                for collapsed in [false, true] {
                    let mut bottom = f32::NEG_INFINITY;
                    for _ in 0..2 {
                        let output = ctx.run(egui::RawInput::default(), |ctx| {
                            draw(ctx, 0, chart, views.all(), None, collapsed);
                        });
                        bottom = f32::NEG_INFINITY;
                        for shape in output.shapes {
                            let rect = shape.shape.visual_bounding_rect();
                            if rect.is_positive() {
                                bottom = bottom.max(rect.bottom());
                            }
                        }
                    }
                    let claimed = chart.top() + stack_height_px(views.all(), collapsed);
                    assert!(
                        bottom <= claimed,
                        "{rows} row(s), {sick} broken, collapsed={collapsed}: \
                         the legend reaches {bottom}, the prediction claims {claimed}"
                    );
                }
            }
        }
    }

    /// The fold's whole promise, as a test rather than a comment: an errored
    /// indicator is still named, and still says "error", with the legend
    /// collapsed. B1 cost the project an indicator that vanished with its
    /// message computed and never shown; a fold that could re-hide one would
    /// be the same bug wearing a chevron.
    #[test]
    fn a_folded_legend_still_states_a_broken_indicator() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor("zigzag.pine"),
                vec![vec![1.0]],
            ));
            views.apply(IndicatorEvent::Error {
                slot,
                error: EvalError {
                    bar_index: 7,
                    message: "PINE_NO_SECURITY: security() is not supported".to_owned(),
                },
            });
        });
        let text = painted_folded(&ctx, &views, true);
        assert!(text.contains("zigzag.pine"), "folded: {text}");
        assert!(text.contains("error"), "folded: {text}");
    }

    /// Stale is the same promise: the chart is drawing an older version than
    /// the one on disk, and folding the legend must not be a way to stop
    /// saying so.
    #[test]
    fn a_folded_legend_still_states_a_stale_indicator() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor("cvd.pine"),
                columns: vec![vec![2.0]],
                bar_paint: Vec::new(),
                rows: 1,
                inputs: Vec::new(),
                stale: Some("edit has errors; running the previous version".to_owned()),
            });
        });
        let text = painted_folded(&ctx, &views, true);
        assert!(text.contains("cvd.pine"), "folded: {text}");
        assert!(text.contains("stale"), "folded: {text}");
    }

    /// What the fold *is* allowed to do: take the healthy rows off the chart
    /// and leave a count in their place. The puck counts what it folded, not
    /// what is on the pane — the broken row below it is still visible and
    /// counting it would be claiming to have hidden something that is right
    /// there.
    #[test]
    fn folding_hides_healthy_rows_and_counts_them() {
        let ctx = egui::Context::default();
        let mut views = IndicatorViews::new();
        for name in ["EMA(9, close)", "ATR(14)"] {
            let slot = views.allocate_slot("test.indicator");
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor(name),
                vec![vec![101.5, 1_234.0]],
            ));
        }
        let broken = views.allocate_slot("test.indicator");
        views.apply(IndicatorEvent::rebuilt(
            broken,
            descriptor("zigzag.pine"),
            vec![vec![1.0]],
        ));
        views.apply(IndicatorEvent::Error {
            slot: broken,
            error: EvalError {
                bar_index: 7,
                message: "broken".to_owned(),
            },
        });

        let folded = painted_folded(&ctx, &views, true);
        assert!(
            !folded.contains("EMA(9, close)") && !folded.contains("ATR(14)"),
            "the healthy rows fold away: {folded}"
        );
        assert!(
            folded.contains('2'),
            "the puck counts the two it hid: {folded}"
        );
        assert!(
            folded.contains("zigzag.pine"),
            "the broken one stays: {folded}"
        );

        let open = painted_folded(&ctx, &views, false);
        assert!(open.contains("EMA(9, close)"), "expanded: {open}");
        assert!(open.contains("ATR(14)"), "expanded: {open}");
    }

    /// Both halves of the gesture reach the app as one named outcome, so the
    /// chevron, the hotkey and the harness hook cannot disagree about what
    /// they toggled from.
    #[test]
    fn the_chevron_asks_to_fold_and_to_unfold() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor("EMA(9, close)"),
                vec![vec![101.5, 1_234.0]],
            ));
        });
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

        for (collapsed, want) in [(false, true), (true, false)] {
            // Aim at the glyph the legend really painted rather than at a
            // position computed from the paddings: the arithmetic would be a
            // second copy of the layout, and a test that agrees with its own
            // copy proves nothing about the one on screen.
            let glyph = if collapsed {
                COLLAPSED_CHEVRON
            } else {
                EXPANDED_CHEVRON
            };
            let mut target = None;
            for _ in 0..2 {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    draw(ctx, 0, chart, views.all(), None, collapsed);
                });
                for shape in output.shapes {
                    if let egui::epaint::Shape::Text(galley) = &shape.shape
                        && galley.galley.text() == glyph
                    {
                        target = Some(galley.visual_bounding_rect().center());
                    }
                }
            }
            let target = target.expect("the legend paints its disclosure");
            let mut actions = Vec::new();
            for pressed in [true, false] {
                let mut input = egui::RawInput::default();
                input.events.push(egui::Event::PointerMoved(target));
                input.events.push(egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                });
                let _ = ctx.run(input, |ctx| {
                    actions.extend(draw(ctx, 0, chart, views.all(), None, collapsed));
                });
            }
            assert!(
                actions.contains(&LegendAction::SetCollapsed(want)),
                "collapsed={collapsed}: the chevron must ask for {want}, got {actions:?}"
            );
        }
    }

    /// Criterion 1, the legend's target: the *row* opens settings, not just
    /// the word.
    ///
    /// The gesture used to be sensed on the label alone, so it worked on the
    /// text and failed on the value beside it and on the dot in front of it —
    /// a double click that lands a few pixels off and does nothing reads as a
    /// broken feature, not as a near miss. Driven at the last value, which is
    /// where the old implementation would have missed.
    #[test]
    fn double_clicking_anywhere_on_a_row_opens_its_settings() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor("EMA(9, close)"),
                vec![vec![101.5, 1_234.0]],
            ));
        });
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

        // Lay the legend out, then aim at the last value's glyphs: the
        // right-hand end of the identity region, well past the name.
        let mut target = None;
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                draw(ctx, 0, chart, views.all(), None, false);
            });
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = &shape.shape
                    && galley.galley.text().contains("1.23K")
                {
                    target = Some(galley.visual_bounding_rect().center());
                }
            }
        }
        let target = target.expect("the row paints its last value");

        let mut actions = Vec::new();
        for pressed in [true, false, true, false] {
            let mut input = egui::RawInput::default();
            input.events.push(egui::Event::PointerMoved(target));
            input.events.push(egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
            let _ = ctx.run(input, |ctx| {
                actions.extend(draw(ctx, 0, chart, views.all(), None, false));
            });
        }
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, LegendAction::OpenSettings(_))),
            "a double click on the row's value must open settings: {actions:?}"
        );
    }

    /// A healthy row reaches pixels: name and last value, in the axis'
    /// compact spelling.
    #[test]
    fn the_legend_names_the_indicator_and_its_last_value() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor("EMA(9, close)"),
                vec![vec![101.5, 1_234.0]],
            ));
        });
        let text = painted(&ctx, &views);
        assert!(text.contains("EMA(9, close)"), "painted: {text}");
        assert!(text.contains("1.23K"), "painted: {text}");
    }

    /// B1: an errored indicator states its error on the chart, message
    /// reachable — it must never simply disappear.
    #[test]
    fn an_errored_indicator_states_its_error_on_the_chart() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor("zigzag.pine"),
                vec![vec![1.0]],
            ));
            views.apply(IndicatorEvent::Error {
                slot,
                error: EvalError {
                    bar_index: 7,
                    message: "PINE_NO_SECURITY: security() is not supported".to_owned(),
                },
            });
        });
        let text = painted(&ctx, &views);
        assert!(text.contains("zigzag.pine"), "painted: {text}");
        assert!(text.contains("error"), "painted: {text}");
    }

    /// A stale row wears the amber word; a flat list draws nothing at all.
    #[test]
    fn stale_rows_say_stale_and_an_empty_list_paints_nothing() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor("cvd.pine"),
                columns: vec![vec![2.0]],
                bar_paint: Vec::new(),
                rows: 1,
                inputs: Vec::new(),
                stale: Some("edit has errors; running the previous version".to_owned()),
            });
        });
        let text = painted(&ctx, &views);
        assert!(text.contains("stale"), "painted: {text}");

        let empty = IndicatorViews::new();
        assert!(painted(&ctx, &empty).is_empty(), "no rows, no chrome");
    }
}
