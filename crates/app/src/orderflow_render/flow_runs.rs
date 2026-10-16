//! FLOW's runs as a drawing builds them, kept from the one before where their keys held.
use super::*;

/// `plan`'s runs, sectors then rims, kept from `previous` where their keys held.
pub(super) fn flow_runs(
    (frame, plan): (&FlowTapeFrame, &FlowPresentation),
    (background, backing): (egui::Color32, Option<egui::Color32>),
    tick_look: Option<quantick_orderflow::TickLook>,
    (painter, history): (&egui::Painter, egui::Rect),
    previous: Option<&flow::FlowRunCache<Arc<[egui::Mesh; 2]>>>,
) -> flow::FlowRunCache<Arc<[egui::Mesh; 2]>> {
    let ppp = painter.ctx().pixels_per_point();
    let options = painter.ctx().options(|o| o.tessellation_options);
    let mut tessellator = egui::epaint::Tessellator::new(ppp, options, [1, 1], Vec::new());
    tessellator.set_clip_rect(history.intersect(painter.clip_rect()));
    let tick = tick_look.map(|(tick, _)| tick);
    let tick_style = tick.map(quantick_orderflow::TickBubbleStyle::bubbles);
    let tick_colors = tick_look.map(|(tick, theme)| {
        BubbleColors::resolve(&super::palette_for_theme(theme), &tick.bubbles())
    });
    let canvas = background.to_array();
    let underlay = backing.as_ref().map(egui::Color32::to_array);
    let keys = plan.region_keys(frame, tick.is_some());
    let hair_color = egui::Color32::from_black_alpha(super::bubbles::SEPARATOR_RING_ALPHA);
    let light = tick_style.as_ref().map_or(0.0, |_| SPHERE_LIGHT_OFFSET);
    let shade = |color| match &tick_style {
        Some(style) => SphereShading::sphere(color, style).faded(style.opacity),
        None => SphereShading::flat(color),
    };
    let mut build = |regions: &[flow::FlowRegionVisual]| {
        let [mut mesh, mut rim] = [egui::Mesh::default(), egui::Mesh::default()];
        for region in regions {
            let dot = &frame.dots[region.dot_index];
            let disc = region.disc;
            let [buy_color, sell_color] = match tick_colors {
                Some(colors) => [colors.buy, colors.sell],
                None => region
                    .regional_fill(frame, canvas, underlay)
                    .map(super::premultiplied),
            };
            let (buy, sell) = dot.side_shares();
            if let Some(style) = &tick_style
                && disc.radius >= style.detail_min_radius
            {
                let hair = super::bubbles::separator_ring_width(disc.radius);
                tessellator.tessellate_shape(
                    egui::Shape::circle_stroke(
                        disc.center.into(),
                        disc.radius + hair / 2.0,
                        egui::Stroke::new(hair, hair_color),
                    ),
                    &mut rim,
                );
            }
            let mut angle = f64::from(PIE_START_ANGLE);
            for (share, color) in [(buy, buy_color), (sell, sell_color)] {
                if share <= 0.0 {
                    continue;
                }
                // Exact sides reach geometry directly; no cached share or readability cutoff.
                let sweep = share * std::f64::consts::TAU;
                add_sector(
                    &mut mesh,
                    disc.center.into(),
                    disc.radius,
                    angle as f32,
                    sweep as f32,
                    shade(color),
                    light,
                );
                angle += sweep;
            }
        }
        Arc::new([mesh, rim])
    };
    let empty = flow::FlowRunCache::default();
    let held = previous.unwrap_or(&empty);
    held.rebuild(keys, |cut| build(&plan.regions[cut]))
}
