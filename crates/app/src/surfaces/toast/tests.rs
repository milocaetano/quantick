use super::super::{Surface, View as _};
use super::*;
use std::time::Duration;
fn env(now: Instant) -> SurfaceEnv<'static> {
    SurfaceEnv::quiet(now)
}

#[test]
fn the_capture_hook_reads_the_stated_input_not_the_environment() {
    let now = Instant::now();
    let ctx = egui::Context::default();
    for (value, message, undo) in [
        ("plain", "Workspace saved.", false),
        ("undo", "Trend line deleted.", true),
    ] {
        crate::hooks::captured::set_on_this_thread(Some(&[("QUANTICK_TOAST", value)]));
        let mut registry = super::super::Surfaces::default();
        let output = ctx.run(Default::default(), |ctx| {
            assert_eq!(
                registry.draw_all(ctx, &env(now)),
                SurfaceResponse::default()
            );
        });
        assert_eq!(registry.toast.model_state.message(), Some(message));
        assert_eq!(registry.toast.model_state.offers_undo(), undo);
        assert_eq!(registry.toast.view.undo_rect().is_some(), undo);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == message)));
        let _ = ctx.run(Default::default(), |ctx| {
            registry.draw_all(ctx, &env(now + Duration::from_secs(8)));
        });
        assert_eq!(
            registry.toast.model_state.message(),
            None,
            "hook and Tick share a time coordinate and hook runs once"
        );
    }
    crate::hooks::captured::set_on_this_thread(Some(&[]));
    let mut unstated = new();
    assert_eq!(unstated.view.apply_env_hook(&env(now)), None);
    crate::hooks::captured::set_on_this_thread(None);
    assert_eq!(
        unstated.model_state.message(),
        None,
        "an unstated input is unset"
    );
}

#[test]
fn only_an_undoable_act_paints_the_button() {
    let ctx = egui::Context::default();
    let now = Instant::now();

    let mut plain = new();
    note(&mut plain, "Drawings cleared.", now);
    let _ = ctx.run(Default::default(), |ctx| {
        plain.draw(ctx, &env(now));
    });
    assert!(plain.view.undo_rect().is_none());

    let mut undoable = new();
    note_with_undo(&mut undoable, "Trend line deleted.", now);
    let _ = ctx.run(Default::default(), |ctx| {
        undoable.draw(ctx, &env(now));
    });
    assert!(undoable.view.undo_rect().is_some());
}

#[test]
fn an_untouched_toast_asks_the_host_for_nothing() {
    let ctx = egui::Context::default();
    let now = Instant::now();
    let mut surface = new();
    note_with_undo(&mut surface, "Trend line deleted.", now);
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(Default::default(), |ctx| {
        response = surface.draw(ctx, &env(now));
    });
    assert_eq!(response, SurfaceResponse::default());
}

#[test]
fn supplied_times_before_the_anchor_and_backwards_ticks_preserve_deadlines() {
    let now = Instant::now();
    for note_before_anchor in [false, true] {
        let mut surface = new();
        let first = surface.view.time(now);
        surface.update(Intent::Tick(first));
        let shown = if note_before_anchor {
            now - Duration::from_secs(1)
        } else {
            now
        };
        note(&mut surface, "saved", shown);
        let backwards = surface.view.time(shown - Duration::from_secs(1));
        surface.update(Intent::Tick(backwards));
        assert_eq!(surface.model_state.message(), Some("saved"));
        let before = surface
            .view
            .time(shown + Duration::from_secs(8) - Duration::from_nanos(1));
        surface.update(Intent::Tick(before));
        assert_eq!(surface.model_state.message(), Some("saved"));
        let deadline = surface.view.time(shown + Duration::from_secs(8));
        surface.update(Intent::Tick(deadline));
        assert_eq!(surface.model_state.message(), None);
    }
}
