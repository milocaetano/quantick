// The `orderflow_render.rs` unit tests, moved out of the file so a session
// opening the renderer no longer reads 1,878 lines of tests it did not ask
// for.
//
// They stay a child module of `crate::orderflow_render` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.

use super::*;

/// The dust threshold is defined by inverting this module's radius
/// mapping, but lives in `config` beside the style it reads. This pins the
/// two together: a print at the threshold must land exactly on the
/// readability floor.
#[test]
fn the_dust_threshold_lands_on_the_readability_floor() {
    use rust_decimal::prelude::ToPrimitive as _;

    let bubbles = BubbleStyle::default();
    let reference = rust_decimal::Decimal::from(400);
    let dust = bubbles
        .dust_quantity(reference)
        .expect("the default style has a readability floor above its minimum");
    let size = (dust / reference)
        .to_f32()
        .expect("the threshold share converts")
        .sqrt();
    let radius = bubble_radius(size, bubbles.min_radius, bubbles.max_radius);
    assert!(
        (radius - bubbles.readable_min_radius).abs() < 1e-3,
        "a dust print rendered at {radius}, not {}",
        bubbles.readable_min_radius
    );
}

/// The shipped presets tune `detail_min_radius` down to buy sphere shading
/// on small prints — "dense tape btc" (the default open) sets it *below*
/// `min_radius`. Anchoring the readability floor there made both the dust
/// merge and the hollow ring inert on exactly the look the project opens
/// with, which is the regression this guards.
#[test]
fn a_low_detail_radius_does_not_disarm_the_readability_floor() {
    let dense_tape_btc = BubbleStyle {
        min_radius: 2.2,
        max_radius: 14.0,
        detail_min_radius: 2.0,
        // Opt in explicitly: the ring is off by default now, and this test
        // is about the readability floor still arming it when the dressing
        // radius is set below the minimum.
        hollow_small_buys: true,
        ..BubbleStyle::default()
    };
    assert!(dense_tape_btc.detail_min_radius < dense_tape_btc.min_radius);
    assert!(
        dense_tape_btc
            .dust_quantity(rust_decimal::Decimal::from(400))
            .is_some(),
        "prints must still be foldable when the dressing radius is low"
    );

    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &dense_tape_btc);
    let mark = BubbleMark {
        center: egui::pos2(40.0, 40.0),
        radius: dense_tape_btc.min_radius,
        side: Side::Buy,
        size: 0.02,
        matched: None,
        buy_share: 1.0,
        folded: 0,
    };
    let solid = BubbleStyle {
        hollow_small_buys: false,
        ..dense_tape_btc.clone()
    };
    assert_ne!(
        painted(|painter| draw_bubble(painter, mark, &dense_tape_btc, &colors)),
        painted(|painter| draw_bubble(painter, mark, &solid, &colors)),
        "the ring must still fire when the dressing radius is below the minimum"
    );
}

#[test]
fn nothing_is_dust_without_a_reference_or_a_readability_floor() {
    let bubbles = BubbleStyle::default();
    assert!(bubbles.dust_quantity(rust_decimal::Decimal::ZERO).is_none());

    let flat = BubbleStyle {
        readable_min_radius: 0.0,
        ..BubbleStyle::default()
    };
    assert!(
        flat.dust_quantity(rust_decimal::Decimal::from(400))
            .is_none()
    );
}

fn luminance(rgb: [u8; 3]) -> f32 {
    0.2126 * f32::from(rgb[0]) + 0.7152 * f32::from(rgb[1]) + 0.0722 * f32::from(rgb[2])
}

#[test]
fn every_theme_moves_from_dark_to_bright() {
    for theme in [
        HeatmapTheme::Bookmap,
        HeatmapTheme::HighContrast,
        HeatmapTheme::ColorBlind,
    ] {
        let dark = thermal_rgb(theme, 0.0);
        let middle = thermal_rgb(theme, 0.55);
        let bright = thermal_rgb(theme, 1.0);
        assert!(
            luminance(dark) < luminance(middle),
            "{theme:?} dark={dark:?} middle={middle:?}",
        );
        assert!(
            luminance(middle) < luminance(bright),
            "{theme:?} middle={middle:?} bright={bright:?}",
        );
    }
}

#[test]
fn thermal_ramp_clamps_invalid_and_out_of_range_values() {
    assert_eq!(
        thermal_rgb(HeatmapTheme::Bookmap, -10.0),
        BOOKMAP_RAMP[0].rgb
    );
    assert_eq!(
        thermal_rgb(HeatmapTheme::Bookmap, 10.0),
        BOOKMAP_RAMP.last().unwrap().rgb
    );
    assert_eq!(
        thermal_rgb(HeatmapTheme::Bookmap, f32::NAN),
        BOOKMAP_RAMP[0].rgb
    );
}

#[test]
fn bookmap_ramp_spans_black_to_warm_white_through_green() {
    // The refined Bookmap ramp starts at pure black so quiet liquidity
    // fades into the canvas, and ends warm-white for the strongest walls.
    assert_eq!(thermal_rgb(HeatmapTheme::Bookmap, 0.0), [0, 0, 0]);
    let top = thermal_rgb(HeatmapTheme::Bookmap, 1.0);
    assert!(top.iter().all(|&channel| channel > 220), "top={top:?}");
    // It passes through a green phase (restored versus the older ramp, which
    // jumped cyan straight to yellow), so mid magnitudes stay separable.
    let mid_high = thermal_rgb(HeatmapTheme::Bookmap, 0.70);
    assert!(
        mid_high[1] > mid_high[0] && mid_high[1] > mid_high[2],
        "expected a green-dominant phase, got {mid_high:?}",
    );
}

#[test]
fn strong_walls_converge_to_same_brightness_on_both_sides() {
    for theme in [
        HeatmapTheme::Bookmap,
        HeatmapTheme::HighContrast,
        HeatmapTheme::ColorBlind,
    ] {
        assert_eq!(
            resting_rgb(theme, BookSide::Bid, 1.0),
            resting_rgb(theme, BookSide::Ask, 1.0)
        );
    }
}

#[test]
fn bubble_area_above_floor_tracks_normalized_quantity() {
    let minimum = 3.0;
    let maximum = 13.0;
    let quarter_quantity_radius = bubble_radius(0.5, minimum, maximum);
    let full_radius = bubble_radius(1.0, minimum, maximum);
    let quarter_area = quarter_quantity_radius.powi(2) - minimum.powi(2);
    let full_area = full_radius.powi(2) - minimum.powi(2);
    assert!((quarter_area / full_area - 0.25).abs() < 1e-5);
}

#[test]
fn partial_marker_height_grows_with_reduction_fraction() {
    let band = EventBand {
        x: 50.0,
        top: 10.0,
        bottom: 30.0,
    };
    let quiet = marker_band(band, 0.1, false);
    let strong = marker_band(band, 0.8, false);
    assert!(quiet.height() < strong.height());
    assert!(strong.height() < band.height());
    assert_eq!(marker_band(band, 0.2, true).height(), band.height());
}

#[test]
fn compact_legend_wraps_without_exceeding_width() {
    let widths = [90.0, 90.0, 90.0];
    let layout = flow_layout(&widths, 190.0, 17.0, 3.0);
    assert_eq!(layout.positions[0], egui::vec2(0.0, 0.0));
    assert_eq!(layout.positions[1], egui::vec2(93.0, 0.0));
    assert_eq!(layout.positions[2], egui::vec2(0.0, 17.0));
    assert!(layout.size.x <= 190.0);
    assert_eq!(layout.size.y, 34.0);
}

#[test]
fn labels_are_honest_and_compact() {
    assert_eq!(
        bubble_label(Decimal::from(1_250), 4, 0, true, true),
        Some("1.25K · ×4".to_owned())
    );
    assert_eq!(format_quantity(Decimal::from(100)), "100");
    assert_eq!(format_quantity(Decimal::ZERO), "0");
    assert_eq!(
        bubble_label(Decimal::ONE, 1, 0, false, true),
        None,
        "one trade does not need a redundant count"
    );
}

