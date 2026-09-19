//! The Bubbles dock tab's sections, one view component each.
//!
//! Every section borrows exactly the settings it edits and draws them in one
//! `show`; the tab composes them in order and commits whatever changed once.

use eframe::egui;
use quantick_orderflow::engine::OrderflowHealth;
use quantick_orderflow::{
    BubbleRenderMode, BubbleSizeReference, BubbleStyle, ConsumptionMark, HeatmapConfig,
    LANE_WINDOW_PRESETS_MS, LaneWindow, LiveLaneStyle, MAX_BUBBLE_MAX_RADIUS,
    MAX_BUBBLE_MIN_RADIUS, MAX_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_SHARE,
    MAX_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_ZOOM, MIN_BUBBLE_MAX_RADIUS, MIN_LIVE_LANE_RADIUS_SCALE,
    MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_ZOOM, format_window_ms,
    lane_window_label, same_lane_window,
};
use rust_decimal::Decimal;

use crate::orderflow_render::ThemeBubbleRgb;

/// How prints fold together before they are drawn: the cluster window, the
/// dust fold, price regions and the closed-bar summary.
pub(super) struct ClusteringSection<'a> {
    pub(super) config: &'a mut HeatmapConfig,
}

impl ClusteringSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let config = self.config;
        ui.horizontal(|ui| {
            ui.label("cluster");
            egui::ComboBox::from_id_salt("heatmap_bubble_cluster")
                .selected_text(cluster_label(config.bubble_cluster_ms))
                .show_ui(ui, |ui| {
                    for (milliseconds, label) in [
                        (0, "Raw · one bubble per print"),
                        (50, "50 ms"),
                        (100, "100 ms"),
                        (200, "200 ms"),
                        (500, "500 ms"),
                        (1_000, "1 s"),
                        (2_000, "2 s"),
                    ] {
                        ui.selectable_value(&mut config.bubble_cluster_ms, milliseconds, label);
                    }
                });
        })
        .response
        .on_hover_text(
            "merge compatible prints (same side, same price range) inside this window into one bubble; quantities are summed exactly",
        );
        ui.horizontal(|ui| {
            ui.label("fold dust");
            egui::ComboBox::from_id_salt("heatmap_bubble_dust")
                .selected_text(dust_label(config.bubble_dust_merge_ms))
                .show_ui(ui, |ui| {
                    for milliseconds in [0, 500, 1_500, 3_000, 10_000] {
                        ui.selectable_value(
                            &mut config.bubble_dust_merge_ms,
                            milliseconds,
                            dust_label(milliseconds),
                        );
                    }
                });
        })
        .response
        .on_hover_text(
            "a second pass over the prints too small to read on their own: inside this window they fold into one bubble per price range. The threshold follows \"readable from px\" — quantities and trade counts are summed exactly",
        );
        ui.horizontal(|ui| {
            ui.label("region height");
            egui::ComboBox::from_id_salt("heatmap_bubble_region")
                .selected_text(region_label(config.bubble_region_rows))
                .show_ui(ui, |ui| {
                    for rows in [1, 2, 3, 4, 6, 8, 12] {
                        ui.selectable_value(
                            &mut config.bubble_region_rows,
                            rows,
                            region_label(rows),
                        );
                    }
                });
            if config.bubble_region_rows > 1 {
                ui.label("window");
                egui::ComboBox::from_id_salt("heatmap_bubble_region_ms")
                    .selected_text(region_window_label(config.bubble_region_ms))
                    .show_ui(ui, |ui| {
                        for milliseconds in [500, 1_000, 1_500, 2_000, 3_000, 5_000] {
                            ui.selectable_value(
                                &mut config.bubble_region_ms,
                                milliseconds,
                                region_window_label(milliseconds),
                            );
                        }
                    });
            }
        })
        .response
        .on_hover_text(
            "fold same-side bubbles landing in a price region this many rows tall into one bubble at their volume-weighted price — aggression read per zone, the Bookmap way, instead of one mark per row. Quantities, ids and matched evidence are summed exactly; buy and sell regions stay separate marks",
        );
        ui.checkbox(&mut config.bubble_candle_summary, "summarize closed bars")
            .on_hover_text(
                "fold every print of a bar and price range into one bubble carrying both sides, drawn as a pie whose sectors are the buy/sell proportion. The forming bar included: its pie is a running total that grows with each order, so the compressed left side reports what is happening now instead of only what already happened. Quantities, ids and matched evidence are summed exactly, and the tape still shows those same prints one by one",
            );
    }
}

