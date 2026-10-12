//! The Fib tools' window half: the Levels tab both mount in the inspector.
//! The tools themselves are headless, in `quantick_drawings`.

mod levels_tab;

pub(super) use levels_tab::{draw_levels_tab, remember_drawing_color};

#[cfg(test)]
mod tests {
    use eframe::egui;
    use quantick_drawings::{
        FIB_EXTENSION_DOTS, FIB_EXTENSION_ICON, FIB_EXTENSION_LETTER, FIB_RETRACEMENT_DOTS,
        FIB_RETRACEMENT_ICON, FIB_RETRACEMENT_LETTER,
    };

    /// Whether a stroke of `half_width` laid between `a` and `b` reaches
    /// into `rect` — the separating-axis test for a capsule against a box,
    /// conservative at the corners, which is the right side to be wrong on
    /// for a guard.
    fn stroke_touches(a: egui::Pos2, b: egui::Pos2, half_width: f32, rect: egui::Rect) -> bool {
        let grown = rect.expand(half_width);
        let outside_all = |pick: fn(egui::Pos2) -> f32, bound: f32, above: bool| {
            let (pa, pb) = (pick(a), pick(b));
            if above {
                pa > bound && pb > bound
            } else {
                pa < bound && pb < bound
            }
        };
        if outside_all(|p| p.x, grown.left(), false)
            || outside_all(|p| p.x, grown.right(), true)
            || outside_all(|p| p.y, grown.top(), false)
            || outside_all(|p| p.y, grown.bottom(), true)
        {
            return false;
        }
        let along = b - a;
        let normal = egui::vec2(-along.y, along.x);
        if normal.length() <= f32::EPSILON {
            return true;
        }
        let normal = normal.normalized();
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        for corner in [
            grown.left_top(),
            grown.right_top(),
            grown.left_bottom(),
            grown.right_bottom(),
        ] {
            let side = (corner - a).dot(normal);
            lowest = lowest.min(side);
            highest = highest.max(side);
        }
        !(lowest > 0.0 || highest < 0.0)
    }

    /// The letter is readable only because nothing else is painted where it
    /// lands — and *painted* is the word that matters. An earlier guard
    /// compared the icon's stroke and anchor coordinates against a margin,
    /// which is the geometry's centre line: a dot is drawn as a disc a tenth
    /// of the box wide, so an anchor whose centre cleared the margin still
    /// put ink a long way over it.
    ///
    /// This one measures what the painter measures: egui lays out the real
    /// glyph at the size the icon asks for, the dots and strokes are grown
    /// by the radius and width `paint_vector_icon` gives them, and the two
    /// are held apart. Both sizes the icon is painted at are checked,
    /// because the stroke width is in pixels and does not shrink with the
    /// box — the flyout's smaller icon is the tighter fit, not the safer one.
    #[test]
    fn nothing_the_fib_icons_paint_reaches_the_letter_beside_it() {
        let ctx = egui::Context::default();
        // Fonts are built on the first frame; `layout_no_wrap` before one has
        // nothing to measure with.
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        for box_px in [
            crate::widgets::TOOLRAIL_ICON.glyph,
            crate::toolrail::FLYOUT_ICON_BOX_PX,
        ] {
            let at = |(x, y): (f32, f32)| egui::pos2(x * box_px, y * box_px);
            let dot_radius = box_px * crate::widgets::ICON_DOT_RADIUS_RATIO;
            let stroke_half = crate::widgets::ICON_STROKE_WIDTH_PX / 2.0;
            for (name, strokes, dots, letter) in [
                (
                    "retracement",
                    FIB_RETRACEMENT_ICON,
                    FIB_RETRACEMENT_DOTS,
                    FIB_RETRACEMENT_LETTER,
                ),
                (
                    "extension",
                    FIB_EXTENSION_ICON,
                    FIB_EXTENSION_DOTS,
                    FIB_EXTENSION_LETTER,
                ),
            ] {
                let galley = ctx.fonts(|fonts| {
                    fonts.layout_no_wrap(
                        letter.text.to_owned(),
                        egui::FontId::proportional(box_px * letter.height),
                        egui::Color32::WHITE,
                    )
                });
                let letter_rect = egui::Rect::from_center_size(at(letter.at), galley.size());
                for polyline in strokes {
                    for pair in polyline.windows(2) {
                        assert!(
                            !stroke_touches(at(pair[0]), at(pair[1]), stroke_half, letter_rect),
                            "{name}: at {box_px} px the stroke {:?}-{:?} runs into the letter",
                            pair[0],
                            pair[1]
                        );
                    }
                }
                for dot in dots {
                    assert!(
                        !letter_rect.expand(dot_radius).contains(at(*dot)),
                        "{name}: at {box_px} px the anchor {dot:?} lands on the letter"
                    );
                }
                // The glyph box is not a clip rect — the button's hit target
                // is what the neighbouring buttons leave free — so that is
                // what the letter has to stay inside of.
                let margin = (crate::widgets::TOOLRAIL_ICON.hit - box_px) / 2.0;
                let button = egui::Rect::from_min_size(
                    egui::pos2(-margin, -margin),
                    egui::Vec2::splat(box_px + 2.0 * margin),
                );
                assert!(
                    button.contains_rect(letter_rect),
                    "{name}: at {box_px} px the letter spills out of its button"
                );
            }
        }
    }
}