/// The stretch older than this session's capture used to be a hatched,
/// tinted block covering a third of the chart. It is now its boundary and
/// nothing else, so the candles and bubbles recorded there stay readable.
#[test]
fn the_pre_capture_span_is_marked_by_its_boundary_alone() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 200.0));
    let leading = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(120.0, 200.0));
    assert_eq!(
        gap_marks(leading, chart, true),
        GapMarks {
            fill: false,
            left_boundary: false,
            right_boundary: true,
        }
    );

    // A chart the bars do not fill starts the span at the oldest bar
    // instead of at the viewport edge. Still one line: there is nothing to
    // the left of it to separate the span from.
    let inset = egui::Rect::from_min_max(egui::pos2(60.0, 0.0), egui::pos2(120.0, 200.0));
    assert_eq!(
        gap_marks(inset, chart, true),
        GapMarks {
            fill: false,
            left_boundary: false,
            right_boundary: true,
        }
    );
}

/// An interior gap is a handful of pixels wide. Without a fill it would
/// read as an empty book rather than as missing data.
#[test]
fn an_interior_gap_keeps_its_fill_and_both_boundaries() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 200.0));
    let interior = egui::Rect::from_min_max(egui::pos2(180.0, 0.0), egui::pos2(186.0, 200.0));
    assert_eq!(
        gap_marks(interior, chart, false),
        GapMarks {
            fill: true,
            left_boundary: true,
            right_boundary: true,
        }
    );
}

/// Nothing captured at all: both bounds are the viewport, so no line is
/// drawn — framing the whole chart would say nothing the label does not.
#[test]
fn a_gap_spanning_the_whole_chart_draws_no_boundary() {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 200.0));
    assert_eq!(
        gap_marks(chart, chart, true),
        GapMarks {
            fill: false,
            left_boundary: false,
            right_boundary: false,
        }
    );
}

#[test]
fn render_style_sanitizes_non_finite_geometry() {
    let style = OrderflowRenderStyle {
        heat_opacity: f32::NAN,
        min_cell_height: f32::INFINITY,
        edge_glow: -1.0,
        bubbles: BubbleStyle {
            min_radius: f32::NAN,
            max_radius: -4.0,
            label_min_radius: f32::NAN,
            ..BubbleStyle::default()
        },
        legend_max_width: f32::NAN,
        ..OrderflowRenderStyle::default()
    }
    .sanitized();
    assert_eq!(style.heat_opacity, 1.0);
    assert_eq!(style.min_cell_height, 1.5);
    assert_eq!(style.edge_glow, 0.0);
    assert!(style.bubbles.max_radius >= style.bubbles.min_radius);
    assert!(style.bubbles.label_min_radius.is_finite());
    assert!(style.legend_max_width.is_finite());
}

#[test]
fn buy_and_sell_bubbles_are_nudged_to_opposite_sides() {
    // Screen y grows downward: buys sit above the print, sells below.
    assert!(side_offset_y(Side::Buy, 4.0, false) < 0.0);
    assert!(side_offset_y(Side::Sell, 4.0, false) > 0.0);
    assert_eq!(side_offset_y(Side::Buy, 0.0, false), 0.0);
    assert_eq!(
        side_offset_y(Side::Buy, 4.0, false).abs(),
        side_offset_y(Side::Sell, 4.0, false).abs()
    );
    // The nudge names a book side, not a screen side: upside down it
    // mirrors with the chart.
    assert!(side_offset_y(Side::Buy, 4.0, true) > 0.0);
    assert!(side_offset_y(Side::Sell, 4.0, true) < 0.0);
}

/// Paint through `draw` off-screen and return the shapes it emitted.
fn painted(draw: impl Fn(&egui::Painter)) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        draw(&ctx.layer_painter(egui::LayerId::background()));
    });
    format!("{:?}", output.shapes)
}

/// The cheap-dot path is the fps contract on a dense tape: below the
/// dressing radius a solid print stays exactly one filled circle — no
/// halo, no rim, and no separator ring may sneak onto it.
#[test]
fn a_cheap_dot_stays_a_single_circle() {
    let bubbles = BubbleStyle {
        min_radius: 2.0,
        detail_min_radius: 6.0,
        hollow_small_buys: false,
        ..BubbleStyle::default()
    };
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
    let shapes = painted(|painter| {
        draw_bubble(
            painter,
            BubbleMark {
                center: egui::pos2(50.0, 50.0),
                radius: 3.0,
                side: Side::Sell,
                size: 0.1,
                matched: None,
                buy_share: 0.0,
                folded: 0,
            },
            &bubbles,
            &colors,
        )
    });
    assert_eq!(
        shapes.matches("CircleShape").count(),
        1,
        "a cheap dot must stay one circle: {shapes}"
    );
}

#[test]
fn the_preview_draws_a_bubble_exactly_the_way_the_chart_does() {
    // The preview is the instrument the user tunes the sliders against, so
    // it must not render its own approximation. Both paths go through
    // draw_bubble; with the trail off (the one mark the chart batches
    // separately) they must emit identical shapes.
    let bubbles = BubbleStyle {
        trail_length: 0.0,
        ..BubbleStyle::default()
    };
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
    let at = egui::pos2(120.0, 80.0);
    let radius = bubble_radius(
        PREVIEW_LARGE_PRINT_SIZE,
        bubbles.min_radius,
        bubbles.max_radius,
    );

    let live = painted(|painter| {
        draw_bubble(
            painter,
            BubbleMark {
                center: at + egui::vec2(0.0, side_offset_y(Side::Buy, bubbles.side_offset, false)),
                radius,
                side: Side::Buy,
                size: PREVIEW_LARGE_PRINT_SIZE,
                matched: Some(PREVIEW_MATCHED_FRACTION),
                buy_share: 1.0,
                folded: 0,
            },
            &bubbles,
            &colors,
        );
    });
    let preview = painted(|painter| {
        draw_preview_bubble(
            painter,
            PreviewBubble {
                center: at,
                size: PREVIEW_LARGE_PRINT_SIZE,
                side: Side::Buy,
                linked_reduction: true,
                buy_share: 1.0,
            },
            f32::INFINITY,
            &bubbles,
            &colors,
        );
    });
    assert!(
        live.contains("Circle"),
        "the sample must actually draw a bubble: {live}"
    );
    assert_eq!(live, preview);
}

#[test]
fn bubble_marks_scale_with_size_and_matched_share() {
    let bubbles = BubbleStyle::default();
    // The front grows with the radius, and never collapses to nothing on
    // the smallest bubble.
    assert!(front_half_length(10.0, &bubbles) > front_half_length(2.0, &bubbles));
    assert!(front_half_length(0.0, &bubbles) >= FRONT_END_PADDING_PX);
    // A sweep haloes brighter than a routine print, and alpha stays legal.
    assert!(halo_alpha(1.0, &bubbles) > halo_alpha(0.0, &bubbles));
    assert!(halo_alpha(1.0, &bubbles) <= 1.0);
    assert_eq!(
        halo_alpha(0.0, &bubbles),
        bubbles.halo_strength,
        "an unsized print gets the plain halo"
    );
    assert!(
        halo_alpha(f32::NAN, &bubbles).is_finite(),
        "a non-finite size must not poison the alpha"
    );
    // The ring brightens with the share of the print that matched, from a
    // floor that keeps a nibble visible.
    assert!(impact_ring_alpha(1.0) > impact_ring_alpha(0.0));
    assert!(impact_ring_alpha(0.0) >= IMPACT_RING_BASE_ALPHA);
    assert!(impact_ring_alpha(1.0) <= 1.0);
}

#[test]
fn bubble_colours_fall_back_to_the_theme_and_the_trail_follows_the_front() {
    let palette = Palette::for_theme(HeatmapTheme::Bookmap);
    let default = BubbleColors::resolve(&palette, &BubbleStyle::default());
    assert_eq!(default.buy, palette.buy);
    assert_eq!(default.sell, palette.sell);
    assert_eq!(default.trail, palette.consumption);

    let overridden = BubbleColors::resolve(
        &palette,
        &BubbleStyle {
            buy_color: Some([1, 2, 3]),
            front_color: Some([9, 9, 9]),
            ..BubbleStyle::default()
        },
    );
    assert_eq!(overridden.buy, egui::Color32::from_rgb(1, 2, 3));
    assert_eq!(
        overridden.sell, palette.sell,
        "untouched side keeps the theme"
    );
    assert_eq!(
        overridden.trail,
        egui::Color32::from_rgb(9, 9, 9),
        "the trail follows the front colour unless overridden itself"
    );
}