/// The live lane's own settings: the reserved band right of the forming
/// bar, which has room the compressed history does not.
pub(super) struct LiveLaneSection<'a> {
    pub(super) lane: &'a mut LiveLaneStyle,
    /// History's cluster window, which "same as history" follows.
    pub(super) inherited_cluster_ms: i64,
}

impl LiveLaneSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let Self {
            lane,
            inherited_cluster_ms: inherited,
        } = self;
        egui::CollapsingHeader::new("live lane")
            .id_salt("bubble_live_lane_section")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("width");
                    ui.add(
                        egui::Slider::new(
                            &mut lane.width_share,
                            MIN_LIVE_LANE_SHARE..=MAX_LIVE_LANE_SHARE,
                        )
                        .custom_formatter(|value, _| format!("{:.0}% of the chart", value * 100.0)),
                    );
                })
                .response
                .on_hover_text(
                    "how much of the chart the rolling tape takes, up to half of it. Also set by dragging the divider on the chart; measured against the chart, not the candle, so zooming the time axis changes how many bars fit beside the tape and never how much room it gets",
                );
                window_rows(ui, lane);
                ui.horizontal(|ui| {
                    ui.label("cluster");
                    egui::ComboBox::from_id_salt("bubble_live_lane_cluster")
                        .selected_text(lane_cluster_label(lane.cluster_ms, inherited))
                        .show_ui(ui, |ui| {
                            for window in [None, Some(0), Some(50), Some(100), Some(200), Some(500)]
                            {
                                ui.selectable_value(
                                    &mut lane.cluster_ms,
                                    window,
                                    lane_cluster_label(window, inherited),
                                );
                            }
                        });
                })
                .response
                .on_hover_text(
                    "clustering window for prints on the tape. A shorter one than history's buys detail where there is room for it; \"same as history\" keeps the two regions identical",
                );
                ui.horizontal(|ui| {
                    ui.label("bubble size");
                    ui.add(
                        egui::Slider::new(
                            &mut lane.radius_scale,
                            MIN_LIVE_LANE_RADIUS_SCALE..=MAX_LIVE_LANE_RADIUS_SCALE,
                        )
                        .suffix("×"),
                    );
                })
                .response
                .on_hover_text(
                    "multiplies both bubble radii inside the lane only. The lane is the one region with room to spare, so a wider range reads as detail here and as overlap anywhere else",
                );
                ui.checkbox(&mut lane.show_marks, "boundary and live-edge line")
                    .on_hover_text(
                        "the dashed line where the bar slots end and the tape begins, and the line on the live edge itself at its right end",
                    );
            });
    }
}

/// The lane's window picker, and the one row that tunes whichever mode it
/// is in: the zoom while it follows the bars, the duration while pinned.
fn window_rows(ui: &mut egui::Ui, lane: &mut LiveLaneStyle) {
    ui.horizontal(|ui| {
        ui.label("window");
        egui::ComboBox::from_id_salt("bubble_live_lane_window")
            .selected_text(lane_window_label(lane.window, None))
            .show_ui(ui, |ui| {
                let mut choose = |ui: &mut egui::Ui, option: LaneWindow| {
                    // Selecting the mode the lane is already in must
                    // not overwrite the number it is carrying, so
                    // the entry compares modes and assigns whole
                    // values only when the mode actually changes.
                    let selected = same_lane_window(lane.window, option);
                    if ui
                        .selectable_label(selected, lane_window_label(option, None))
                        .clicked()
                        && !selected
                    {
                        lane.window = option;
                    }
                };
                choose(ui, LaneWindow::default());
                for ms in LANE_WINDOW_PRESETS_MS {
                    choose(ui, LaneWindow::Fixed { ms });
                }
            });
    })
    .response
    .on_hover_text(
        "how much market time fits in the tape. Following the bars keeps roughly one bar's worth of flow in the band whatever the instrument; a fixed window shows that much time however fast the bars are closing, which is what a burst calls for. The clustering window follows either way, so a crowded tape gathers into fewer, bigger bubbles instead of a smear",
    );
    match &mut lane.window {
        LaneWindow::Auto { zoom } => {
            ui.horizontal(|ui| {
                ui.label("zoom");
                ui.add(
                    egui::Slider::new(zoom, MIN_LIVE_LANE_ZOOM..=MAX_LIVE_LANE_ZOOM)
                        .logarithmic(true)
                        .suffix("×"),
                );
            })
            .response
            .on_hover_text(
                "the recent bars' typical duration, scaled. Zoom in and prints run across the tape faster and further apart; zoom out and more time crowds in. Also set by dragging the time strip under the tape",
            );
        }
        LaneWindow::Fixed { ms } => {
            ui.horizontal(|ui| {
                ui.label("duration");
                ui.add(
                    egui::Slider::new(ms, MIN_LIVE_LANE_WINDOW_MS..=MAX_LIVE_LANE_WINDOW_MS)
                        .logarithmic(true)
                        .custom_formatter(|value, _| format_window_ms(value as i64)),
                );
            })
            .response
            .on_hover_text(
                "market time pinned in the tape, whatever the bars do. Also set by dragging the time strip under the tape",
            );
        }
    }
}

