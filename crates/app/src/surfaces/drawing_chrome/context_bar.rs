//! The selected object's context bar.
//!
//! This is what a selection raises — one row of icons, where the object is,
//! for the handful of things a trader does with their eye on the chart.
//! Everything else stayed behind the gear. The bar re-uses
//! [`super::apply_actions`] verbatim, so lock, delete, hide and the undo
//! coalescing have one implementation and cannot drift between the two hosts.

use eframe::egui;

use super::{
    DrawingChromeAsk, DrawingChromeSurface, DrawingEnv, InspectorActions, clamp_into_chart,
};
use crate::drawings::{self, DrawingAuthor};

/// The bar's own state: the widget's, plus the rectangle a test reads back.
#[derive(Default)]
pub(crate) struct ContextBarState {
    pub bar: drawings::context_bar::ContextBar,
    #[cfg(test)]
    pub last_drawn_rect: Option<egui::Rect>,
}

/// The rectangle the context bar may occupy: the pane, with the live lane
/// taken off its right edge.
///
/// The lane is where the price the trader is reading is being formed, and
/// losing sight of it is the trader's first veto — `context_bar::place` has
/// kept clear of it since it was written. A *parked* bar is placed by a
/// different rule and has to be held to the same one, so both go through this
/// rectangle rather than through the pane's own.
///
/// Never narrower than the bar itself: a lane wider than the history area
/// would otherwise leave nothing to place in, and a pane's own edge is a
/// better answer than an empty rectangle.
pub(crate) fn bounds(chart: egui::Rect, right_limit: f32, size: egui::Vec2) -> egui::Rect {
    let right = right_limit
        .min(chart.right())
        .max(chart.left() + size.x)
        .min(chart.right());
    egui::Rect::from_min_max(chart.min, egui::pos2(right, chart.bottom()))
}