#[test]
fn sphere_colours_brighten_the_core_and_darken_the_rim() {
    let color = egui::Color32::from_rgb(40, 200, 120);
    let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
    assert!(luminance(rgb(sphere_core_color(color, 0.35))) > luminance(rgb(color)));
    assert!(luminance(rgb(sphere_edge_color(color, 0.55))) < luminance(rgb(color)));
    // Zero strength is the identity, so "sphere with no shading" degrades
    // honestly into the flat colour instead of some third look.
    assert_eq!(sphere_core_color(color, 0.0), color);
    assert_eq!(sphere_edge_color(color, 0.0), color);
    assert_eq!(sphere_edge_color(color, 1.0), egui::Color32::BLACK);
}

#[test]
fn a_sphere_disc_is_a_bounded_two_ring_fan() {
    let mut mesh = egui::Mesh::default();
    let center = egui::pos2(50.0, 50.0);
    let radius = 10.0;
    let full = std::f32::consts::TAU;
    let shading = SphereShading {
        core: egui::Color32::WHITE,
        body: egui::Color32::GRAY,
        edge: egui::Color32::BLACK,
    };
    add_shaded_sector(&mut mesh, center, radius, 0.0, full, shading);
    let segments = sphere_segments(radius);
    // One vertex more per ring than the wrapping fan needed: an arc has two
    // ends, and a whole circle is the arc whose ends coincide.
    assert_eq!(mesh.vertices.len(), 1 + 2 * (segments + 1));
    assert_eq!(mesh.indices.len(), segments * 9);
    for vertex in &mesh.vertices {
        assert!(
            (vertex.pos - center).length() <= radius + 0.001,
            "shading must stay inside the bubble: {:?}",
            vertex.pos
        );
    }

    // Degenerate geometry appends nothing rather than poisoning the mesh.
    let before = mesh.vertices.len();
    for (center, radius, sweep) in [
        (egui::pos2(f32::NAN, 0.0), radius, full),
        (center, 0.0, full),
        (center, radius, 0.0),
        (center, radius, f32::NAN),
    ] {
        add_shaded_sector(&mut mesh, center, radius, 0.0, sweep, shading);
    }
    assert_eq!(mesh.vertices.len(), before);
}

/// A pie is two wedges, and each one stays a wedge: bounded by the radius,
/// anchored on the shared centre, and cheaper than a whole disc.
#[test]
fn a_sector_covers_only_its_own_slice() {
    let center = egui::pos2(50.0, 50.0);
    let radius = 12.0;
    let shading = SphereShading::flat(egui::Color32::GRAY);
    let mut quarter = egui::Mesh::default();
    add_shaded_sector(
        &mut quarter,
        center,
        radius,
        PIE_START_ANGLE,
        std::f32::consts::FRAC_PI_2,
        shading,
    );
    let mut whole = egui::Mesh::default();
    add_shaded_sector(
        &mut whole,
        center,
        radius,
        PIE_START_ANGLE,
        std::f32::consts::TAU,
        shading,
    );
    assert!(quarter.vertices.len() < whole.vertices.len());
    // Straight up and to the right of centre: the quarter starting at
    // twelve o'clock sweeps clockwise into exactly that quadrant.
    assert!(
        quarter.vertices.iter().all(|vertex| vertex.pos.x
            >= center.x - radius * SPHERE_LIGHT_OFFSET - 0.001
            && vertex.pos.y <= center.y + 0.001),
        "a quarter must not paint the other three: {:?}",
        quarter.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()
    );
}

#[test]
fn the_crown_never_touches_the_disc_and_never_closes_a_circle() {
    // The two properties the mark exists for. The first is why it replaced
    // the vertical front: a bubble's area is its quantity, so nothing may
    // be drawn over it. The second is why it is an arc and not a ring: a
    // closed circle concentric with the disc makes the disc's own edge
    // ambiguous, which is what the impact ring did.
    for radius in [1.0_f32, 2.2, 3.5, 5.7, 9.3, 15.0, 22.0, 48.0] {
        for matched in [0.0_f32, 0.1, 0.5, 0.99, 1.0] {
            let geometry = crown_geometry(radius, matched);
            assert!(
                geometry.arc_radius - geometry.width / 2.0 > radius,
                "at r={radius} m={matched} the crown's inner edge \
                     {} is not clear of the rim",
                geometry.arc_radius - geometry.width / 2.0
            );
            assert!(
                geometry.sweep <= GOLDEN_ANGLE + 1e-5,
                "at r={radius} m={matched} the sweep {} exceeds the golden angle",
                geometry.sweep
            );
            assert!(
                geometry.sweep >= GOLDEN_ANGLE * INV_PHI_2 - 1e-5,
                "a print that ate anything still shows a mark"
            );
        }
    }
}

#[test]
fn the_crown_grows_with_the_matched_share() {
    // Arc length is the channel a trader reads ordinally without a
    // reference beside it, so it has to be monotone in what it encodes.
    let radius = 12.0;
    let mut previous = f32::NEG_INFINITY;
    for matched in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let length = crown_geometry(radius, matched).arc_length();
        assert!(
            length > previous,
            "matched={matched} must draw a longer arc than the share below it"
        );
        previous = length;
    }
    // A full sweep reaches the golden angle exactly, and a nibble 1/φ² of it.
    assert!((crown_geometry(radius, 1.0).sweep - GOLDEN_ANGLE).abs() < 1e-5);
    assert!((crown_geometry(radius, 0.0).sweep - GOLDEN_ANGLE * INV_PHI_2).abs() < 1e-5);
}

#[test]
fn the_crown_replaces_the_front_and_leaves_the_disc_alone() {
    let bubbles = BubbleStyle::default();
    assert_eq!(bubbles.consumption_mark, ConsumptionMark::Crown);
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
    let mark = BubbleMark {
        center: egui::pos2(60.0, 60.0),
        radius: 10.0,
        side: Side::Buy,
        size: 0.6,
        matched: Some(0.7),
        buy_share: 1.0,
        folded: 0,
    };
    let crowned = painted(|painter| draw_bubble(painter, mark, &bubbles, &colors));
    let fronted = painted(|painter| {
        draw_bubble(
            painter,
            mark,
            &BubbleStyle {
                consumption_mark: ConsumptionMark::Front,
                ..bubbles.clone()
            },
            &colors,
        )
    });
    assert_ne!(crowned, fronted, "the mark must change what is painted");
    assert!(
        fronted.contains("LineSegment"),
        "the front is still the line it always was: {fronted}"
    );
    assert!(
        !crowned.contains("LineSegment"),
        "the crown draws no line through the bubble: {crowned}"
    );

    // A print that ate nothing wears no crown at all.
    let untouched = painted(|painter| {
        draw_bubble(
            painter,
            BubbleMark {
                matched: None,
                ..mark
            },
            &bubbles,
            &colors,
        )
    });
    assert!(untouched.len() < crowned.len());

    // The third variant is a port, not a placeholder: asking for no mark
    // must paint exactly what a print that ate nothing paints, so the
    // consumption signal can be switched off without also changing the
    // disc. Without this the arm is only reachable through the panel.
    let silent = painted(|painter| {
        draw_bubble(
            painter,
            mark,
            &BubbleStyle {
                consumption_mark: ConsumptionMark::None,
                ..bubbles.clone()
            },
            &colors,
        )
    });
    assert_eq!(
        silent, untouched,
        "ConsumptionMark::None must leave the bubble exactly as it was"
    );
}

#[test]
fn a_crown_follows_its_own_side_unless_the_panel_overrode_it() {
    let palette = Palette::for_theme(HeatmapTheme::Bookmap);
    let bubbles = BubbleStyle::default();
    let colors = BubbleColors::resolve(&palette, &bubbles);
    // Derived from the side colour, and brighter than it: consumption is
    // the same event, hotter — no third hue enters the canvas.
    for (side, base) in [(Side::Buy, colors.buy), (Side::Sell, colors.sell)] {
        let crown = colors.crown_for_side(side);
        assert_ne!(crown, base);
        assert!(
            crown.r() >= base.r() && crown.g() >= base.g() && crown.b() >= base.b(),
            "the crown is the side colour pushed toward white, not away from it"
        );
    }
    assert_ne!(
        colors.crown_for_side(Side::Buy),
        colors.crown_for_side(Side::Sell)
    );

    // The consumption colour override stays the one door to change it.
    let overridden = BubbleColors::resolve(
        &palette,
        &BubbleStyle {
            front_color: Some([10, 20, 30]),
            ..bubbles
        },
    );
    assert_eq!(
        overridden.crown_for_side(Side::Buy),
        egui::Color32::from_rgb(10, 20, 30)
    );
}

