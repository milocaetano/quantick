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
//! The legend collapses to a puck, because the corner it sits in is the
//! corner a trader reads the newest bars in — and with a position open the
//! HUD is already there. A sub-pane's indicator keeps its name and value in
//! that pane's own header, so folding its row costs nothing. An overlay's
//! does *not*: nothing tags an overlay's last value against the price scale
//! today, so what the fold takes from an overlay is its only readout. That is
//! why the puck carries the plot colours rather than a bare count — the
//! colour is what maps the line on the chart back to a name, and the hover
//! gives the name and the number without unfolding.
//!
//! What is said *once* by the legend and nowhere else never collapses at all
//! — see [`survives_collapse`], which is the invariant, not a promise.

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
/// How many folded indicators the puck names with their own colour dot before
/// it gives up and counts the rest.
///
/// Four fits the puck without turning it into the row it replaced. Past that
/// the dots stop being countable at a glance anyway, and the overflow reads
/// as `+N` — still never a bare integer, which beside the position HUD is a
/// number in the vocabulary of contracts.
const MAX_PUCK_DOTS: usize = 4;
/// How many folded indicators the puck's hover names before it says "+N more".
/// A tooltip taller than the chart it covers is not a way to read anything.
const MAX_HOVER_LINES: usize = 8;
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
/// (`tab.rs`: `paper_hud_here = side == focused`, and the HUD draws from
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
                        // The chip belongs to the puck only when the
                        // previewed indicator is one the puck actually hid.
                        // An errored row that is also previewing keeps its
                        // own line, and a puck announcing a preview beside a
                        // row that says `error` describes a state the pane is
                        // not in.
                        let previewing_folded = views.iter().any(|view| {
                            Some(view.slot) == preview_slot && !survives_collapse(view)
                        });
                        draw_puck(ui, views, previewing_folded, &mut actions);
                        for view in views.iter().filter(|view| survives_collapse(view)) {
                            // Its own preview, not the puck's: a row that
                            // kept its place through the fold states
                            // everything about itself.
                            draw_row(
                                ui,
                                view,
                                preview_slot == Some(view.slot),
                                false,
                                &mut actions,
                            );
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
        // The whole puck answers the click, not just the 18 px glyph inside
        // it. A control that looks bigger than it is teaches a trader to miss
        // it — the same reason `draw_row` senses its identity region rather
        // than its label. Laid out first, interacted with after, so the rect
        // is the one that really got drawn.
        let puck = ui
            .scope(|ui| {
                ui.horizontal(|ui| {
                    chevron(ui, Some(COLLAPSED_CHEVRON));
                    draw_folded_dots(ui, views);
                    if previewing {
                        ui.label(egui::RichText::new("preview").small().color(theme::ACCENT))
                            .on_hover_text(
                                "showing un-applied settings — Apply keeps, Discard reverts",
                            );
                    }
                });
            })
            .response;
        if ui
            .interact(puck.rect, ui.id().with("legend-puck"), egui::Sense::click())
            // Lazy: the summary is a string per folded indicator plus a join,
            // and the eager form would build all of it every frame the legend
            // is shut, for a tooltip shown on almost none of them. This is the
            // render path.
            .on_hover_ui(|ui| {
                ui.label(EXPAND_HINT);
                let summary = folded_summary(views);
                if !summary.is_empty() {
                    ui.label(summary);
                }
            })
            .clicked()
        {
            actions.push(LegendAction::SetCollapsed(false));
        }
    });
}

/// The folded indicators, as their own plot colours.
///
/// A colour is what maps a line on the chart back to a name, so this says the
/// most the puck can in the least room — and unlike a bare count it can never
/// be read as a position size by a trader glancing under the HUD, where the
/// numbers next door are quantities and prices. Past [`MAX_PUCK_DOTS`] the
/// rest becomes `+N`, which keeps the number attached to the dots explaining
/// it.
fn draw_folded_dots(ui: &mut egui::Ui, views: &[IndicatorView]) {
    let folded = folded_views(views);
    for view in folded.iter().take(MAX_PUCK_DOTS) {
        draw_status_dot(ui, view);
    }
    if folded.len() > MAX_PUCK_DOTS {
        ui.label(
            egui::RichText::new(format!("+{}", folded.len() - MAX_PUCK_DOTS))
                .small()
                .color(theme::TEXT_MUTED),
        );
    }
}

/// The rows the fold took off the chart, in the order they were added.
fn folded_views(views: &[IndicatorView]) -> Vec<&IndicatorView> {
    views
        .iter()
        .filter(|view| !survives_collapse(view))
        .collect()
}