/// Render style, radii and size reference, and the finish of every disc.
pub(super) struct SizePlacementSection<'a> {
    pub(super) bubbles: &'a mut BubbleStyle,
}

impl SizePlacementSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let bubbles = self.bubbles;
        egui::CollapsingHeader::new("size & placement")
            .id_salt("bubble_size_section")
            .default_open(true)
            .show(ui, |ui| {
                render_style_rows(ui, bubbles);
                size_rows(ui, bubbles);
                finish_rows(ui, bubbles);
            });
    }
}

/// Flat or sphere, and the sphere's shading when it is chosen.
fn render_style_rows(ui: &mut egui::Ui, bubbles: &mut BubbleStyle) {
    ui.horizontal(|ui| {
        ui.label("render style");
        egui::ComboBox::from_id_salt("bubble_render_mode")
            .selected_text(render_mode_label(bubbles.render_mode))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut bubbles.render_mode,
                    BubbleRenderMode::Flat,
                    "Flat · 2D disc",
                );
                ui.selectable_value(
                    &mut bubbles.render_mode,
                    BubbleRenderMode::Sphere,
                    "Sphere · 3D shaded",
                );
            })
            .response
            .on_hover_text(
                "flat is the classic solid disc. Sphere shades every bubble like a \
                 ball lit from the upper left (the Bookmap look): on a dense tape \
                 each darkened rim keeps overlapping prints readable as separate \
                 bubbles instead of one merged blob. Purely visual — clustering and \
                 liquidity association do not change.",
            );
    });
    if bubbles.render_mode == BubbleRenderMode::Sphere {
        ui.add(egui::Slider::new(&mut bubbles.sphere_shading, 0.0..=1.0).text("depth shading"))
            .on_hover_text(
                "how much the rim darkens toward the edge; higher separates \
                 overlapping bubbles harder, zero reads flat again",
            );
        ui.add(egui::Slider::new(&mut bubbles.sphere_highlight, 0.0..=1.0).text("highlight"))
            .on_hover_text("strength of the light spot that gives the ball its volume");
        ui.small(
            "Sphere shading applies from the 'detail from px' radius up; smaller \
             prints stay cheap dots.",
        );
    }
}