#[test]
fn sphere_mode_swaps_the_flat_fill_for_a_shaded_mesh() {
    let mark = BubbleMark {
        center: egui::pos2(120.0, 80.0),
        radius: 12.0,
        side: Side::Buy,
        size: 0.8,
        matched: None,
        buy_share: 1.0,
        folded: 0,
    };
    let palette = Palette::for_theme(HeatmapTheme::Bookmap);
    // Both modes are named explicitly: the shipped default is the sphere
    // now, and a test comparing the two must not depend on which one that
    // happens to be.
    let flat_style = BubbleStyle {
        render_mode: BubbleRenderMode::Flat,
        ..BubbleStyle::default()
    };
    let sphere_style = BubbleStyle {
        render_mode: BubbleRenderMode::Sphere,
        ..BubbleStyle::default()
    };
    let colors = BubbleColors::resolve(&palette, &flat_style);

    let flat = painted(|painter| draw_bubble(painter, mark, &flat_style, &colors));
    let sphere = painted(|painter| draw_bubble(painter, mark, &sphere_style, &colors));
    assert_ne!(flat, sphere, "the mode must change what is painted");
    assert!(
        sphere.contains("Mesh"),
        "sphere mode paints a vertex-shaded mesh: {sphere}"
    );

    // Below the detail floor both modes paint the same cheap dot, keeping
    // the tessellation budget flat on a fast tape.
    let dot = BubbleMark {
        radius: flat_style.detail_min_radius - 1.0,
        ..mark
    };
    let flat_dot = painted(|painter| draw_bubble(painter, dot, &flat_style, &colors));
    let sphere_dot = painted(|painter| draw_bubble(painter, dot, &sphere_style, &colors));
    assert_eq!(flat_dot, sphere_dot);
}

#[test]
fn the_preview_draws_a_sphere_bubble_exactly_the_way_the_chart_does() {
    // Same contract as the flat parity test: the preview must not render
    // its own approximation of the sphere look.
    let bubbles = BubbleStyle {
        render_mode: BubbleRenderMode::Sphere,
        trail_length: 0.0,
        ..BubbleStyle::default()
    };
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
    let at = egui::pos2(120.0, 80.0);
    let radius = bubble_radius(
        PREVIEW_LARGE_PRINT_SIZE,
        bubbles.min_radius,
        bubbles.max_radius,
    );

    let live = painted(|painter| {
        draw_bubble(
            painter,
            BubbleMark {
                center: at + egui::vec2(0.0, side_offset_y(Side::Buy, bubbles.side_offset, false)),
                radius,
                side: Side::Buy,
                size: PREVIEW_LARGE_PRINT_SIZE,
                matched: Some(PREVIEW_MATCHED_FRACTION),
                buy_share: 1.0,
                folded: 0,
            },
            &bubbles,
            &colors,
        );
    });
    let preview = painted(|painter| {
        draw_preview_bubble(
            painter,
            PreviewBubble {
                center: at,
                size: PREVIEW_LARGE_PRINT_SIZE,
                side: Side::Buy,
                linked_reduction: true,
                buy_share: 1.0,
            },
            f32::INFINITY,
            &bubbles,
            &colors,
        );
    });
    assert!(
        live.contains("Mesh"),
        "the sample must shade a sphere: {live}"
    );
    assert_eq!(live, preview);
}

#[test]
fn hollow_small_buys_opens_the_dot_and_leaves_dressed_bubbles_alone() {
    // The ring is off by default now; this test is about the knob still
    // doing what it says for anyone who turns it back on.
    let hollow = BubbleStyle {
        trail_length: 0.0,
        hollow_small_buys: true,
        ..BubbleStyle::default()
    };
    let solid = BubbleStyle {
        hollow_small_buys: false,
        ..hollow.clone()
    };
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &hollow);
    let mark = |radius| BubbleMark {
        center: egui::pos2(40.0, 40.0),
        radius,
        side: Side::Buy,
        size: 0.05,
        matched: None,
        buy_share: 1.0,
        folded: 0,
    };

    // Below the readability floor — where colour alone stops working —
    // the setting must change what is painted.
    let small = hollow.min_radius;
    assert!(small < hollow.readable_min_radius);
    assert_ne!(
        painted(|painter| draw_bubble(painter, mark(small), &hollow, &colors)),
        painted(|painter| draw_bubble(painter, mark(small), &solid, &colors)),
        "a buy below the floor must not paint the same with the ring off"
    );

    // Above it the setting must change nothing: at that size the fill and
    // its sphere shading already say which side the bubble is.
    let big = hollow.readable_min_radius + 1.0;
    assert_eq!(
        painted(|painter| draw_bubble(painter, mark(big), &hollow, &colors)),
        painted(|painter| draw_bubble(painter, mark(big), &solid, &colors)),
        "a buy above the floor must be untouched by the ring setting"
    );
}

/// Orientation flips the price fraction and nothing else: y mirrors
/// around the canvas's middle, x — time — never moves.
#[test]
fn an_inverted_layout_mirrors_normalized_y() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 2, 0, 2, 0.0);
    let flipped = layout.with_inverted(true);
    assert_eq!(layout.y(0.25), 100.0);
    assert_eq!(flipped.y(0.25), 300.0);
    assert_eq!(flipped.x(0.5), layout.x(0.5), "time never turns over");
}

/// The key starts below whatever already owns the canvas's top-left
/// corner. It used to clear a constant 22 px — the chart header alone —
/// and printed straight through the indicator chips stacked under it.
#[test]
fn the_legend_starts_below_the_corner_it_was_told_about() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 2, 0, 2, 0.0);
    let projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            rust_decimal::Decimal::ONE,
            rust_decimal::Decimal::from(100),
        ),
    );

    let top_of_key = |inset: f32| {
        let style = OrderflowRenderStyle {
            legend_top_inset: inset,
            ..OrderflowRenderStyle::default()
        };
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let context = RenderContext::new(&projection, layout, &style);
            draw_compact_legend(&ctx.layer_painter(egui::LayerId::background()), &context);
        });
        let mut top = f32::INFINITY;
        for shape in output.shapes {
            let rect = shape.shape.visual_bounding_rect();
            if rect.is_positive() {
                top = top.min(rect.top());
            }
        }
        top
    };

    let header_only = top_of_key(LEGEND_HEADER_CLEARANCE_PX);
    let with_two_chips = top_of_key(LEGEND_HEADER_CLEARANCE_PX + 60.0);
    assert!(
        header_only.is_finite() && with_two_chips.is_finite(),
        "the key's panel has to be measurable: {header_only} / {with_two_chips}"
    );
    assert!(
        with_two_chips - header_only >= 59.0,
        "a taller corner has to push the key down by the same amount: \
             {header_only} → {with_two_chips}"
    );
    // And a caller that measured nothing still clears the header.
    assert!(top_of_key(0.0) >= rect.top() + LEGEND_HEADER_CLEARANCE_PX);

    // Past half the canvas the corner belongs to whatever is stacked
    // there. The key stands down instead of printing over it — chrome
    // yields, and nothing it names stops being drawn.
    assert!(
        !top_of_key(rect.height() * MAX_LEGEND_TOP_INSET_FRAC + 1.0).is_finite(),
        "the key must draw nothing when the corner is full"
    );
}

