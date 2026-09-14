//! The order-flow settings surface: the L2 dock tab, the bubbles dock tab,
//! the live-lane and bubble control groups they are built from, and the
//! bubble preset picker with its save and reload.
//!
//! Drawn only while a dock tab is open, never on the chart's own frame.
//! Every control writes the same `HeatmapConfig` field the chart reads, and
//! the root's `commit_config_changes` is the one door a change goes
//! through to reach the worker.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_orderbook::BookLevel;
use quantick_orderflow::{
    BubbleRenderMode, BubbleSizeReference, ConsumptionMark, DisplayGrouping, HeatmapConfig,
    HeatmapTheme, IntensityMode, LANE_WINDOW_PRESETS_MS, LaneWindow, MAX_BUBBLE_MAX_RADIUS,
    MAX_BUBBLE_MIN_RADIUS, MAX_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_SHARE,
    MAX_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_ZOOM, MIN_BUBBLE_MAX_RADIUS, MIN_LIVE_LANE_RADIUS_SCALE,
    MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_ZOOM, format_window_ms,
    lane_window_label, same_lane_window,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::bubble_presets;
use crate::bubble_presets::{BubblePreset, PresetSource};
use crate::orderflow_render::{draw_preview, theme_bubble_rgb};

use super::OrderflowView;
use super::frame::status_color;

impl OrderflowView {
    /// Picker, save and reload for the named bubble looks.
    ///
    /// Saving writes the whole presets file, so what the panel shows and what
    /// the repository holds never drift apart.
    fn draw_bubble_presets(&mut self, ui: &mut egui::Ui) {
        // The picker reads the stored presets while the closure below wants to
        // mutate them, so it hands back an index and the name is read after.
        // Cloning every name each frame would be the other way out, and this
        // runs on the render thread.
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label("preset");
            let selected = if self.presets.active.is_empty() {
                "— custom —"
            } else {
                self.presets.active.as_str()
            };
            egui::ComboBox::from_id_salt("bubble_preset")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (index, preset) in self.presets.presets.iter().enumerate() {
                        if ui
                            .selectable_label(self.presets.active == preset.name, &preset.name)
                            .clicked()
                        {
                            chosen = Some(index);
                        }
                    }
                });
            if ui
                .small_button(icons::ARROW_CLOCKWISE)
                .on_hover_text("reload the presets file from disk, discarding unsaved tweaks")
                .clicked()
            {
                self.reload_presets();
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_name_draft)
                    .hint_text("preset name")
                    .desired_width(150.0),
            );
            if ui
                .button("save")
                .on_hover_text("write the current bubble settings to the presets file")
                .clicked()
            {
                self.save_preset();
            }
            let removable = !self.preset_name_draft.trim().is_empty()
                && self.presets.get(self.preset_name_draft.trim()).is_some();
            if ui
                .add_enabled(removable, egui::Button::new("delete"))
                .on_hover_text("remove this preset from the file")
                .clicked()
            {
                let name = self.preset_name_draft.trim().to_owned();
                self.presets.remove(&name);
                self.persist_presets(format!("preset '{name}' removed"));
            }
        });
        if let Some(index) = chosen
            && let Some(name) = self
                .presets
                .presets
                .get(index)
                .map(|preset| preset.name.clone())
        {
            self.apply_preset(&name);
        }
        ui.small(format!("presets · {}", self.presets_source));
        if let Some(status) = &self.preset_status {
            ui.small(status.clone());
        }
    }

    /// Apply the stored preset called `name`, reporting whether it exists.
    ///
    /// The panel's picker and a feed's declared preset both land here, so a
    /// preset applies identically no matter who asked. An unknown name changes
    /// nothing and returns `false`; the caller decides how loudly to say so.
    pub(crate) fn apply_preset(&mut self, name: &str) -> bool {
        let Some(preset) = self.presets.get(name).cloned() else {
            return false;
        };
        preset.apply_to(&mut self.config);
        self.presets.active = preset.name.clone();
        self.preset_name_draft = preset.name.clone();
        self.preset_status = Some(format!("'{}' applied", preset.name));
        true
    }

    fn save_preset(&mut self) {
        let name = self.preset_name_draft.trim().to_owned();
        if name.is_empty() {
            self.preset_status = Some("name the preset before saving".to_owned());
            return;
        }
        self.presets
            .upsert(BubblePreset::capture(&name, &self.config));
        self.presets.active = name.clone();
        self.persist_presets(format!("'{name}' saved"));
    }

    fn persist_presets(&mut self, success: String) {
        match bubble_presets::save(&self.presets) {
            Ok(path) => {
                self.presets_source = PresetSource::WorkingDir(path.clone());
                self.preset_status = Some(format!("{success} → {}", path.display()));
            }
            Err(message) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BUBBLE_PRESETS_NOT_SAVED",
                    error = message.as_str(),
                    action = "keep_settings_in_memory_only",
                    "bubble presets could not be written; the current look is in memory only"
                );
                self.preset_status = Some(format!("not saved — {message}"));
            }
        }
    }

    fn reload_presets(&mut self) {
        let (presets, source, error) = bubble_presets::load();
        self.presets = presets;
        self.presets_source = source;
        match error {
            Some(message) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BUBBLE_PRESETS_UNREADABLE",
                    error = message.as_str(),
                    action = "using_built_in_presets",
                    "bubble presets file could not be read; built-in presets are in use"
                );
                self.preset_status = Some(format!("presets not loaded — {message}"));
            }
            None => {
                let active = self.presets.active.clone();
                if active.is_empty() {
                    self.preset_status = Some("presets reloaded".to_owned());
                } else {
                    self.apply_preset(&active);
                    self.preset_status = Some(format!("reloaded · '{active}' applied"));
                }
            }
        }
    }

    /// Every visual choice for the bubbles themselves, grouped so the section
    /// stays readable: size and placement, the marks a consuming print leaves,
    /// labels, and colour.
    /// The live lane's own settings: the reserved band right of the forming
    /// bar, which has room the compressed history does not.
    fn draw_live_lane_controls(&mut self, ui: &mut egui::Ui) {
        let inherited = self.config.bubble_cluster_ms;
        let lane = &mut self.config.live_lane;

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
                // One row, whichever language the window is in: the zoom while
                // it follows the bars, the duration while it is pinned.
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
                                egui::Slider::new(
                                    ms,
                                    MIN_LIVE_LANE_WINDOW_MS..=MAX_LIVE_LANE_WINDOW_MS,
                                )
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

    fn draw_bubble_controls(&mut self, ui: &mut egui::Ui) {
        let theme_rgb = theme_bubble_rgb(self.config.theme);
        let bubbles = &mut self.config.bubbles;

        egui::CollapsingHeader::new("size & placement")
            .id_salt("bubble_size_section")
            .default_open(true)
            .show(ui, |ui| {
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
                    ui.add(
                        egui::Slider::new(&mut bubbles.sphere_shading, 0.0..=1.0)
                            .text("depth shading"),
                    )
                    .on_hover_text(
                        "how much the rim darkens toward the edge; higher separates \
                         overlapping bubbles harder, zero reads flat again",
                    );
                    ui.add(
                        egui::Slider::new(&mut bubbles.sphere_highlight, 0.0..=1.0)
                            .text("highlight"),
                    )
                    .on_hover_text("strength of the light spot that gives the ball its volume");
                    ui.small(
                        "Sphere shading applies from the 'detail from px' radius up; smaller \
                         prints stay cheap dots.",
                    );
                }
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
                ui.add(
                    egui::Slider::new(&mut bubbles.side_offset, 0.0..=20.0)
                        .text("side separation px"),
                )
                .on_hover_text(
                    "buy bubbles are nudged up, sell bubbles down: a buy lifts the ask, a sell \
                     hits the bid, so they are not on the same row. With a one-tick spread they \
                     would otherwise stack into an unreadable line. Zero pins both to the exact \
                     price.",
                );
                ui.add(egui::Slider::new(&mut bubbles.opacity, 0.05..=1.0).text("fill opacity"));
                ui.add(
                    egui::Slider::new(&mut bubbles.outline_width, 0.0..=4.0).text("rim width px"),
                )
                .on_hover_text("zero draws no rim");
                ui.add(egui::Slider::new(&mut bubbles.halo_strength, 0.0..=0.6).text("halo"))
                    .on_hover_text("soft glow behind the fill; opens up a little with size");
                ui.add(
                    egui::Slider::new(&mut bubbles.detail_min_radius, 0.0..=20.0)
                        .text("detail from px"),
                )
                .on_hover_text(
                    "bubbles smaller than this are plain dots (no halo, rim or impact ring). \
                     Raising it buys frame time on a fast tape.",
                );
                ui.add(
                    egui::Slider::new(&mut bubbles.readable_min_radius, 0.0..=24.0)
                        .text("readable from px"),
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
            });

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

    /// The L2 dock tab's body: everything the depth map owns. Returns
    /// whether capture must restart because the base capture resolution
    /// changed.
    ///
    /// The layer's *toggle* lives in the toolbar; this tab is settings only —
    /// opening it never starts capture (looking is not enabling).
    pub fn draw_l2_tab(&mut self, ui: &mut egui::Ui) -> bool {
        self.sync_published();
        let before = self.config.clone();
        egui::ScrollArea::vertical()
            .id_salt("orderflow_l2_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(self.published.status.label())
                        .small()
                        .color(status_color(&self.published.status)),
                );
                ui.small(
                    "Brightness is resting liquidity. Green/red bubbles are confirmed trades.",
                );
                ui.small(
                    "A bite means a compatible L2 reduction; a violet tail is an unattributed withdrawal.",
                );
                draw_preview(ui, &self.config).on_hover_text(
                    "Deterministic preview: persistent wall, aligned depletion, full withdrawal and clustered trades.",
                );
                ui.separator();

                ui.horizontal(|ui| {
                    ui.strong("liquidity ranges");
                    ui.add_space(8.0);
                    egui::ComboBox::from_id_salt("heatmap_theme")
                        .selected_text(theme_label(self.config.theme))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.config.theme,
                                HeatmapTheme::Bookmap,
                                "Bookmap",
                            );
                            ui.selectable_value(
                                &mut self.config.theme,
                                HeatmapTheme::HighContrast,
                                "High contrast",
                            );
                            ui.selectable_value(
                                &mut self.config.theme,
                                HeatmapTheme::ColorBlind,
                                "Color blind",
                            );
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("display range");
                    egui::ComboBox::from_id_salt("heatmap_display_grouping")
                        .selected_text(display_grouping_label(self.config.display_grouping))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.config.display_grouping,
                                DisplayGrouping::Adaptive { target_rows: 160 },
                                "Auto · follows zoom",
                            );
                            ui.selectable_value(
                                &mut self.config.display_grouping,
                                DisplayGrouping::Native,
                                "Native · 1×",
                            );
                            for multiple in [2, 5, 10, 25, 50] {
                                ui.selectable_value(
                                    &mut self.config.display_grouping,
                                    DisplayGrouping::Multiple(multiple),
                                    format!("Range · {multiple}×"),
                                );
                            }
                            ui.selectable_value(
                                &mut self.config.display_grouping,
                                DisplayGrouping::Multiple(3),
                                "Custom…",
                            );
                        });
                });
                match &mut self.config.display_grouping {
                    DisplayGrouping::Adaptive { target_rows } => {
                        ui.add(
                            egui::Slider::new(target_rows, 40..=400)
                                .text("target screen rows")
                                .logarithmic(true),
                        )
                        .on_hover_text(
                            "automatically widens price ranges as you zoom out; history stays intact",
                        );
                    }
                    DisplayGrouping::Multiple(multiple)
                        if ![2_u32, 5, 10, 25, 50].contains(multiple) =>
                    {
                        ui.add(
                            egui::DragValue::new(multiple)
                                .range(1..=1_000_000)
                                .prefix("custom multiple "),
                        );
                    }
                    DisplayGrouping::Native | DisplayGrouping::Multiple(_) => {}
                }
                ui.small(
                    "Display grouping is instant and non-destructive; it never restarts L2 capture.",
                );
                ui.add(egui::Slider::new(&mut self.config.opacity, 0.05..=1.0).text("brightness"));
                ui.add(
                    egui::Slider::new(&mut self.config.gamma, 0.25..=2.0)
                        .text("quiet liquidity"),
                );

                ui.separator();
                ui.strong("visual layers");
                ui.small(
                    "One switch per legend entry. Display-only: capture keeps running and \
                     history keeps accumulating, so a layer switched back on repaints the \
                     past it kept recording.",
                );
                ui.checkbox(&mut self.config.show_liquidity, "liquidity")
                    .on_hover_text("the resting-liquidity heat cells");
                ui.checkbox(
                    &mut self.config.show_buy_aggressions,
                    "buy aggression",
                )
                .on_hover_text(
                    "buy-side bubbles; needs the aggression layer on (toolbar). Hiding one \
                     side never rescales the other",
                );
                ui.checkbox(
                    &mut self.config.show_sell_aggressions,
                    "sell aggression",
                )
                .on_hover_text(
                    "sell-side bubbles; needs the aggression layer on (toolbar). Hiding one \
                     side never rescales the other",
                );
                ui.checkbox(
                    &mut self.config.show_aligned_depletion,
                    "aggression-aligned depletion",
                )
                .on_hover_text(
                    "depletion markers where a factual trade matches a factual L2 reduction",
                );
                ui.checkbox(
                    &mut self.config.show_unattributed_reductions,
                    "L2 reduction (unattributed)",
                )
                .on_hover_text(
                    "depth-only reductions and their fading withdrawal tails; with both \
                     depletion layers off, bubbles also lose their consumption marks",
                );
                ui.checkbox(&mut self.config.show_gaps, "L2 gap")
                    .on_hover_text(
                        "dashed boundaries around intervals with no depth coverage; the stretch \
                         older than this session's capture is marked by its boundary alone",
                    );

                ui.separator();
                ui.strong("liquidity response");
                ui.add_enabled(
                    self.config.liquidity_events_enabled(),
                    egui::Slider::new(&mut self.config.liquidity_correlation_ms, 25..=1_000)
                        .text("matching window ms")
                        .logarithmic(true),
                )
                .on_hover_text(
                    "time/price window used to associate a factual trade with a factual L2 reduction",
                );
                ui.add_enabled(
                    self.config.show_unattributed_reductions,
                    egui::Slider::new(&mut self.config.min_unattributed_reduction, 0.0..=1.0)
                        .text("min unattributed pull"),
                )
                .on_hover_text(
                    "hide unattributed (depth-only) reductions smaller than this fraction of the level; aggression-aligned bites always show",
                );
                ui.add_enabled(
                    self.config.show_unattributed_reductions,
                    egui::Slider::new(&mut self.config.min_unattributed_pull_share, 0.0..=1.0)
                        .text("min pull vs walls"),
                )
                .on_hover_text(
                    "hide unattributed pulls smaller than this share of the visible full-intensity liquidity (P99); a deep pull of a tiny level is noise, of a wall it is the story",
                );
                ui.small(
                    "Association is evidence, not causality: depth updates can also contain pulls or replacements.",
                );

                ui.separator();
                ui.strong("scale & history");
                let mut retention_minutes = self.config.retention_ms as f64 / 60_000.0;
                ui.add(
                    egui::Slider::new(&mut retention_minutes, 1.0..=1_440.0)
                        .logarithmic(true)
                        .text("retention min"),
                );
                self.config.retention_ms = (retention_minutes * 60_000.0) as i64;
                let mut automatic = matches!(self.config.intensity_mode, IntensityMode::VisibleP99);
                ui.checkbox(&mut automatic, "auto intensity (visible P99)");
                if automatic {
                    self.config.intensity_mode = IntensityMode::VisibleP99;
                } else {
                    let mut maximum = match self.config.intensity_mode {
                        IntensityMode::Fixed(value) => value.to_f64().unwrap_or(1.0),
                        IntensityMode::VisibleP99 => 1.0,
                    };
                    ui.add(
                        egui::DragValue::new(&mut maximum)
                            .range(0.000_000_01..=1_000_000_000.0)
                            .speed(1.0)
                            .prefix("full qty "),
                    );
                    self.config.intensity_mode = IntensityMode::Fixed(
                        Decimal::from_f64(maximum.max(0.000_000_01)).unwrap_or(Decimal::ONE),
                    );
                }
                ui.checkbox(&mut self.config.show_legend, "show chart legend");

                ui.collapsing("advanced · capture resolution", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("base price bucket");
                        ui.add(
                            egui::DragValue::new(&mut self.capture_grouping_draft)
                                .range(0.000_000_01..=1_000_000.0)
                                .speed(0.01),
                        );
                    });
                    let candidate =
                        Decimal::from_f64(self.capture_grouping_draft.max(0.000_000_01))
                            .unwrap_or(Decimal::new(1, 2));
                    if ui
                        .add_enabled(
                            candidate != self.config.price_grouping,
                            egui::Button::new("apply base resolution & resync"),
                        )
                        .clicked()
                    {
                        self.config.price_grouping = candidate;
                    }
                    ui.small(
                        "Changing the capture bucket requires a fresh snapshot and clears retained L2 history.",
                    );
                });

                ui.separator();
                let health = &self.published.health;
                ui.label(format!(
                    "{} · {} bid / {} ask levels",
                    self.published.status.label(),
                    health.bid_levels,
                    health.ask_levels
                ));
                if let Some(ladder) = &self.published.ladder {
                    let side = |level: Option<BookLevel>| match level {
                        Some(level) => format!("{} × {}", level.price(), level.quantity()),
                        None => "—".to_owned(),
                    };
                    let spread = match (ladder.best_bid, ladder.best_ask) {
                        (Some(bid), Some(ask)) => (ask.price() - bid.price()).to_string(),
                        _ => "—".to_owned(),
                    };
                    ui.label(format!(
                        "book now: bid {} · ask {} · spread {}",
                        side(ladder.best_bid),
                        side(ladder.best_ask),
                        spread
                    ))
                    .on_hover_text(
                        "Best resting bid and ask of the live book, read from the published ladder.",
                    );
                    ui.small(format!(
                        "ladder holds {} bids / {} asks in view",
                        ladder.bids.len(),
                        ladder.asks.len()
                    ));
                }
                ui.label(format!(
                    "{} runs · {:.1} MiB retained · projection {:.1} ms",
                    health.archived_runs + health.active_levels,
                    health.history_bytes as f64 / (1024.0 * 1024.0),
                    health.projection_ms
                ));
                ui.label(format!(
                    "effective range {} · {}× base",
                    health.effective_grouping, health.effective_grouping_multiple
                ));
                if ui.button("reset L2 visuals").clicked() {
                    let price_grouping = self.config.price_grouping;
                    self.capture_grouping_draft =
                        price_grouping.to_f64().unwrap_or(self.capture_grouping_draft);
                    self.config = HeatmapConfig {
                        enabled: self.config.enabled,
                        price_grouping,
                        // The bubble layer owns its own panel and its own reset,
                        // so its whole look (and the preset it came from) stays.
                        show_aggressions: self.config.show_aggressions,
                        bubble_cluster_ms: self.config.bubble_cluster_ms,
                        bubble_dust_merge_ms: self.config.bubble_dust_merge_ms,
                        bubble_candle_summary: self.config.bubble_candle_summary,
                        bubble_region_rows: self.config.bubble_region_rows,
                        bubble_region_ms: self.config.bubble_region_ms,
                        bubbles: self.config.bubbles.clone(),
                        live_lane: self.config.live_lane.clone(),
                        ..HeatmapConfig::default()
                    };
                }
            });
        self.commit_config_changes(before)
    }

    /// The Bubbles dock tab's body: the aggression layer's settings.
    /// Everything here is independent of L2 capture — bubbles are built from
    /// the aggregate-trade stream. Same return contract as
    /// [`Self::draw_l2_tab`].
    pub fn draw_bubbles_tab(&mut self, ui: &mut egui::Ui) -> bool {
        self.sync_published();
        let before = self.config.clone();
        egui::ScrollArea::vertical()
            .id_salt("orderflow_bubbles_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                        ui.small(
                            "Confirmed executions from the trade stream. Colour is the aggressor side; area is quantity.",
                        );
                        ui.small(
                            "This layer is independent: it keeps drawing with L2 capture off.",
                        );
                        draw_preview(ui, &self.config).on_hover_text(
                            "Deterministic preview: every control below shows its effect here, without waiting for a trade.",
                        );
                        ui.separator();

                        self.draw_bubble_presets(ui);
                        ui.separator();

                        ui.checkbox(&mut self.config.show_aggressions, "show aggression bubbles")
                            .on_hover_text(
                                "records and projects confirmed trades; does not start or stop L2 depth capture",
                            );
                        ui.add_enabled_ui(self.config.show_aggressions, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("cluster");
                                egui::ComboBox::from_id_salt("heatmap_bubble_cluster")
                                    .selected_text(cluster_label(self.config.bubble_cluster_ms))
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
                                            ui.selectable_value(
                                                &mut self.config.bubble_cluster_ms,
                                                milliseconds,
                                                label,
                                            );
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
                                    .selected_text(dust_label(self.config.bubble_dust_merge_ms))
                                    .show_ui(ui, |ui| {
                                        for milliseconds in [0, 500, 1_500, 3_000, 10_000] {
                                            ui.selectable_value(
                                                &mut self.config.bubble_dust_merge_ms,
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
                                    .selected_text(region_label(self.config.bubble_region_rows))
                                    .show_ui(ui, |ui| {
                                        for rows in [1, 2, 3, 4, 6, 8, 12] {
                                            ui.selectable_value(
                                                &mut self.config.bubble_region_rows,
                                                rows,
                                                region_label(rows),
                                            );
                                        }
                                    });
                                if self.config.bubble_region_rows > 1 {
                                    ui.label("window");
                                    egui::ComboBox::from_id_salt("heatmap_bubble_region_ms")
                                        .selected_text(region_window_label(
                                            self.config.bubble_region_ms,
                                        ))
                                        .show_ui(ui, |ui| {
                                            for milliseconds in [500, 1_000, 1_500, 2_000, 3_000, 5_000]
                                            {
                                                ui.selectable_value(
                                                    &mut self.config.bubble_region_ms,
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
                            ui.checkbox(
                                &mut self.config.bubble_candle_summary,
                                "summarize closed bars",
                            )
                            .on_hover_text(
                                "fold every print of a bar and price range into one bubble carrying both sides, drawn as a pie whose sectors are the buy/sell proportion. The forming bar included: its pie is a running total that grows with each order, so the compressed left side reports what is happening now instead of only what already happened. Quantities, ids and matched evidence are summed exactly, and the tape still shows those same prints one by one",
                            );
                            self.draw_live_lane_controls(ui);
                            self.draw_bubble_controls(ui);
                        });

                        ui.separator();
                        let health = &self.published.health;
                        // The projection carries clusters whether or not the
                        // bubble layer draws them — the live strip reads the
                        // same ones. Calling them "bubbles" while none is on
                        // screen would report a layer that is off.
                        let noun = if self.config.show_aggressions {
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
                        if ui
                            .button("reset bubble visuals")
                            .on_hover_text("only this tab; L2 and history settings stay as they are")
                            .clicked()
                        {
                            let defaults = HeatmapConfig::default();
                            self.config.bubble_cluster_ms = defaults.bubble_cluster_ms;
                            self.config.bubble_dust_merge_ms = defaults.bubble_dust_merge_ms;
                            self.config.bubble_candle_summary = defaults.bubble_candle_summary;
                            self.config.bubble_region_rows = defaults.bubble_region_rows;
                            self.config.bubble_region_ms = defaults.bubble_region_ms;
                            self.config.bubbles = defaults.bubbles;
                            self.config.live_lane = defaults.live_lane;
                            // No stored preset is on screen any more, so the
                            // picker must not keep claiming one.
                            self.presets.active.clear();
                            self.preset_status =
                                Some("bubble defaults restored (not saved)".to_owned());
                        }
                    });
        self.commit_config_changes(before)
    }
}

fn theme_label(theme: HeatmapTheme) -> &'static str {
    match theme {
        HeatmapTheme::Bookmap => "Bookmap",
        HeatmapTheme::HighContrast => "High contrast",
        HeatmapTheme::ColorBlind => "Color blind",
    }
}

fn display_grouping_label(grouping: DisplayGrouping) -> String {
    match grouping {
        DisplayGrouping::Native => "Native · 1×".to_owned(),
        DisplayGrouping::Multiple(multiple) => format!("Range · {multiple}×"),
        DisplayGrouping::Adaptive { target_rows } => format!("Auto · {target_rows} rows"),
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