/// What the puck's hover says: one line per folded indicator, named and
/// valued, so "which ones?" is answered without opening anything.
///
/// A row that is hidden or has display layers switched off says so here, in
/// the words its expanded row used. Without them the summary would report a
/// name and no number and leave the trader to guess whether the indicator is
/// off, broken, or merely new — the same guess the expanded row never makes
/// them make.
fn folded_summary(views: &[IndicatorView]) -> String {
    let folded = folded_views(views);
    let mut lines: Vec<String> = folded
        .iter()
        .take(MAX_HOVER_LINES)
        .map(|view| {
            let value = (!view.hidden)
                .then(|| view.columns.first().and_then(|column| column.last()))
                .flatten();
            match value {
                Some(value) => format!("{}  {}", view.label(), chart::compact_value(*value)),
                // A hidden row has no value worth quoting — the same silence
                // its expanded row keeps, for the same reason.
                None => format!("{}  hidden", view.label()),
            }
        })
        .collect();
    if folded.len() > MAX_HOVER_LINES {
        lines.push(format!("+{} more", folded.len() - MAX_HOVER_LINES));
    }
    lines.join(
        "
",
    )
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

/// What the chevron's hover says on each half of the gesture.
///
/// The key is spelled the way `LEGEND_SHORTCUT` binds it — Ctrl+L. A tooltip
/// that names a key the app does not answer to is worse than one that names
/// none: the trader presses it, nothing happens, and the feature reads as
/// broken rather than as mis-documented.
const EXPAND_HINT: &str = "show every indicator (Ctrl+L)";
const COLLAPSE_HINT: &str = "collapse to a count — broken ones stay (Ctrl+L)";

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
            .on_hover_text("remove this indicator from every chart of this layout")
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
            folded.contains("zigzag.pine") && folded.contains("error"),
            "the broken one stays, and still says why: {folded}"
        );

        // The puck names what it hid with the plots' own colours. Counted as
        // shapes rather than as text on purpose: a bare integer beside the
        // position HUD reads as a quantity, so "there is no naked count here"
        // is part of what this test holds.
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let mut dots = 0_usize;
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                draw(ctx, 0, rect, views.all(), None, true);
            });
            dots = output
                .shapes
                .iter()
                .filter(|shape| matches!(shape.shape, egui::epaint::Shape::Circle(_)))
                .count();
        }
        assert_eq!(
            dots, 2,
            "one dot per folded indicator, and none for the row that stayed"
        );

        let open = painted_folded(&ctx, &views, false);
        assert!(open.contains("EMA(9, close)"), "expanded: {open}");
        assert!(open.contains("ATR(14)"), "expanded: {open}");
    }

    /// Preview rides the puck, and only when the puck actually hid the
    /// indicator being previewed.
    ///
    /// Folding must not become a way to stop saying "the chart is showing
    /// settings the state file does not hold" — the dialog that holds them
    /// may be sitting behind the chart, which is why the chip exists at all.
    /// The second half is the honesty half: an errored indicator keeps its
    /// own row through the fold, so a puck that announced *its* preview would
    /// be describing a state the pane is not in.
    #[test]
    fn preview_rides_the_puck_and_only_for_what_the_puck_hid() {
        let ctx = egui::Context::default();
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

        let healthy = views_with(|views, slot| {
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor("EMA(9, close)"),
                vec![vec![101.5, 1_234.0]],
            ));
        });
        let slot = healthy.all()[0].slot;
        let painted = |views: &IndicatorViews, preview: Option<SlotId>| {
            let mut text = String::new();
            for _ in 0..2 {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    draw(ctx, 0, chart, views.all(), preview, true);
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
        };

        let folded = painted(&healthy, Some(slot));
        assert!(folded.contains("preview"), "the puck says it: {folded}");
        assert!(
            !folded.contains("EMA(9, close)"),
            "without unfolding the row: {folded}"
        );

        // Same indicator, now broken and previewing: it keeps its own row, so
        // the puck has nothing of its own to announce.
        let broken = views_with(|views, slot| {
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor("EMA(9, close)"),
                vec![vec![101.5]],
            ));
            views.apply(IndicatorEvent::Error {
                slot,
                error: EvalError {
                    bar_index: 3,
                    message: "broken".to_owned(),
                },
            });
        });
        let broken_slot = broken.all()[0].slot;
        let folded = painted(&broken, Some(broken_slot));
        assert!(
            !folded.contains("preview"),
            "the puck folded nothing, so it announces no preview: {folded}"
        );
        assert!(
            folded.contains("error"),
            "the row still states it: {folded}"
        );
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