/// The legend is a key for what is on screen: exactly one entry per layer
/// that is both active as a family and switched on individually.
#[test]
fn the_legend_lists_only_the_layers_that_are_on() {
    let labels = |style: &OrderflowRenderStyle| -> Vec<String> {
        legend_entries(style, "liquidity".to_owned())
            .into_iter()
            .map(|(_, label)| label)
            .collect()
    };

    let all = OrderflowRenderStyle::default();
    assert_eq!(
        labels(&all),
        [
            "liquidity",
            "buy aggression",
            "sell aggression",
            "aggression-aligned depletion",
            "L2 reduction (unattributed)",
            "L2 gap",
        ]
    );

    let mut some = all.clone();
    some.show_liquidity = false;
    some.show_sell = false;
    some.show_unattributed = false;
    assert_eq!(
        labels(&some),
        ["buy aggression", "aggression-aligned depletion", "L2 gap"]
    );

    // Family switches still trump the per-layer ones: without L2 capture
    // no depth entry may appear, whatever its individual flag says. The
    // family is now both panes — the key describes the canvas, not one
    // pane of it.
    let mut bubbles_only = all.clone();
    bubbles_only.depth_layer = false;
    bubbles_only.lane_depth_layer = false;
    assert_eq!(labels(&bubbles_only), ["buy aggression", "sell aggression"]);

    // A layer the candles have switched off but the tape still draws keeps
    // its key: withholding it would deny a mark that is on screen.
    let mut tape_only = all.clone();
    tape_only.depth_layer = false;
    tape_only.aggression_layer = false;
    assert_eq!(
        labels(&tape_only),
        labels(&all),
        "the tape alone still earns every key"
    );

    let mut nothing = all;
    nothing.depth_layer = false;
    nothing.aggression_layer = false;
    nothing.lane_depth_layer = false;
    nothing.lane_aggression_layer = false;
    assert!(labels(&nothing).is_empty());
}

/// A two-sided bubble is a pie: both side colours on one mark, and the
/// proportion is what the sectors carry. Where a pie cannot be read the
/// mark falls back to exactly the dot it has always been.
#[test]
fn a_two_sided_bubble_draws_both_sides_and_a_small_one_falls_back() {
    let bubbles = BubbleStyle {
        min_radius: 2.0,
        max_radius: 20.0,
        detail_min_radius: 6.0,
        hollow_small_buys: false,
        render_mode: BubbleRenderMode::Flat,
        ..BubbleStyle::default()
    };
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
    let mark = |radius: f32, buy_share: f32| BubbleMark {
        center: egui::pos2(60.0, 60.0),
        radius,
        side: Side::Buy,
        size: 0.6,
        matched: None,
        buy_share,
        folded: 0,
    };
    // Compared after the fill alpha, which is what actually lands in the
    // mesh vertices.
    let ink = |color: egui::Color32| format!("{:?}", color.gamma_multiply(bubbles.opacity));

    // A dressed pie paints both colours into one mesh.
    let pie = painted(|painter| draw_bubble(painter, mark(12.0, 0.4), &bubbles, &colors));
    assert!(pie.contains("Mesh"), "a pie is a mesh: {pie}");
    assert!(
        pie.contains(&ink(colors.buy)) && pie.contains(&ink(colors.sell)),
        "both sides must be inked: {pie}"
    );

    // A single-sided bubble is untouched by the pie path: it still takes
    // the cheap flat circle it always did.
    let solid = painted(|painter| draw_bubble(painter, mark(12.0, 1.0), &bubbles, &colors));
    assert!(
        !solid.contains("Mesh"),
        "a plain bubble stays flat: {solid}"
    );
    assert!(!solid.contains(&ink(colors.sell)));

    // Below the dressing radius the pie is unreadable, so the mark returns
    // to one dot in the dominant side's colour.
    let dot = painted(|painter| {
        draw_bubble(
            painter,
            mark(bubbles.detail_min_radius - 1.0, 0.4),
            &bubbles,
            &colors,
        )
    });
    assert_eq!(
        dot.matches("CircleShape").count(),
        1,
        "a mixed dot must stay one circle: {dot}"
    );
    assert!(!dot.contains(&ink(colors.sell)));
}

/// The presets the user actually runs push the dressing radius *below* the
/// minimum to buy sphere shading on small bubbles, which makes every mark
/// "dressed". The pie must not ride in on that: it needs the dedicated
/// readability floor too, or a summarized bar turns into a rash of
/// two-tone specks nobody can read a proportion from.
#[test]
fn a_pie_needs_the_readability_floor_on_the_shipped_presets() {
    // "dense tape btc" — the project's default open — as shipped.
    let dense_tape_btc = BubbleStyle {
        min_radius: 2.2,
        max_radius: 14.0,
        detail_min_radius: 2.0,
        readable_min_radius: quantick_orderflow::config::DEFAULT_READABLE_MIN_RADIUS,
        hollow_small_buys: true,
        render_mode: BubbleRenderMode::Sphere,
        ..BubbleStyle::default()
    };
    assert!(dense_tape_btc.detail_min_radius < dense_tape_btc.min_radius);
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &dense_tape_btc);
    let mark = |radius: f32| BubbleMark {
        center: egui::pos2(60.0, 60.0),
        radius,
        side: Side::Buy,
        size: 0.5,
        matched: None,
        buy_share: 0.5,
        folded: 0,
    };
    let sell_ink = format!("{:?}", colors.sell.gamma_multiply(dense_tape_btc.opacity));

    // At the smallest drawn radius every bubble is "dressed" here, and the
    // mark must still be one speck of one colour.
    let speck = painted(|painter| {
        draw_bubble(
            painter,
            mark(dense_tape_btc.min_radius),
            &dense_tape_btc,
            &colors,
        )
    });
    assert!(
        !speck.contains(&sell_ink),
        "a speck must not try to be a pie: {speck}"
    );

    // Past the readability floor the proportion is worth drawing.
    let readable = painted(|painter| {
        draw_bubble(
            painter,
            mark(dense_tape_btc.readable_min_radius + 1.0),
            &dense_tape_btc,
            &colors,
        )
    });
    assert!(readable.contains(&sell_ink), "a readable pie: {readable}");
}

/// Hiding the bubble layer hides bubbles — it does not empty the frame.
///
/// The clusters are a fact more than one surface reads: the bubbles, the
/// consumption carve behind them, and the live strip's histogram beside
/// the price axis. The projection used to apply these switches, so turning
/// the bubbles off blanked the strip with them ("se eu desativar as bolhas
/// de agressão, quero continuar vendo essa parte"). The filter belongs
/// here, one step before the ink.
#[test]
fn hiding_the_bubble_layer_keeps_the_clusters_in_the_frame() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(600.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 2, 0, 2, 0.0);
    let mut projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            rust_decimal::Decimal::ONE,
            rust_decimal::Decimal::from(100),
        ),
    );
    for (agg_id, side, x) in [(1_u64, Side::Buy, 0.25_f64), (2, Side::Sell, 0.75)] {
        projection.aggressions.push(AggressionPrimitive {
            agg_id,
            agg_ids: vec![agg_id],
            generation: None,
            side,
            consumed_side: match side {
                Side::Buy => BookSide::Ask,
                Side::Sell => BookSide::Bid,
            },
            quantity: rust_decimal::Decimal::ONE,
            buy_share: match side {
                Side::Buy => 1.0,
                Side::Sell => 0.0,
            },
            live: false,
            price_bucket: rust_decimal::Decimal::ONE,
            price_span: rust_decimal::Decimal::ONE,
            trade_count: 1,
            first_timestamp_ms: 0,
            last_timestamp_ms: 0,
            matched_quantity: rust_decimal::Decimal::ZERO,
            matched_fraction: 0.0,
            liquidity_event_ids: Vec::new(),
            x,
            y: 0.5,
            size: 1.0,
            folded_marks: 0,
        });
    }

    let drawn = |style: &OrderflowRenderStyle| {
        RenderContext::new(&projection, layout, style)
            .bubbles()
            .map(|mark| mark.agg_id)
            .collect::<Vec<_>>()
    };

    let both = OrderflowRenderStyle::default();
    assert_eq!(drawn(&both), vec![1, 2]);

    let mut buys_hidden = both.clone();
    buys_hidden.show_buy = false;
    assert_eq!(drawn(&buys_hidden), vec![2]);

    let mut layer_off = both.clone();
    layer_off.aggression_layer = false;
    assert!(drawn(&layer_off).is_empty(), "no bubble is drawn");
    // …and the frame the other surfaces read is untouched: this is the
    // whole point of moving the switch out of the projection.
    assert_eq!(projection.aggressions.len(), 2);
    // The strip builds its histogram from exactly these clusters, so it
    // still has both prints with the bubble layer off.
    let rows = crate::live_strip::aggression_rows(
        &projection.aggressions,
        0,
        projection.summarized,
        projection.effective_grouping.bucket_width,
    );
    assert_eq!(rows.len(), 1, "both prints share one bucket");
    assert_eq!(rows[0].buy, rust_decimal::Decimal::ONE);
    assert_eq!(rows[0].sell, rust_decimal::Decimal::ONE);

    // Nothing draws over the canvas either — the bubble pass is silent.
    let painted_with_layer_off = painted(|painter| {
        draw_aggression_bubbles(
            painter,
            &RenderContext::new(&projection, layout, &layer_off),
        );
    });
    assert!(
        !painted_with_layer_off.contains("Circle"),
        "no circle with the layer off: {painted_with_layer_off}"
    );
}

