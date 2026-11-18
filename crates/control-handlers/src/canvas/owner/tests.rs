use super::*;
use quantick_workspace::canvas::{FloatHost, PaneMode, preset};

struct FakeHost {
    state: CanvasState,
    saved: CanvasState,
    events: Vec<&'static str>,
    widget_refusal: Option<String>,
}
impl FakeHost {
    fn new() -> Self {
        let state = CanvasState::from_preset(preset("time+flow").unwrap());
        Self {
            saved: state.clone(),
            state,
            events: Vec::new(),
            widget_refusal: None,
        }
    }
}
impl CanvasOwner for FakeHost {
    fn state(&self) -> &CanvasState {
        &self.state
    }
    fn drawn(&self) -> Drawn {
        Drawn {
            area: CanvasRect::new(10.0, 20.0, 1000.0, 600.0),
            columns: false,
        }
    }
    fn saved(&self) -> Option<CanvasState> {
        Some(self.saved.clone())
    }
    fn unsaved(&self) -> bool {
        self.state != self.saved
    }
    fn origin(&self) -> (f32, f32) {
        (100.0, 200.0)
    }
    fn monitor(&self, _rect: CanvasRect) -> Option<String> {
        Some("monitor".into())
    }
    fn pick_layout(&mut self, preset: &'static LayoutPreset) {
        self.events.push("layout");
        self.state.apply_preset(preset);
    }
    fn show_widget(&mut self, _id: &'static str) -> Result<(), String> {
        self.events.push("widget");
        self.widget_refusal.clone().map_or(Ok(()), Err)
    }
    fn act(
        &mut self,
        saved: Option<&CanvasState>,
        act: CanvasAct,
    ) -> Result<(bool, Option<u32>), ControlError> {
        self.events.push("canvas");
        let drawn = self.drawn();
        crate::canvas::act(&mut self.state, saved, drawn, act)
    }
    fn open_window_menu(&mut self) -> bool {
        true
    }
    fn picker_read(&self) -> LayoutPickerResult {
        panic!("transaction does not read the picker")
    }
}

#[test]
fn layout_and_widget_actions_reach_only_their_owner() {
    let mut host = FakeHost::new();
    let unchanged = answer(
        &mut host,
        CanvasAct::PickLayout {
            preset_id: "time+flow".into(),
        },
    )
    .unwrap();
    assert!(!unchanged.changed && !unchanged.unsaved);
    let changed = answer(
        &mut host,
        CanvasAct::PickLayout {
            preset_id: "charts-4".into(),
        },
    )
    .unwrap();
    assert!(changed.changed && changed.unsaved);
    assert_eq!(changed.preset_id.as_deref(), Some("charts-4"));
    let before = host.state.clone();
    assert!(
        answer(
            &mut host,
            CanvasAct::PickLayout {
                preset_id: "missing".into()
            }
        )
        .is_err()
    );
    assert_eq!(host.state, before);
    assert_eq!(host.events, ["layout", "layout"]);
    let added = answer(
        &mut host,
        CanvasAct::AddWidget {
            widget_id: "depth".into(),
        },
    )
    .unwrap();
    assert!(added.changed);
    assert_eq!(
        host.state, before,
        "a surface widget does not add a canvas pane"
    );
    host.widget_refusal = Some("feed has no book".into());
    let error = answer(
        &mut host,
        CanvasAct::AddWidget {
            widget_id: "depth".into(),
        },
    )
    .unwrap_err();
    assert_eq!(error.message, "feed has no book");
    assert_eq!(host.events, ["layout", "layout", "widget", "widget"]);
}

#[test]
fn saved_removal_refuses_without_confirmation_and_reads_after_mutation() {
    let mut host = FakeHost::new();
    let before = host.state.clone();
    assert!(
        answer(
            &mut host,
            CanvasAct::Remove {
                slot: 0,
                confirm: false
            }
        )
        .is_err()
    );
    assert_eq!(host.state, before);
    let result = answer(
        &mut host,
        CanvasAct::Remove {
            slot: 0,
            confirm: true,
        },
    )
    .unwrap();
    assert!(result.changed && result.unsaved);
    assert!(result.panes.iter().all(|pane| pane.slot != 0));
    assert_eq!(host.events, ["canvas", "canvas"]);
}

#[test]
fn pop_out_uses_the_drawn_place_and_desktop_origin() {
    let mut host = FakeHost::new();
    let drawn = host
        .state
        .rects(host.drawn().area)
        .into_iter()
        .find(|(slot, _)| *slot == 0)
        .unwrap()
        .1;
    answer(
        &mut host,
        CanvasAct::PopOut {
            slot: 0,
            monitor: None,
            rect: None,
        },
    )
    .unwrap();
    let Some(FloatHost::OsWindow { monitor, rect }) = host.state.host(0) else {
        panic!("chart has its window")
    };
    assert_eq!(monitor.as_str(), "monitor");
    assert_eq!(
        *rect,
        CanvasRect::new(drawn.x + 100.0, drawn.y + 200.0, drawn.w, drawn.h)
    );
    assert!(matches!(host.state.mode(0), Some(PaneMode::OsWindow)));
}

#[test]
fn nominal_area_and_once_after_paint_hooks_keep_their_boundaries() {
    let nominal = CanvasRect::new(0.0, 0.0, 1280.0, 720.0);
    assert_eq!(canvas_area(CanvasRect::new(4.0, 5.0, 0.0, 600.0)), nominal);
    assert_eq!(canvas_area(CanvasRect::new(4.0, 5.0, 600.0, -1.0)), nominal);
    let area = CanvasRect::new(4.0, 5.0, 600.0, 700.0);
    assert_eq!(canvas_area(area), area);
    let mut hooked = false;
    assert_eq!(
        canvas_hooks(false, &mut hooked, || panic!("before paint")),
        (false, vec![])
    );
    assert!(!hooked);
    assert_eq!(
        canvas_hooks(true, &mut hooked, || ("1".into(), String::new())),
        (true, vec![])
    );
    assert!(hooked);
    assert_eq!(
        canvas_hooks(true, &mut hooked, || panic!("already consumed")),
        (false, vec![])
    );
}

#[test]
fn hold_waits_for_a_float_and_never_overwrites_a_pinned_pointer() {
    let mut host = FakeHost::new();
    let area = host.drawn().area;
    assert!(hold_from_hook(&host.state, || "0:0.25,0.5".into(), area, false).is_none());
    answer(
        &mut host,
        CanvasAct::Float {
            slot: 0,
            rect: None,
        },
    )
    .unwrap();
    let (_, point) = hold_from_hook(&host.state, || "0:0.25,0.5".into(), area, false).unwrap();
    assert_eq!(point, (260.0, 320.0));
    assert!(hold_from_hook(&host.state, || panic!("read while pinned"), area, true).is_none());
    assert!(hold_from_hook(&host.state, || "bad".into(), area, false).is_none());
}