/// The radius range, what quantity fills it, and the display floor.
fn size_rows(ui: &mut egui::Ui, bubbles: &mut BubbleStyle) {
    ui.add(
        egui::Slider::new(&mut bubbles.min_radius, 0.5..=MAX_BUBBLE_MIN_RADIUS)
            .text("smallest print px"),
    )
    .on_hover_text("floor radius, so the quietest print is still a visible dot");
    ui.add(
        egui::Slider::new(
            &mut bubbles.max_radius,
            MIN_BUBBLE_MAX_RADIUS..=MAX_BUBBLE_MAX_RADIUS,
        )
        .text("biggest print px"),
    )
    .on_hover_text(
        "radius of a full-size print; bubble *area* stays proportional to \
                         quantity between the two limits",
    );
    if bubbles.max_radius < bubbles.min_radius {
        bubbles.max_radius = bubbles.min_radius;
    }
    ui.horizontal(|ui| {
        ui.label("full size at");
        egui::ComboBox::from_id_salt("bubble_size_reference")
            .selected_text(size_reference_label(bubbles.size_reference))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut bubbles.size_reference,
                    BubbleSizeReference::VisibleP99,
                    "Auto · session P99",
                );
                ui.selectable_value(
                    &mut bubbles.size_reference,
                    BubbleSizeReference::VisibleMax,
                    "Auto · largest in session",
                );
                ui.selectable_value(
                    &mut bubbles.size_reference,
                    BubbleSizeReference::Fixed,
                    "Fixed quantity",
                );
            })
            .response
            .on_hover_text(
                "the automatic modes measure the recorded session, so zooming never \
                 resizes a print. P99 keeps one outlier sweep from shrinking \
                 everything else, but the top 1% all render at the maximum radius. \
                 'Largest in session' restores a strict order; a fixed quantity \
                 makes bubble size mean the same thing across sessions.",
            );
    });
    if bubbles.size_reference == BubbleSizeReference::Fixed {
        ui.add(
            egui::DragValue::new(&mut bubbles.size_reference_quantity)
                .range(0.000_001..=1_000_000_000.0)
                .speed(1.0)
                .prefix("full size qty "),
        )
        .on_hover_text("quantity drawn at the maximum radius, in the symbol's units");
    }
    ui.add(
        egui::DragValue::new(&mut bubbles.min_quantity)
            .range(0.0..=1_000_000_000.0)
            .speed(0.5)
            .prefix("hide below qty "),
    )
    .on_hover_text(
        "display-only floor: smaller prints are not drawn. Applied after liquidity \
         association, so a hidden print still counts as the evidence behind a \
         consumption mark. Zero draws everything.",
    );
}

/// Side separation, fill, rim, halo and the two small-print thresholds.
fn finish_rows(ui: &mut egui::Ui, bubbles: &mut BubbleStyle) {
    ui.add(egui::Slider::new(&mut bubbles.side_offset, 0.0..=20.0).text("side separation px"))
        .on_hover_text(
            "buy bubbles are nudged up, sell bubbles down: a buy lifts the ask, a sell \
             hits the bid, so they are not on the same row. With a one-tick spread they \
             would otherwise stack into an unreadable line. Zero pins both to the exact \
             price.",
        );
    ui.add(egui::Slider::new(&mut bubbles.opacity, 0.05..=1.0).text("fill opacity"));
    ui.add(egui::Slider::new(&mut bubbles.outline_width, 0.0..=4.0).text("rim width px"))
        .on_hover_text("zero draws no rim");
    ui.add(egui::Slider::new(&mut bubbles.halo_strength, 0.0..=0.6).text("halo"))
        .on_hover_text("soft glow behind the fill; opens up a little with size");
    ui.add(egui::Slider::new(&mut bubbles.detail_min_radius, 0.0..=20.0).text("detail from px"))
        .on_hover_text(
            "bubbles smaller than this are plain dots (no halo, rim or impact ring). \
             Raising it buys frame time on a fast tape.",
        );
    ui.add(
        egui::Slider::new(&mut bubbles.readable_min_radius, 0.0..=24.0).text("readable from px"),
    )
    .on_hover_text(
        "the size at which a bubble stops being readable on its own. Prints below \
         it are what \"fold dust\" merges, and what the ring below marks. Raise it \
         for fewer, larger bubbles; zero disables both.",
    );
    ui.checkbox(&mut bubbles.hollow_small_buys, "hollow small buys")
        .on_hover_text(
            "draw buy prints below the readable radius as an open ring instead of \
             a solid dot. At that size a green speck and a red speck read the same; \
             a ring and a disc do not. Larger bubbles keep their fill.",
        );
}

/// The marks a print leaves when it ate resting liquidity.
pub(super) struct ConsumptionMarksSection<'a> {
    pub(super) bubbles: &'a mut BubbleStyle,
}