/// The lane's radius multiplier has to survive all the way to the circle
/// that gets drawn, and it must touch nothing outside the lane.
#[test]
fn the_lane_scale_reaches_the_bubbles_and_stops_at_the_boundary() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 4, 0, 4, 5.0);
    let style = OrderflowRenderStyle {
        bubbles: BubbleStyle {
            min_radius: 2.0,
            max_radius: 10.0,
            detail_min_radius: 100.0, // cheap dots only: one circle each
            hollow_small_buys: false,
            halo_strength: 0.0,
            trail_length: 0.0,
            show_quantity_labels: false,
            show_trade_count: false,
            ..BubbleStyle::default()
        },
        live_lane: LiveLaneStyle {
            radius_scale: 2.0,
            ..LiveLaneStyle::default()
        },
        ..OrderflowRenderStyle::default()
    };

    let radii = |live: bool| {
        let mut projection = HeatmapProjection::empty(
            true,
            quantick_orderflow::EffectiveGrouping::resolve(
                quantick_orderflow::DisplayGrouping::Native,
                rust_decimal::Decimal::ONE,
                rust_decimal::Decimal::from(100),
            ),
        );
        projection.aggressions.push(AggressionPrimitive {
            agg_id: 1,
            agg_ids: vec![1],
            generation: None,
            side: Side::Buy,
            consumed_side: BookSide::Ask,
            quantity: rust_decimal::Decimal::ONE,
            buy_share: 1.0,
            live,
            price_bucket: rust_decimal::Decimal::ONE,
            price_span: rust_decimal::Decimal::ONE,
            trade_count: 1,
            first_timestamp_ms: 0,
            last_timestamp_ms: 0,
            matched_quantity: rust_decimal::Decimal::ZERO,
            matched_fraction: 0.0,
            liquidity_event_ids: Vec::new(),
            x: 0.5,
            y: 0.5,
            size: 1.0,
            folded_marks: 0,
        });
        painted(|painter| {
            draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, &style));
        })
    };

    // At full size the radius is the configured maximum, doubled inside
    // the lane and untouched outside it.
    let history = radii(false);
    let lane = radii(true);
    assert!(
        history.contains("radius: 10.0"),
        "history radius: {history}"
    );
    assert!(lane.contains("radius: 20.0"), "lane radius: {lane}");
}

/// A bubble is a disc: keeping its centre in its pane is not enough, since
/// a fat radius beside the divider still spills across it. Each print is
/// clipped to the pane it belongs to, so the two charts never draw into
/// each other — "os gráficos estão penetrando um no outro".
#[test]
fn a_bubble_beside_the_divider_is_clipped_to_its_own_pane() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    // Three bar slots plus a 300 px lane: the divider lands on x = 700.
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let divider = layout.lane_left_x().expect("a lane has a boundary");
    let style = OrderflowRenderStyle {
        bubbles: BubbleStyle {
            min_radius: 20.0,
            max_radius: 20.0,
            detail_min_radius: 100.0, // cheap dots: one circle per print
            hollow_small_buys: false,
            halo_strength: 0.0,
            trail_length: 0.0,
            show_quantity_labels: false,
            show_trade_count: false,
            ..BubbleStyle::default()
        },
        ..OrderflowRenderStyle::default()
    };

    let clipped = |x: f64, live: bool| {
        let mut projection = HeatmapProjection::empty(
            true,
            quantick_orderflow::EffectiveGrouping::resolve(
                quantick_orderflow::DisplayGrouping::Native,
                rust_decimal::Decimal::ONE,
                rust_decimal::Decimal::from(100),
            ),
        );
        projection.aggressions.push(AggressionPrimitive {
            agg_id: 1,
            agg_ids: vec![1],
            generation: None,
            side: Side::Buy,
            consumed_side: BookSide::Ask,
            quantity: rust_decimal::Decimal::ONE,
            buy_share: 1.0,
            live,
            price_bucket: rust_decimal::Decimal::ONE,
            price_span: rust_decimal::Decimal::ONE,
            trade_count: 1,
            first_timestamp_ms: 0,
            last_timestamp_ms: 0,
            matched_quantity: rust_decimal::Decimal::ZERO,
            matched_fraction: 0.0,
            liquidity_event_ids: Vec::new(),
            x,
            y: 0.5,
            size: 1.0,
            folded_marks: 0,
        });
        painted(|painter| {
            draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, &style));
        })
    };

    // The last instant before the lane opens: a print of the candles, drawn
    // hard against the divider with a radius that would reach well past it.
    let history = clipped(0.749, false);
    assert!(
        history.contains(&format!("{:?}", layout.history_rect())),
        "a candle-pane print must carry the candle pane's clip: {history}"
    );
    assert!(
        !history.contains(&format!("{:?}", layout.chart_rect)),
        "and never the whole chart's: {history}"
    );
    // ...and the tape's first print is clipped the other way.
    let lane = clipped(0.751, true);
    assert!(
        lane.contains(&format!("{:?}", layout.lane_rect())),
        "a tape print must carry the tape's clip: {lane}"
    );
    assert!(layout.history_rect().right() <= divider);
    assert!(layout.lane_rect().left() >= divider);
}

/// The candles and the tape are switched apart: clearing a layer on one
/// pane leaves the other drawing exactly what it drew. This is the pixel
/// half of the promise — the config half is
/// `hiding_a_layer_on_one_pane_leaves_the_other_drawing_and_fed`.
#[test]
fn a_layer_switched_off_on_one_pane_still_draws_on_the_other() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    // Three bar slots plus a 300 px lane: the divider lands on x = 700.
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);

    // One print on each pane, from the same projection, so the only thing
    // that can separate them is the switch under test.
    let mut projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            rust_decimal::Decimal::ONE,
            rust_decimal::Decimal::from(100),
        ),
    );
    for (x, live) in [(0.4_f64, false), (0.9_f64, true)] {
        projection.aggressions.push(AggressionPrimitive {
            agg_id: 1,
            agg_ids: vec![1],
            generation: None,
            side: Side::Buy,
            consumed_side: BookSide::Ask,
            quantity: rust_decimal::Decimal::ONE,
            buy_share: 1.0,
            live,
            price_bucket: rust_decimal::Decimal::ONE,
            price_span: rust_decimal::Decimal::ONE,
            trade_count: 1,
            first_timestamp_ms: 0,
            last_timestamp_ms: 0,
            matched_quantity: rust_decimal::Decimal::ZERO,
            matched_fraction: 0.0,
            liquidity_event_ids: Vec::new(),
            x,
            y: 0.5,
            size: 1.0,
            folded_marks: 0,
        });
    }

    let drawn = |chart: bool, lane: bool| {
        let style = OrderflowRenderStyle {
            aggression_layer: chart,
            lane_aggression_layer: lane,
            bubbles: BubbleStyle {
                min_radius: 20.0,
                max_radius: 20.0,
                detail_min_radius: 100.0, // cheap dots: one circle per print
                hollow_small_buys: false,
                halo_strength: 0.0,
                trail_length: 0.0,
                show_quantity_labels: false,
                show_trade_count: false,
                ..BubbleStyle::default()
            },
            ..OrderflowRenderStyle::default()
        };
        let context = RenderContext::new(&projection, layout, &style);
        let counted = context.bubbles().count();
        (
            counted,
            painted(|painter| draw_aggression_bubbles(painter, &context)),
        )
    };

    let (both, _) = drawn(true, true);
    assert_eq!(both, 2, "with both panes on, both prints draw");

    // The candles are cleared. The tape's print survives — the whole ask.
    let (tape_only, marks) = drawn(false, true);
    assert_eq!(
        tape_only, 1,
        "the tape keeps drawing with the candles clear"
    );
    assert!(
        marks.contains(&format!("{:?}", layout.lane_rect())),
        "and it is the tape's print that survived: {marks}"
    );

    // And the other way round.
    let (chart_only, marks) = drawn(true, false);
    assert_eq!(chart_only, 1);
    assert!(
        marks.contains(&format!("{:?}", layout.history_rect())),
        "the candle-pane print is the one left: {marks}"
    );

    assert_eq!(drawn(false, false).0, 0, "both off draws nothing");
}