/// Draw this frame.
pub(crate) fn draw(
    chrome: &mut DrawingChromeSurface,
    ctx: &egui::Context,
    env: &DrawingEnv<'_>,
) -> DrawingChromeAsk {
    let mut ask = DrawingChromeAsk::default();
    let selection = env.selected_index();
    if chrome.bar.bar.note_selection(selection) {
        // A new object is a new question: the panel the last one opened does
        // not follow the selection around.
        chrome.shared.open = false;
    }
    // …except when the object just placed asked for it. Applied after the
    // reset above, because the placement that made the request also made the
    // selection change that clears it.
    if std::mem::take(&mut chrome.pending_open_settings) {
        chrome.shared.open = true;
        chrome.shared.last_selection = None;
    }
    let Some(selected) = env.selected.as_ref() else {
        return ask;
    };
    let index = selected.index;
    // An armed tool means the trader is drawing, not editing. The bar is
    // opaque to the pointer, so leaving it up would let it eat the click that
    // places the next object — the selection it belongs to is the one they
    // just finished, not the one they are starting.
    if env.drawing_tool_armed {
        return ask;
    }
    // Which gestures hide the bar is decided here, once, from the raw input —
    // not at each of the six call sites that could move the world. A *click*
    // never suppresses: it is how the trader reaches the bar. Only a decided
    // drag does, and only when it began outside the bar, so dragging the grip
    // does not hide what is being dragged.
    //
    // Note what is deliberately absent: the market moving. A drawing carried
    // along by the auto-scroll carries the bar with it, smoothly —
    // suppressing that would blink the bar all session on a live tape.
    let now_ms = (ctx.input(|input| input.time) * 1000.0) as u64;
    let bar_rect = chrome.bar.bar.last_rect();
    let (dragging, origin, zoomed, screen) = ctx.input(|input| {
        (
            input.pointer.is_decidedly_dragging(),
            input.pointer.press_origin(),
            input.raw_scroll_delta.y.abs() > f32::EPSILON
                || (input.zoom_delta() - 1.0).abs() > f32::EPSILON,
            input.screen_rect,
        )
    });
    // Against the rect the press *landed* on, not this frame's — the bar
    // moves with a grip drag, so comparing against the moved rect makes the
    // origin fall outside after ~20 px and suppresses the very gesture that
    // is moving it. The grip is the escape hatch for a bar sitting over
    // something the trader needs to see; it has to survive being used.
    let on_the_bar = matches!(
        (origin, chrome.bar.bar.press_rect(origin, bar_rect)),
        (Some(origin), Some(rect)) if rect.contains(origin)
    );
    if dragging && !on_the_bar {
        chrome.bar.bar.suppress_gesture();
    } else if !dragging {
        chrome.bar.bar.release_gesture();
    }
    if zoomed {
        chrome.bar.bar.suppress_transient(now_ms);
    }
    chrome.bar.bar.note_screen(screen, now_ms);
    // Suppressed means suppressed: nothing below this line runs, so the bar
    // cannot measure a world the gesture that suppressed it is still moving.
    // That is the rule the drag gestures already learned.
    if chrome.bar.bar.suppressed(now_ms) {
        return ask;
    }
    let (Some(chart), Some(bbox)) = (env.chart_area, env.selected_bbox) else {
        return ask;
    };
    // Read the object, never clone it, on the way in. This runs every frame
    // something is selected, and a `Drawing` carries a `Vec` of anchors plus a
    // boxed payload — a pencil stroke is 512 of them. The copy the undo
    // baseline needs is taken in `apply_actions`, only on the frame an edit
    // actually opens a gesture; idle frames clone nothing.
    let drawing = selected.drawing;
    let tool = drawing.tool;
    let locked = drawing.locked;
    let mut style = drawing.style;
    let glyph_before = tool.glyph_size(drawing);
    // One line, only for an object the trader did not place. Formatting it
    // costs an allocation on the frames a selected annotation is on screen —
    // never on the tape's path, and never for the objects the trader drew.
    let author = drawing.author.as_ref().map(DrawingAuthor::label);
    let mut object = drawings::context_bar::BarObject {
        style: &mut style,
        glyph_size: glyph_before,
        author: author.as_deref(),
        locked,
        hidden: drawing.hidden,
        supports_fill: tool.supports_fill(),
        // Pinned, the full panel is already on screen: a gear leading where
        // the eye already is would be the dead slot the bar's own contract
        // forbids.
        settings_available: !chrome.shared.pinned,
        confirming_delete: chrome.shared.delete_confirm && locked,
        tool_name: tool.name(),
    };
    let size = drawings::context_bar::bar_size(&drawings::context_bar::slots(
        drawings::context_bar::capabilities(&object),
    ));
    // The live lane is off limits to the bar however it got where it is: that
    // strip is where the price the trader is reading is being formed, and
    // `place` has kept clear of it since it was written. A parked bar is
    // placed by a different rule, not held to a different one.
    let right_limit = env.lane_divider_x.unwrap_or(chart.right());
    let reachable = bounds(chart, right_limit, size);
    let position = match chrome.bar.bar.manual_position() {
        // Repair for drawing, never overwrite — the rule the properties popup
        // already follows, for the same reason. A bar parked out near the
        // right edge of a wide pane must stay reachable when the canvas is
        // split and that pane is half as wide, and the repair leaves the
        // parked point alone, so widening the pane gives it back. (A fresh
        // drag is a fresh decision and does replace it, measured from where
        // the bar is actually drawn — dragging from a point the window is not
        // at would make it jump on the first pixel.)
        //
        // The clamp is against the pane the *selection* lives on, which is
        // what makes a bar parked over one chart of a split come back inside
        // the other one rather than hovering over its neighbour.
        Some(parked) => parked,
        None => drawings::context_bar::place(chart, right_limit, bbox, size),
    };
    // Both answers go through the same repair, so "clear of the live lane" is
    // a property of the bar and not of the branch that placed it. `place`
    // keeps clear of the lane on every path but its last one — the fallback
    // for an object that covers the pane end to end, which clamps against the
    // pane's own right edge — and that path is reachable with a full-height
    // profile on a narrow split. It also keeps the popover bound below
    // honest, which is derived from where the bar ends up.
    let position = clamp_into_chart(position, size, reachable);
    // What the popovers are clamped into: the same rectangle *without* the
    // bar's width floor, but never narrower than the bar that was actually
    // drawn.
    //
    // The floor exists so a history area narrower than the bar still has
    // somewhere to put one — `place` makes the same call — and it is the
    // bar's reason, not the palette's: a palette can be pushed left, so
    // nothing buys it the right to sit on the forming column. But when the
    // floor did have to push the bar into the lane, a bound that stopped
    // short of it would leave the palette hanging off nothing, which is the
    // failure the placement rule spends its effort on.
    let popover_bounds = egui::Rect::from_min_max(
        chart.min,
        egui::pos2(
            right_limit.min(chart.right()).max(position.x + size.x),
            chart.bottom(),
        ),
    );
    let intent = drawings::context_bar::show(
        &mut chrome.bar.bar,
        ctx,
        position,
        popover_bounds,
        &mut object,
    );
    let glyph_after = object.glyph_size;
    #[cfg(test)]
    {
        chrome.bar.last_drawn_rect = chrome.bar.bar.last_rect();
    }

    if intent.reset_position {
        chrome.bar.bar.clear_manual();
    } else if intent.drag_delta != egui::Vec2::ZERO {
        chrome.bar.bar.set_manual(position + intent.drag_delta);
    }
    let actions = InspectorActions {
        toggle_hidden: intent.toggle_hidden,
        toggle_lock: intent.actions.toggle_lock,
        delete: intent.actions.delete,
        force_delete: intent.force_delete,
        cancel_delete: intent.cancel_delete,
        edited: intent.edited,
        ..InspectorActions::default()
    };
    if intent.edited {
        // The copy the trader is editing, handed back rather than written
        // through a `&mut` into the pane: the host owns every object every
        // renderer reads.
        let mut edited = drawing.clone();
        edited.style = style;
        if let Some(size) = glyph_after {
            tool.set_glyph_size(&mut edited, size.px);
        }
        ask.edited = Some(Box::new(edited));
    }
    if intent.open_settings {
        chrome.shared.open = true;
        // Place against the object the way a fresh selection would.
        chrome.shared.last_selection = None;
    }
    ask.duplicate |= intent.duplicate;
    ask.merge(super::apply_actions(chrome, ctx, actions, index, env));
    ask
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live lane is off limits to the bar however it got where it is.
    ///
    /// `context_bar::place` has kept clear of that strip since it was written
    /// — it is where the price the trader is reading is being formed, and
    /// losing sight of it is the trader's first veto. A parked bar is placed
    /// by a different rule, so the bound both rules clamp into is what keeps
    /// them honest: without it the repair would happily pin a parked bar at
    /// the pane's own right edge, on top of the forming column, for every
    /// object of the session.
    #[test]
    fn the_context_bar_bound_takes_the_live_lane_off_the_pane() {
        let chart = egui::Rect::from_min_max(egui::pos2(60.0, 88.0), egui::pos2(1284.0, 832.0));
        let size = egui::vec2(292.0, 40.0);
        let divider = 900.0;

        let reachable = bounds(chart, divider, size);
        assert_eq!(
            reachable.right(),
            divider,
            "the lane is not the bar's to use"
        );
        assert_eq!(
            clamp_into_chart(egui::pos2(3000.0, 400.0), size, reachable).x,
            divider - size.x,
            "a bar parked out past the lane is repaired to its inner edge"
        );

        // No lane on this pane: the chart's own edge is the answer, exactly as
        // the automatic rule takes `chart.right()` when the divider is None.
        assert_eq!(bounds(chart, chart.right(), size).right(), chart.right());

        // A lane wider than the history area leaves nothing to place in. The
        // pane's edge beats an empty rectangle — the same call `place` makes.
        let all_lane = bounds(chart, chart.left() + 10.0, size);
        assert!(
            all_lane.width() >= size.x.min(chart.width()),
            "a bar still has somewhere to go: {all_lane:?}"
        );
        assert!(chart.contains_rect(all_lane), "and it is inside the pane");
    }
}