impl ConsumptionMarksSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let bubbles = self.bubbles;
        egui::CollapsingHeader::new("consumption marks")
            .id_salt("bubble_consumption_section")
            .default_open(true)
            .show(ui, |ui| {
                ui.small(
                    "Drawn only when a print aligned with a factual L2 reduction — the bubble \
                     ate resting liquidity.",
                );
                ui.horizontal(|ui| {
                    ui.label("mark");
                    egui::ComboBox::from_id_salt("bubble_consumption_mark")
                        .selected_text(consumption_mark_label(bubbles.consumption_mark))
                        .show_ui(ui, |ui| {
                            for mark in [
                                ConsumptionMark::Crown,
                                ConsumptionMark::Front,
                                ConsumptionMark::None,
                            ] {
                                ui.selectable_value(
                                    &mut bubbles.consumption_mark,
                                    mark,
                                    consumption_mark_label(mark),
                                );
                            }
                        })
                        .response
                        .on_hover_text(
                            "the crown is an open arc just outside the rim, on the side of the \
                             book the print ate — its length grows with how much of the print \
                             matched, and it never crosses the disc whose area is the quantity. \
                             The front is the older vertical line through the bubble.",
                        );
                });
                if bubbles.consumption_mark.is_front() {
                    ui.add(
                        egui::Slider::new(&mut bubbles.front_width, 0.5..=10.0)
                            .text("front width px"),
                    );
                    ui.add(
                        egui::Slider::new(&mut bubbles.front_length_scale, 0.5..=6.0)
                            .text("front length × radius"),
                    );
                }
                ui.checkbox(&mut bubbles.show_impact_ring, "impact ring on the rim");
                ui.add_enabled(
                    bubbles.show_impact_ring,
                    egui::Slider::new(&mut bubbles.impact_ring_width, 0.5..=6.0)
                        .text("ring width px"),
                )
                .on_hover_text("brightness of the ring also tracks how much of the print matched");
                ui.add(
                    egui::Slider::new(&mut bubbles.trail_length, 0.0..=80.0)
                        .text("trail length px"),
                )
                .on_hover_text(
                    "the glow leaking into the consumed side, marking where the wall ended; \
                     zero draws no trail",
                );
                ui.add_enabled(
                    bubbles.trail_length > 0.0,
                    egui::Slider::new(&mut bubbles.trail_opacity, 0.0..=1.0).text("trail opacity"),
                );
            });
    }
}

/// Quantity and cluster-count labels inside the bubbles.
pub(super) struct LabelsSection<'a> {
    pub(super) bubbles: &'a mut BubbleStyle,
}

impl LabelsSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let bubbles = self.bubbles;
        egui::CollapsingHeader::new("labels")
            .id_salt("bubble_label_section")
            .show(ui, |ui| {
                ui.checkbox(
                    &mut bubbles.show_quantity_labels,
                    "quantity inside the bubble",
                );
                ui.checkbox(&mut bubbles.show_trade_count, "×N clustered prints");
                ui.add(
                    egui::Slider::new(&mut bubbles.label_min_radius, 4.0..=48.0)
                        .text("label from px"),
                )
                .on_hover_text(
                    "only bubbles this big get a label, and only when the text fits inside",
                );
            });
    }
}

/// Per-colour overrides, each falling back to the chart theme's own.
pub(super) struct ColoursSection<'a> {
    pub(super) bubbles: &'a mut BubbleStyle,
    pub(super) theme_rgb: ThemeBubbleRgb,
}

impl ColoursSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let Self { bubbles, theme_rgb } = self;
        egui::CollapsingHeader::new("colours")
            .id_salt("bubble_colour_section")
            .show(ui, |ui| {
                let front_fallback = bubbles.front_color.unwrap_or(theme_rgb.front);
                color_override(ui, "buy", &mut bubbles.buy_color, theme_rgb.buy);
                color_override(ui, "sell", &mut bubbles.sell_color, theme_rgb.sell);
                color_override(
                    ui,
                    "consumption front",
                    &mut bubbles.front_color,
                    theme_rgb.front,
                );
                color_override(ui, "trail", &mut bubbles.trail_color, front_fallback);
                color_override(ui, "label", &mut bubbles.label_color, theme_rgb.text);
                ui.small("Unset colours follow the chart theme.");
            });
    }
}

/// Read-only: how many marks the projection drew, and what it left out.
pub(super) struct BubbleHealthSection<'a> {
    pub(super) health: &'a OrderflowHealth,
    pub(super) show_aggressions: bool,
}

