//! FLOW's runs as a drawing builds them, kept from the one before where their keys held.
use super::*;

/// `plan`'s runs, sectors then rims, kept from `previous` where their keys held.
pub(super) fn flow_runs(
    (frame, plan): (&FlowTapeFrame, &FlowPresentation),
    (background, backing): (egui::Color32, Option<egui::Color32>),
    tick_look: Option<quantick_orderflow::TickLook>,
    (painter, history): (&egui::Painter, egui::Rect),
    previous: Option<&flow::FlowRunCache<Arc<[egui::Mesh; 2]>>>,
    appearance: &crate::theme::Palette,
) -> flow::FlowRunCache<Arc<[egui::Mesh; 2]>> {
    let ppp = painter.ctx().pixels_per_point();
    let options = painter.ctx().options(|o| o.tessellation_options);
    let mut tessellator = egui::epaint::Tessellator::new(ppp, options, [1, 1], Vec::new());
    tessellator.set_clip_rect(history.intersect(painter.clip_rect()));
    let tick_style = tick_look.map(|(tick, _)| tick.bubbles());
    let tick_colors = tick_look.map(|(tick, theme)| {
        let mut style = quantick_orderflow::config::theme::OrderflowRenderStyle::default();
        style.resolve_palette(theme, &appearance.resolved.orderflow);
        super::super::bubbles::bubble_colors(
            &style.colors(super::super::premultiplied),
            &tick.bubbles(),
        )
    });
    let canvas = background.to_array();
    let underlay = backing.as_ref().map(egui::Color32::to_array);
    let keys = plan.region_keys(frame, tick_look.is_some());
    // A palette that rims its bubbles (the lit flow pane's) draws them flat.
    let flat = tick_colors.is_some_and(|colors| colors.flat_rims.is_some());
    let light = tick_style.as_ref().map_or(0.0, |_| SPHERE_LIGHT_OFFSET);
    let shade = |color| match &tick_style {
        Some(style) if flat => SphereShading::flat(color).faded(style.opacity),
        Some(style) => SphereShading::sphere(color, style).faded(style.opacity),
        None => SphereShading::flat(color),
    };
    let mut build = |regions: &[flow::FlowRegionVisual]| {
        let [mut mesh, mut rim] = [egui::Mesh::default(), egui::Mesh::default()];
        for region in regions {
            let disc = region.disc;
            let colors = match tick_colors {
                Some(colors) => [colors.buy, colors.sell],
                None => region
                    .regional_fill_with(frame, canvas, underlay, &appearance.resolved)
                    .map(super::premultiplied),
            };
            if let (Some(style), Some(colors)) = (&tick_style, tick_colors)
                && disc.radius >= style.detail_min_radius
            {
                let side = frame.dots[region.dot_index].dominant_side();
                let rings = colors
                    .edge(side, disc.radius)
                    .rings(disc.radius, colors.separator);
                for (radius, width, ink, alpha) in rings {
                    let stroke = egui::Stroke::new(width, ink.gamma_multiply(alpha));
                    let shape = egui::Shape::circle_stroke(disc.center.into(), radius, stroke);
                    tessellator.tessellate_shape(shape, &mut rim);
                }
            }
            for (side, angle, sweep) in region.sectors(frame, PIE_START_ANGLE) {
                let (center, radius, color) =
                    (disc.center.into(), disc.radius, shade(colors[side]));
                add_sector(&mut mesh, center, radius, angle, sweep, color, light);
            }
        }
        Arc::new([mesh, rim])
    };
    let empty = flow::FlowRunCache::default();
    let held = previous.unwrap_or(&empty);
    held.rebuild(keys, |cut| build(&plan.regions[cut]))
}
