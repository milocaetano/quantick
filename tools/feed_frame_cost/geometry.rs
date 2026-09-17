// Untimed evidence from the actual final panel and its painted text shapes.
#[cfg(test)]
pub(crate) fn f2_panel_proof(ctx: &egui::Context, view: &DeliveryView, reference: bool) {
    if reference {
        let retained = ctx
            .data(|data| data.get_temp::<CachedGalleys>(egui::Id::new("feed.delivery.readout")))
            .expect("proof cache primed");
        paint_panel(ctx, retained);
    } else {
        draw_panel(ctx, &view.text());
    }
}

#[cfg(test)]
pub(crate) fn f2_panel_geometry(
    ctx: &egui::Context,
    output: &egui::FullOutput,
    view: &DeliveryView,
) -> serde_json::Value {
    fn bounds(r: egui::Rect) -> [f32; 4] {
        [r.min.x, r.min.y, r.max.x, r.max.y]
    }
    fn collect(
        shape: &egui::Shape,
        clip: egui::Rect,
        wanted: &[String; 3],
        rows: &mut Vec<serde_json::Value>,
    ) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, wanted, rows);
                }
            }
            egui::Shape::Text(text) if wanted.contains(&text.galley.job.text) => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                assert!(clip.contains_rect(rect), "delivery galley clipped");
                rows.push(serde_json::json!({"text":text.galley.job.text,"rect":bounds(rect),"clip":bounds(clip)}));
            }
            _ => {}
        }
    }
    let panel =
        egui::containers::panel::PanelState::load(ctx, egui::Id::new("feed_delivery_status"))
            .expect("actual panel state")
            .rect;
    let cache = ctx
        .data(|data| data.get_temp::<CachedGalleys>(egui::Id::new("feed.delivery.readout")))
        .expect("actual panel cache");
    let expected_height = cache.cells.iter().map(|cell| cell.size().y).sum::<f32>()
        + 2.0 * ctx.style().spacing.item_spacing.y
        + 8.0;
    // Panel layout rounds to physical pixels; retain exact values and bound only
    // the documented rounding quantum, not a performance tolerance.
    assert!((panel.height() - expected_height).abs() <= 1.0 / ctx.pixels_per_point());
    let wanted = view.text();
    let mut rows = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, shape.clip_rect, &wanted.0, &mut rows);
    }
    assert_eq!(rows.len(), 3, "exactly three actual delivery galleys");
    serde_json::json!({"panel":bounds(panel),"expected_height":expected_height,"galleys":rows})
}