impl BubbleHealthSection<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui) {
        let health = self.health;
        // The projection carries clusters whether or not the
        // bubble layer draws them — the live strip reads the
        // same ones. Calling them "bubbles" while none is on
        // screen would report a layer that is off.
        let noun = if self.show_aggressions {
            "bubbles"
        } else {
            "clusters (bubble layer off)"
        };
        ui.label(format!(
            "{} {noun} projected · {} aggressions retained",
            health.projection_aggressions, health.aggression_count
        ));
        if health.floored_quantity > Decimal::ZERO {
            ui.small(format!(
                "{} contracts below your display floor are not drawn",
                health.floored_quantity
            ))
            .on_hover_text(concat!(
                "the minimum-quantity setting under bubble visuals. It is the ",
                "only thing left that keeps contracts off the canvas, so it says ",
                "how many - in contracts, not in dots, because what matters is ",
                "the size of what is missing. Set it to zero to draw everything",
            ));
        }
        if health.folded_aggressions > 0 {
            ui.small(format!(
                "{} marks merged into a neighbour to fit the frame",
                health.folded_aggressions
            ))
            .on_hover_text(concat!(
                "the frame draws a bounded number of bubbles, split between ",
                "the candles and the tape so neither can crowd the other out. ",
                "Over that budget the marks merge - the candles fold their ",
                "smallest together, the tape folds its oldest - and a merged ",
                "bubble carries the exact summed quantity and says how many ",
                "marks it stands for. A fold never crosses a side, a pane or a ",
                "bar, so a frame with more of those than it has budget draws ",
                "the extra marks instead. Nothing is discarded",
            ));
        }
    }
}

fn cluster_label(milliseconds: i64) -> String {
    if milliseconds == 0 {
        "Raw".to_owned()
    } else {
        format!("{milliseconds} ms")
    }
}

fn lane_cluster_label(window: Option<i64>, inherited: i64) -> String {
    match window {
        None => format!("Same as history · {}", dust_label(inherited)),
        Some(0) => "Raw · one bubble per print".to_owned(),
        Some(milliseconds) => dust_label(milliseconds),
    }
}

fn region_label(rows: u32) -> String {
    if rows <= 1 {
        "Off · one mark per row".to_owned()
    } else {
        format!("{rows} rows")
    }
}

fn region_window_label(milliseconds: i64) -> String {
    if milliseconds % 1_000 == 0 {
        format!("{} s", milliseconds / 1_000)
    } else {
        format!("{milliseconds} ms")
    }
}

fn dust_label(milliseconds: i64) -> String {
    if milliseconds == 0 {
        "Off · draw every print".to_owned()
    } else if milliseconds % 1_000 == 0 {
        format!("{} s", milliseconds / 1_000)
    } else {
        format!("{milliseconds} ms")
    }
}

const fn size_reference_label(reference: BubbleSizeReference) -> &'static str {
    match reference {
        BubbleSizeReference::VisibleP99 => "Auto · session P99",
        BubbleSizeReference::VisibleMax => "Auto · largest in session",
        BubbleSizeReference::Fixed => "Fixed quantity",
    }
}

const fn render_mode_label(mode: BubbleRenderMode) -> &'static str {
    match mode {
        BubbleRenderMode::Flat => "Flat · 2D disc",
        BubbleRenderMode::Sphere => "Sphere · 3D shaded",
    }
}

const fn consumption_mark_label(mark: ConsumptionMark) -> &'static str {
    match mark {
        ConsumptionMark::Crown => "Crown · arc outside the rim",
        ConsumptionMark::Front => "Front · line through the bubble",
        ConsumptionMark::None => "None",
    }
}

/// One optional colour: a swatch that adopts the override the moment it is
/// touched, and a way back to the theme.
fn color_override(ui: &mut egui::Ui, label: &str, value: &mut Option<[u8; 3]>, fallback: [u8; 3]) {
    ui.horizontal(|ui| {
        let mut rgb = value.unwrap_or(fallback);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *value = Some(rgb);
        }
        ui.label(label);
        if value.is_some() {
            if ui
                .small_button("theme")
                .on_hover_text("follow the chart theme again")
                .clicked()
            {
                *value = None;
            }
        } else {
            ui.weak("(theme)");
        }
    });
}