/// The depth map is switched per pane by the region it may paint, not by
/// dropping cells: a resting level that has been there since before the
/// tape's window opened is one continuous band across the divider, and
/// hiding the map over the candles has to cut it there rather than lose it.
#[test]
fn the_depth_map_is_cut_at_the_divider_rather_than_dropped() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let divider = layout.lane_left_x().expect("a lane has a boundary");

    assert_eq!(layout.layer_clip(true, true), Some(layout.chart_rect));
    assert_eq!(layout.layer_clip(true, false), Some(layout.history_rect()));
    assert_eq!(layout.layer_clip(false, true), Some(layout.lane_rect()));
    assert_eq!(layout.layer_clip(false, false), None);
    // The two regions meet at the divider and neither reaches past it, so
    // a band crossing it is cut, never doubled or dropped.
    assert!(layout.layer_clip(true, false).unwrap().right() <= divider);
    assert!(layout.layer_clip(false, true).unwrap().left() >= divider);

    // A canvas with no tape has one pane, and it is the candles'. The
    // lane's switch cannot blank a chart that has no lane.
    let laneless = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 0.0);
    assert!(laneless.lane_left_x().is_none());
    assert_eq!(laneless.layer_clip(true, false), Some(laneless.chart_rect));
    assert_eq!(laneless.layer_clip(true, true), Some(laneless.chart_rect));
    assert_eq!(laneless.layer_clip(false, true), None);
}

/// The two sides never both get the side nudge: an even split sits on the
/// exact price, and the lean grows continuously from there.
#[test]
fn a_pie_leans_with_its_buy_share() {
    let offset = 4.0;
    let lean = |buy_share: f32| -((finite_unit(buy_share) - 0.5) * 2.0) * offset;
    assert_eq!(lean(1.0), side_offset_y(Side::Buy, offset, false));
    assert_eq!(lean(0.0), side_offset_y(Side::Sell, offset, false));
    assert_eq!(lean(0.5), 0.0);
    assert!(lean(0.75) < 0.0 && lean(0.75) > lean(1.0));
}

/// The lane is a pane of its own: a fixed band on the right edge of the
/// chart, with the candles' pane ending exactly where it opens.
#[test]
fn the_lane_is_a_fixed_band_on_the_right_edge() {
    let viewport = Viewport::new(); // candle_width 8, following
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
    // Four regions: three bar slots and the lane, 300 px wide.
    let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 4, 300.0);
    let boundary = layout.lane_left_x().expect("a lane has a boundary");
    assert!(
        (boundary - 700.0).abs() < 0.01,
        "the band is the last 300 px"
    );

    // Bars keep their candle width, and the last one ends at the divider.
    let bar_width = (layout.x(1.0 / 4.0) - layout.x(0.0)).abs();
    assert!((bar_width - viewport.candle_width()).abs() < 0.01);
    assert!(
        (layout.x(3.0 / 4.0) - boundary).abs() < 0.01,
        "the candles' pane ends exactly where the lane opens"
    );
    // ...and the lane spans the band, whatever the candles are worth.
    assert!((layout.x(1.0) - rect.right()).abs() < 0.01);

    let no_lane = ProjectedLayout::new(rect, &viewport, 4, 0, 4, 0.0);
    assert_eq!(no_lane.lane_left_x(), None);
    let flat_width = (no_lane.x(1.0) - no_lane.x(3.0 / 4.0)).abs();
    assert!((flat_width - viewport.candle_width()).abs() < 0.01);
    assert!((no_lane.x(1.0) - rect.right()).abs() < 0.01);
}

/// The point of the pane: every chart movement leaves the tape alone.
#[test]
fn panning_and_zooming_the_candles_never_move_the_lane() {
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
    let mut panned = Viewport::new();
    panned.pan_pixels(240.0, 60); // 30 candles into history
    let mut zoomed = Viewport::new();
    zoomed.zoom(4.0);

    let still = Viewport::new();
    let reference = ProjectedLayout::new(rect, &still, 60, 0, 5, 300.0);
    for viewport in [&panned, &zoomed] {
        let moved = ProjectedLayout::new(rect, viewport, 60, 0, 5, 300.0);
        assert_eq!(moved.lane_left_x(), reference.lane_left_x());
        // Same instant on the tape, same pixel — however the candles moved.
        for position in [0.85_f64, 0.9, 1.0] {
            assert!(
                (moved.x(position) - reference.x(position)).abs() < 0.01,
                "the tape moved with the candles at {position}"
            );
        }
    }
}

/// The lane never draws marks it cannot place, and hiding them is exactly
/// one switch away.
#[test]
fn the_lane_marks_need_a_lane_a_live_edge_and_permission() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
    let style = OrderflowRenderStyle::default();
    let mut projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            rust_decimal::Decimal::ONE,
            rust_decimal::Decimal::from(100),
        ),
    );
    projection.live_now_x = Some(0.95);

    let with_lane = ProjectedLayout::new(rect, &viewport, 4, 0, 4, 5.0);
    let drawn = painted(|painter| {
        draw_live_lane_marks(painter, &RenderContext::new(&projection, with_lane, &style));
    });
    assert!(
        drawn.matches("LineSegment").count() >= 2,
        "both the boundary and the live-time line must draw: {drawn}"
    );

    // No live edge: the frame is history, and history has no present.
    let mut settled = projection.clone();
    settled.live_now_x = None;
    // No lane at all: nothing to divide.
    let no_lane = ProjectedLayout::new(rect, &viewport, 4, 0, 4, 0.0);
    // Switched off by the user.
    let hidden = OrderflowRenderStyle {
        live_lane: LiveLaneStyle {
            show_marks: false,
            ..LiveLaneStyle::default()
        },
        ..OrderflowRenderStyle::default()
    };
    let nothing = painted(|_| {});
    for (frame, layout, style) in [
        (&settled, with_lane, &style),
        (&projection, no_lane, &style),
        (&projection, with_lane, &hidden),
    ] {
        assert_eq!(
            painted(|painter| draw_live_lane_marks(
                painter,
                &RenderContext::new(frame, layout, style)
            )),
            nothing
        );
    }
}

#[test]
fn the_lane_is_the_only_region_wider_than_a_candle() {
    let viewport = Viewport::new(); // candle_width 8, following
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
    // 3 regions: 2 bar slots and the lane, 32 px wide — four candles' worth
    // at this zoom, and still 32 px at any other.
    let layout = ProjectedLayout::new(rect, &viewport, 2, 0, 3, 32.0);
    let closed_w = (layout.x(1.0 / 3.0) - layout.x(0.0)).abs();
    let live_w = (layout.x(1.0) - layout.x(2.0 / 3.0)).abs();
    assert!(closed_w > 0.0);
    assert!(
        (live_w - closed_w * 4.0).abs() < 0.01,
        "the lane should be 4x a bar slot: bar={closed_w} lane={live_w}"
    );
}

