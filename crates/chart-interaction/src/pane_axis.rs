//! Axis and indicator gestures, resolved from caller-supplied hit facts.
use crate::pane::{Effect, Model};
use crate::viewport::DEFAULT_PX_PER_BAR;

pub const AXIS_ZOOM_DRAG_PX: f32 = 150.0;
pub const AXIS_ZOOM_SCROLL_PX: f32 = 200.0;
pub const TIME_ZOOM_DRAG_PX: f32 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleTarget {
    Price,
    Indicator(u64),
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleAction {
    Reset,
    Zoom {
        factor: f64,
        auto: (f64, f64),
        flip_span: Option<f64>,
    },
    Pan {
        delta: f64,
        height: f64,
        auto: (f64, f64),
    },
}
#[derive(Debug, Clone, Copy)]
pub enum AxisGesture {
    /// A time strip: drag or scroll zooms its axis, `reset` (a double-click)
    /// puts that axis back on its default spacing, as the price axis does.
    Time {
        reset: bool,
        drag: f32,
        scroll: f32,
        scroll_gain: f32,
        tape: bool,
    },
    ResizeTape {
        delta: f32,
        width: f32,
    },
    Scale {
        target: ScaleTarget,
        reset: bool,
        drag: f32,
        scroll: f32,
        auto: Option<(f64, f64)>,
        flip_span: Option<f64>,
        inverted: bool,
    },
    PaneBody {
        slot: u64,
        reset: bool,
        primary_free: bool,
        drag: [f32; 2],
        height: f32,
        auto: Option<(f64, f64)>,
        total: usize,
        scroll: f32,
        scroll_gain: f32,
    },
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sizing {
    Auto,
    Manual(f32),
    Collapsed,
}
#[derive(Debug, Clone, Copy)]
pub enum IndicatorGesture {
    Disclosure {
        slot: u64,
        clicked: bool,
        double_clicked: bool,
        collapsed: bool,
        minimum_height: f32,
    },
    Divider {
        slot: u64,
        reset: bool,
        delta: f32,
        height: f32,
    },
    Settings(u64),
    Guide {
        slot: u64,
        enabled: bool,
    },
}

/// The one cursor a time strip shows. Never two: the strip's resize arrow and
/// the chart's crosshair do not share the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripCursor {
    /// Hovered: drag or scroll here zooms the time axis.
    Resize,
    /// Held: the drag reads the chart, so the pointer is a crosshair.
    Crosshair,
}

/// The strip's cursor from its hover and primary-button hold, `None` when the
/// pointer is elsewhere and the strip has nothing to say.
#[must_use]
pub fn time_strip_cursor(hovered: bool, held: bool) -> Option<StripCursor> {
    if held {
        Some(StripCursor::Crosshair)
    } else if hovered {
        Some(StripCursor::Resize)
    } else {
        None
    }
}

pub(crate) fn axis(model: &mut Model, gesture: AxisGesture) -> Vec<Effect> {
    let mut out = Vec::new();
    match gesture {
        AxisGesture::Time {
            reset,
            drag,
            scroll,
            scroll_gain,
            tape,
        } => {
            if reset {
                if tape {
                    out.push(Effect::ResetTapeZoom);
                } else {
                    model.viewport.set_px_per_bar(DEFAULT_PX_PER_BAR);
                }
            }
            for factor in [
                (drag != 0.0).then(|| (drag / TIME_ZOOM_DRAG_PX).exp()),
                (scroll != 0.0).then(|| 2.0_f32.powf(scroll / scroll_gain)),
            ]
            .into_iter()
            .flatten()
            {
                if tape {
                    out.push(Effect::ZoomTape(factor));
                } else {
                    model.viewport.zoom(factor);
                }
            }
        }
        AxisGesture::ResizeTape { delta, width } => out.push(Effect::ResizeTape { delta, width }),
        AxisGesture::Scale {
            target,
            reset,
            drag,
            scroll,
            auto,
            flip_span,
            inverted,
        } => {
            if reset {
                out.push(Effect::Scale {
                    target,
                    action: ScaleAction::Reset,
                });
            }
            if let Some(auto) = auto {
                if drag != 0.0 {
                    let sense = if flip_span.is_some() && inverted {
                        -1.0
                    } else {
                        1.0
                    };
                    out.push(Effect::Scale {
                        target,
                        action: ScaleAction::Zoom {
                            factor: f64::from(sense * drag / AXIS_ZOOM_DRAG_PX).exp(),
                            auto,
                            flip_span,
                        },
                    });
                }
                if scroll != 0.0 {
                    out.push(Effect::Scale {
                        target,
                        action: ScaleAction::Zoom {
                            factor: f64::from(-scroll / AXIS_ZOOM_SCROLL_PX).exp(),
                            auto,
                            flip_span: None,
                        },
                    });
                }
            }
        }
        AxisGesture::PaneBody {
            slot,
            reset,
            primary_free,
            drag,
            height,
            auto,
            total,
            scroll,
            scroll_gain,
        } => {
            let target = ScaleTarget::Indicator(slot);
            if primary_free {
                if reset {
                    out.push(Effect::Scale {
                        target,
                        action: ScaleAction::Reset,
                    });
                }
                if total > 0 && drag[0] != 0.0 {
                    model.viewport.pan_pixels(drag[0], total);
                }
                if height > 1.0
                    && drag[1] != 0.0
                    && let Some(auto) = auto
                {
                    out.push(Effect::Scale {
                        target,
                        action: ScaleAction::Pan {
                            delta: f64::from(drag[1]),
                            height: f64::from(height),
                            auto,
                        },
                    });
                }
            }
            if scroll != 0.0 {
                model.viewport.zoom(2.0_f32.powf(scroll / scroll_gain));
            }
        }
    }
    out
}

pub(crate) fn indicator(model: &mut Model, gesture: IndicatorGesture) -> Vec<Effect> {
    let mut out = Vec::new();
    match gesture {
        IndicatorGesture::Settings(slot) => model.pending_settings = Some(slot),
        IndicatorGesture::Guide { slot, enabled } => {
            model.pending_indicator_guide = Some((slot, enabled))
        }
        IndicatorGesture::Disclosure {
            slot,
            clicked,
            double_clicked,
            collapsed,
            minimum_height,
        } => {
            if double_clicked && model.strip_expanded == Some(slot) {
                model.pending_settings = Some(slot);
                model.strip_expanded = None;
            } else if clicked {
                model.strip_expanded = collapsed.then_some(slot);
                out.push(Effect::IndicatorSizing {
                    slot,
                    sizing: if collapsed {
                        Sizing::Manual(minimum_height)
                    } else {
                        Sizing::Collapsed
                    },
                });
            }
        }
        IndicatorGesture::Divider {
            slot,
            reset,
            delta,
            height,
        } => {
            let sizing = if reset {
                Some(Sizing::Auto)
            } else {
                (delta != 0.0).then_some(Sizing::Manual(height - delta))
            };
            if let Some(sizing) = sizing {
                out.push(Effect::IndicatorSizing { slot, sizing });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::{Intent, update};
    #[test]
    fn a_strip_double_click_survives_the_geometry_change_and_is_spent() {
        let mut model = Model::default();
        assert_eq!(
            update(
                &mut model,
                Intent::Indicator(IndicatorGesture::Disclosure {
                    slot: 7,
                    clicked: true,
                    double_clicked: false,
                    collapsed: true,
                    minimum_height: 80.0,
                })
            ),
            [Effect::IndicatorSizing {
                slot: 7,
                sizing: Sizing::Manual(80.0)
            }]
        );
        assert!(
            update(
                &mut model,
                Intent::Indicator(IndicatorGesture::Disclosure {
                    slot: 7,
                    clicked: true,
                    double_clicked: true,
                    collapsed: false,
                    minimum_height: 80.0,
                })
            )
            .is_empty()
        );
        assert_eq!(model.pending_settings.take(), Some(7));
        assert_eq!(model.strip_expanded, None);
        assert_eq!(
            update(
                &mut model,
                Intent::Indicator(IndicatorGesture::Disclosure {
                    slot: 7,
                    clicked: true,
                    double_clicked: false,
                    collapsed: false,
                    minimum_height: 80.0,
                })
            ),
            [Effect::IndicatorSizing {
                slot: 7,
                sizing: Sizing::Collapsed
            }]
        );
    }
    #[test]
    fn a_tool_claims_primary_only_and_the_wheel_still_zooms() {
        let mut model = Model::default();
        let effects = update(
            &mut model,
            Intent::Axis(AxisGesture::PaneBody {
                slot: 7,
                reset: true,
                primary_free: false,
                drag: [20.0, 20.0],
                height: 100.0,
                auto: Some((1.0, 2.0)),
                total: 100,
                scroll: 120.0,
                scroll_gain: 120.0,
            }),
        );
        assert!(effects.is_empty());
        assert_eq!(model.viewport.px_per_bar(), 16.0);
        assert!(model.viewport.follows_live());
    }
    #[test]
    fn inverted_axis_drag_mirrors_but_wheel_does_not_flip() {
        let mut model = Model::default();
        let auto = (10.0, 20.0);
        assert_eq!(
            update(
                &mut model,
                Intent::Axis(AxisGesture::Scale {
                    target: ScaleTarget::Price,
                    reset: true,
                    drag: 150.0,
                    scroll: 200.0,
                    auto: Some(auto),
                    flip_span: Some(10.0),
                    inverted: true,
                })
            ),
            [
                Effect::Scale {
                    target: ScaleTarget::Price,
                    action: ScaleAction::Reset
                },
                Effect::Scale {
                    target: ScaleTarget::Price,
                    action: ScaleAction::Zoom {
                        factor: (-1.0_f64).exp(),
                        auto,
                        flip_span: Some(10.0)
                    }
                },
                Effect::Scale {
                    target: ScaleTarget::Price,
                    action: ScaleAction::Zoom {
                        factor: (-1.0_f64).exp(),
                        auto,
                        flip_span: None
                    }
                }
            ]
        );
        assert_eq!(
            update(
                &mut model,
                Intent::Axis(AxisGesture::Scale {
                    target: ScaleTarget::Indicator(7),
                    reset: true,
                    drag: 150.0,
                    scroll: 200.0,
                    auto: None,
                    flip_span: None,
                    inverted: false,
                })
            ),
            [Effect::Scale {
                target: ScaleTarget::Indicator(7),
                action: ScaleAction::Reset
            }]
        );
    }
    #[test]
    fn a_time_strip_double_click_resets_its_own_axis_spacing_only() {
        let mut model = Model::default();
        // A launch hook opened the pane at 12 px: that, not the compiled
        // default, is the spacing a reset returns to.
        model.viewport.open_at_px_per_bar(12.0);
        model.viewport.zoom(3.0);
        model.viewport.pan_pixels(40.0, 100);
        let time = |reset, tape| {
            Intent::Axis(AxisGesture::Time {
                reset,
                drag: 0.0,
                scroll: 0.0,
                scroll_gain: 120.0,
                tape,
            })
        };
        assert!(update(&mut model, time(true, false)).is_empty());
        assert_eq!(model.viewport.px_per_bar(), 12.0);
        assert!(!model.viewport.follows_live(), "a reset is not a jump");

        model.viewport.zoom(3.0);
        assert_eq!(
            update(&mut model, time(true, true)),
            [Effect::ResetTapeZoom]
        );
        assert_eq!(
            model.viewport.px_per_bar(),
            36.0,
            "the tape strip resets the tape, never the candles"
        );
        assert!(update(&mut model, time(false, true)).is_empty());
    }
    #[test]
    fn a_time_strip_shows_one_cursor_resize_on_hover_crosshair_while_held() {
        assert_eq!(time_strip_cursor(false, false), None);
        assert_eq!(time_strip_cursor(true, false), Some(StripCursor::Resize));
        assert_eq!(time_strip_cursor(true, true), Some(StripCursor::Crosshair));
        assert_eq!(
            time_strip_cursor(false, true),
            Some(StripCursor::Crosshair),
            "a drag that left the strip still reads the chart"
        );
    }
    #[test]
    fn indicator_divider_auto_reset_wins_over_drag() {
        let mut model = Model::default();
        assert_eq!(
            update(
                &mut model,
                Intent::Indicator(IndicatorGesture::Divider {
                    slot: 4,
                    reset: true,
                    delta: 15.0,
                    height: 80.0,
                })
            ),
            [Effect::IndicatorSizing {
                slot: 4,
                sizing: Sizing::Auto
            }]
        );
        assert_eq!(
            update(
                &mut model,
                Intent::Indicator(IndicatorGesture::Divider {
                    slot: 4,
                    reset: false,
                    delta: -15.0,
                    height: 80.0,
                })
            ),
            [Effect::IndicatorSizing {
                slot: 4,
                sizing: Sizing::Manual(95.0)
            }]
        );
    }
}