/// A bar panned off the right of its own pane scrolls out of sight instead
/// of being drawn over the tape.
#[test]
fn a_candle_panned_behind_the_tape_is_clipped_to_its_own_pane() {
    let mut viewport = Viewport::new();
    viewport.pan_pixels(80.0, 20); // 10 candles into history
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
    let layout = ProjectedLayout::new(rect, &viewport, 20, 0, 21, 300.0);
    let divider = layout.lane_left_x().expect("a lane has a boundary");

    // The newest bar is now right of the divider, and its pane stops there.
    let newest = 19.5 / 21.0;
    assert!(layout.x(newest) > divider);
    assert!(layout.pane(newest).right() <= divider + 0.01);
    // A band entirely past the divider is clipped away to nothing...
    assert!(!layout.band(newest, newest, 0.1, 0.2, 1.0).is_positive());
    // ...while the tape's own pane starts there and reaches the edge.
    assert!((layout.pane(1.0).left() - divider).abs() < 0.01);
    assert!((layout.pane(1.0).right() - rect.right()).abs() < 0.01);
}
/// A folded bubble does not look like a print.
///
/// The budget merges marks instead of discarding them, so nothing a trader
/// needs is ever missing — but a merged bubble carries a quantity that
/// never crossed the tape at once. Sizing a position off it as if it had
/// is the exact harm the fold was introduced to avoid, so the two must be
/// distinguishable on the canvas and not only in a settings panel.
#[test]
fn a_folded_bubble_wears_a_ring_a_print_does_not() {
    let bubbles = BubbleStyle::default();
    let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
    let mark = BubbleMark {
        center: egui::pos2(40.0, 40.0),
        radius: 10.0,
        side: Side::Buy,
        size: 0.5,
        matched: None,
        buy_share: 1.0,
        folded: 0,
    };
    let print = painted(|painter| draw_bubble(painter, mark, &bubbles, &colors));
    let fold = painted(|painter| {
        draw_bubble(painter, BubbleMark { folded: 4, ..mark }, &bubbles, &colors)
    });
    assert_ne!(
        print, fold,
        "a fold of four marks painted exactly what one print paints"
    );
    assert!(
        fold.len() > print.len(),
        "the fold has to add ink, not swap it"
    );
    // The count itself is the label's business now (`⊕4` against a
    // cluster's `×4`); what `draw_bubble` owes is the ring, which is what
    // the dots too small for text have to rely on.

    // At dot size the count will not fit, and the ring alone has to carry
    // the statement — the part a trader must not miss is "more than one".
    let dot = BubbleMark {
        radius: 3.0,
        ..mark
    };
    let dot_print = painted(|painter| draw_bubble(painter, dot, &bubbles, &colors));
    let dot_fold =
        painted(|painter| draw_bubble(painter, BubbleMark { folded: 2, ..dot }, &bubbles, &colors));
    assert_ne!(
        dot_print, dot_fold,
        "a folded dot is indistinguishable from a single print"
    );
}
/// A fold and a cluster may not read the same.
///
/// `×4` says four prints happened together at one price — a fact about the
/// market, and a size a trader may act on. `⊕4` says the frame put four
/// marks together to fit its budget — a fact about the canvas, and a size
/// that never crossed the tape at once. Sharing a glyph would let the
/// second be read as the first.
#[test]
fn a_fold_and_a_cluster_do_not_share_a_glyph() {
    let cluster =
        bubble_label(rust_decimal::Decimal::from(20), 4, 0, true, true).expect("labels are on");
    let fold =
        bubble_label(rust_decimal::Decimal::from(20), 4, 4, true, true).expect("labels are on");
    assert_eq!(cluster, "20 · ×4");
    assert_eq!(fold, "20 · ⊕4");
    assert_ne!(
        cluster, fold,
        "a budget fold reads as four prints that traded"
    );
}
/// A reduction kind switched off leaves the canvas, not just the legend.
///
/// The trader's report: unchecking "L2 reduction (unattributed)" took the
/// entry out of the legend and left every violet mark painting. The
/// projection filters those events by threshold and never by choice - both
/// kinds are factual and the history stays complete - so the renderer is
/// the only place the switch can be honoured, and it was not asking. The
/// legend then said the layer was off while the trader looked straight at
/// it, which is the data-honesty rule inverted.
#[test]
fn the_legend_and_the_canvas_agree_about_a_reduction_kind() {
    // The legend already honoured both switches; the canvas did not. Pin
    // them to the same two flags so they cannot drift apart again.
    let entries = |aligned: bool, unattributed: bool| {
        let style = OrderflowRenderStyle {
            depth_layer: true,
            aggression_layer: true,
            show_aligned: aligned,
            show_unattributed: unattributed,
            ..OrderflowRenderStyle::default()
        };
        legend_entries(&style, "liquidity".to_owned())
            .into_iter()
            .map(|(_, label)| label)
            .collect::<Vec<_>>()
    };
    let both = entries(true, true);
    assert!(both.iter().any(|l| l.contains("unattributed")));
    assert!(both.iter().any(|l| l.contains("aggression-aligned")));

    let aligned_only = entries(true, false);
    assert!(
        !aligned_only.iter().any(|l| l.contains("unattributed")),
        "the legend drops the entry"
    );
    assert!(
        aligned_only
            .iter()
            .any(|l| l.contains("aggression-aligned"))
    );
}

/// Switching the book off clears the marks that describe it, on the pane
/// that lost it — and switching one kind off clears that kind everywhere.
///
/// Both were broken and both looked the same from the chair: the trader
/// unticked "L2 reduction (unattributed)", the legend dropped the entry,
/// and every violet mark kept painting. `draw_liquidity_events` walked the
/// projection and drew each event without ever reading a switch — the
/// projection filters these by *threshold* and never by choice, since both
/// kinds are factual and the retained history stays complete, so the
/// renderer was the only place left to ask and it was not asking.
///
/// The pane half matters just as much: the two maps switch apart, so
/// "the book is off" is a question per canvas. Answering it with "is a book
/// drawn anywhere" left the candles violet whenever the tape still had one.
#[test]
fn a_reduction_leaves_the_canvas_with_the_book_that_explains_it() {
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    // A lane 200 px wide: everything past x = 600 belongs to the tape.
    let layout = ProjectedLayout::new(rect, &viewport, 2, 0, 2, 200.0);
    let mut projection = HeatmapProjection::empty(
        true,
        quantick_orderflow::EffectiveGrouping::resolve(
            quantick_orderflow::DisplayGrouping::Native,
            rust_decimal::Decimal::ONE,
            rust_decimal::Decimal::from(100),
        ),
    );
    let event = |event_id: u64, x: f64, evidence: LiquidityEvidence| {
        quantick_orderflow::LiquidityEventPrimitive {
            event_id,
            generation: 1,
            side: quantick_orderbook::BookSide::Bid,
            price_bucket: rust_decimal::Decimal::from(100),
            timestamp_ms: 0,
            before: rust_decimal::Decimal::from(10),
            after: rust_decimal::Decimal::ZERO,
            removed: rust_decimal::Decimal::from(10),
            fraction: 1.0,
            full_removal: true,
            matched_quantity: rust_decimal::Decimal::ZERO,
            matched_fraction: 0.0,
            evidence,
            x,
            y0: 0.4,
            y1: 0.6,
        }
    };
    // One of each kind on the candles, one of each on the tape.
    projection.liquidity_events = vec![
        event(1, 0.2, LiquidityEvidence::DepthOnly),
        event(2, 0.3, LiquidityEvidence::AggressionAligned),
        event(3, 0.9, LiquidityEvidence::DepthOnly),
        event(4, 0.95, LiquidityEvidence::AggressionAligned),
    ];

    let ink = |style: &OrderflowRenderStyle| {
        painted(|painter| {
            draw_liquidity_events(painter, &RenderContext::new(&projection, layout, style))
        })
    };
    let both_maps = OrderflowRenderStyle {
        depth_layer: true,
        lane_depth_layer: true,
        ..OrderflowRenderStyle::default()
    };
    let all = ink(&both_maps);
    assert!(all.len() > 200, "the baseline has to actually paint");

    // The candles lose their map, the tape keeps its own.
    let candles_clear = ink(&OrderflowRenderStyle {
        depth_layer: false,
        ..both_maps.clone()
    });
    assert!(
        candles_clear.len() < all.len(),
        "removing the candles' book removed no ink"
    );
    let tape_clear = ink(&OrderflowRenderStyle {
        lane_depth_layer: false,
        ..both_maps.clone()
    });
    assert!(
        tape_clear.len() < all.len(),
        "removing the tape's book removed no ink"
    );

    // No book on either canvas: nothing the book explains is painted, and
    // the trader never had to visit four switches to get there.
    let no_book = ink(&OrderflowRenderStyle {
        depth_layer: false,
        lane_depth_layer: false,
        ..both_maps.clone()
    });
    assert_eq!(
        no_book,
        painted(|_| {}),
        "the book is off everywhere and something still paints"
    );

    // And one kind off, with both maps on, takes that kind alone.
    let unattributed_off = ink(&OrderflowRenderStyle {
        show_unattributed: false,
        ..both_maps.clone()
    });
    assert!(
        unattributed_off.len() < all.len(),
        "unticking the unattributed reductions painted them anyway"
    );
    let neither_kind = ink(&OrderflowRenderStyle {
        show_unattributed: false,
        show_aligned: false,
        ..both_maps.clone()
    });
    assert_eq!(
        neither_kind,
        painted(|_| {}),
        "both kinds off and something still paints"
    );
}
