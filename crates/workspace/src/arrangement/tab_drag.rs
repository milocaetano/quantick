//! Atomic drag policy in desktop physical pixels, independent of native UI.
use super::{
    TabId,
    windows::{TabDrag, TransferTarget, WindowId},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    /// Both inputs use the viewport's full logical scale, including zoom.
    pub fn physical(origin: Self, local: Self, pixels_per_point: f32) -> Self {
        Self {
            x: (origin.x + local.x) * pixels_per_point,
            y: (origin.y + local.y) * pixels_per_point,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}
impl Rect {
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}
#[derive(Clone, Debug)]
pub struct Strip {
    pub window: WindowId,
    pub bounds: Rect,
    /// Visible chips in presentation order; positions include hidden chips.
    pub chips: Vec<(TabId, usize, Rect)>,
    pub exclusions: Vec<Rect>,
    /// The complete title bar. Its Home/add/overflow/window controls cancel
    /// a drop instead of becoming detach destinations.
    pub chrome: Rect,
    pub order: Vec<TabId>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drop {
    pub target: TransferTarget,
    pub position: usize,
    /// Desktop physical pixels, translated back by the painting adapter.
    pub marker_x: Option<f32>,
    pub eligible: bool,
}

/// Opaque ownership token stays unchanged until release. Escape consumes it.
#[derive(Default)]
pub struct Gesture {
    drag: Option<TabDrag>,
    reveal: Option<(WindowId, usize)>,
    hover_ticks: u32,
}
impl Gesture {
    pub fn begin(&mut self, drag: TabDrag) {
        self.drag = Some(drag);
    }
    pub fn active(&self) -> Option<TabDrag> {
        self.drag
    }
    pub fn cancel(&mut self) {
        self.drag = None;
        self.reveal = None;
        self.hover_ticks = 0;
    }
    pub fn reveal(&self, window: WindowId) -> Option<usize> {
        self.reveal
            .filter(|(id, _)| *id == window)
            .map(|(_, index)| index)
    }
    /// Twelve process frames at a strip edge reveal the next hidden market.
    /// No clock or native input enters the ordering policy.
    pub fn hover(&mut self, point: Point, strips: &[Strip]) {
        if self.drag.is_none() {
            return;
        }
        let Some(strip) = strips.iter().find(|strip| strip.bounds.contains(point)) else {
            self.hover_ticks = 0;
            return;
        };
        let (Some(first), Some(last)) = (strip.chips.first(), strip.chips.last()) else {
            return;
        };
        let edge = (strip.bounds.max.x - strip.bounds.min.x) * 0.15;
        let next = if point.x < first.2.min.x + edge && first.1 > 0 {
            Some(first.1 - 1)
        } else if point.x > last.2.max.x - edge && last.1 + 1 < strip.order.len() {
            Some(last.1 + 1)
        } else {
            None
        };
        if let Some(next) = next {
            self.hover_ticks += 1;
            if self.hover_ticks >= 12 {
                self.reveal = Some((strip.window, next));
                self.hover_ticks = 0;
            }
        } else {
            self.hover_ticks = 0;
        }
    }
    pub fn release(
        &mut self,
        point: Point,
        strips: &[Strip],
        escape: bool,
    ) -> Option<(TabDrag, Drop)> {
        let drag = self.drag.take()?;
        self.reveal = None;
        (!escape).then(|| (drag, destination(drag, point, strips)))
    }
}

pub fn destination(drag: TabDrag, point: Point, strips: &[Strip]) -> Drop {
    let Some(strip) = strips.iter().find(|strip| strip.bounds.contains(point)) else {
        return Drop {
            target: TransferTarget::NewWindow,
            position: 0,
            marker_x: None,
            eligible: !strips.iter().any(|strip| strip.chrome.contains(point)),
        };
    };
    let source_index = strip.order.iter().position(|id| *id == drag.tab());
    let before = strip
        .chips
        .iter()
        .filter(|(id, _, _)| *id != drag.tab())
        .find(|(_, _, rect)| point.x < (rect.min.x + rect.max.x) * 0.5);
    let (position, marker_x) = if let Some((_, index, rect)) = before {
        (
            *index - usize::from(source_index.is_some_and(|source| source < *index)),
            rect.min.x,
        )
    } else if let Some((_, index, rect)) = strip
        .chips
        .iter()
        .rev()
        .find(|(id, _, _)| *id != drag.tab())
    {
        (
            *index + 1 - usize::from(source_index.is_some_and(|source| source < *index)),
            rect.max.x,
        )
    } else {
        (0, strip.bounds.min.x)
    };
    let eligible = !strip.exclusions.iter().any(|rect| rect.contains(point));
    Drop {
        target: TransferTarget::Window(strip.window),
        position: position.min(strip.order.len() - usize::from(strip.window == drag.source())),
        marker_x: eligible.then_some(marker_x),
        eligible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{arrangement::windows::TabWindows, lifetime::WorkspaceId};
    fn point(x: f32, y: f32) -> Point {
        Point { x, y }
    }
    fn rect(x: f32) -> Rect {
        Rect {
            min: point(x, 0.0),
            max: point(x + 100.0, 40.0),
        }
    }
    #[test]
    fn reorder_counts_insertion_after_excluding_dragged_chip_and_escape_wins_release() {
        let mut windows = TabWindows::new(WorkspaceId(1), TabId(1));
        windows.append(TabId(2));
        windows.append(TabId(3));
        let drag = windows.begin_drag(TabId(1)).unwrap();
        let strip = Strip {
            window: WindowId::ROOT,
            bounds: Rect {
                min: point(0.0, 0.0),
                max: point(400.0, 40.0),
            },
            chips: vec![
                (TabId(1), 0, rect(0.0)),
                (TabId(2), 1, rect(100.0)),
                (TabId(3), 2, rect(200.0)),
            ],
            exclusions: vec![],
            chrome: Rect {
                min: point(0.0, 0.0),
                max: point(500.0, 40.0),
            },
            order: vec![TabId(1), TabId(2), TabId(3)],
        };
        assert_eq!(
            destination(drag, point(190.0, 10.0), std::slice::from_ref(&strip)).position,
            1
        );
        assert_eq!(
            destination(drag, point(350.0, 10.0), std::slice::from_ref(&strip)).position,
            2
        );
        let before = windows.clone();
        let mut gesture = Gesture::default();
        gesture.begin(drag);
        assert!(
            gesture
                .release(point(500.0, 10.0), &[strip], true)
                .is_none()
        );
        assert!(gesture.active().is_none());
        assert_eq!(windows, before);
    }
    #[test]
    fn physical_desktop_coordinates_support_negative_origins_and_scaled_strips() {
        let windows = TabWindows::new(WorkspaceId(1), TabId(1));
        let drag = windows.begin_drag(TabId(1)).unwrap();
        let strip = Strip {
            window: WindowId(2),
            bounds: Rect {
                min: point(-1800.0, -900.0),
                max: point(-900.0, -822.0),
            },
            chips: vec![(
                TabId(2),
                0,
                Rect {
                    min: point(-1700.0, -900.0),
                    max: point(-1400.0, -822.0),
                },
            )],
            exclusions: vec![],
            chrome: Rect {
                min: point(-1800.0, -900.0),
                max: point(-800.0, -822.0),
            },
            order: vec![TabId(2)],
        };
        assert_eq!(
            destination(drag, point(-1500.0, -850.0), &[strip]).target,
            TransferTarget::Window(WindowId(2))
        );
        assert_eq!(
            destination(drag, point(0.0, 80.0), &[]).target,
            TransferTarget::NewWindow
        );
    }

    #[test]
    fn overflow_reveal_keeps_drag_origin_and_cancellation_preserves_every_strip() {
        let mut windows = TabWindows::new(WorkspaceId(1), TabId(1));
        for id in 2..=8 {
            windows.append(TabId(id));
        }
        let drag = windows.begin_drag(TabId(1)).unwrap();
        let strip = Strip {
            window: WindowId::ROOT,
            bounds: Rect {
                min: point(0.0, 0.0),
                max: point(220.0, 40.0),
            },
            chips: vec![(TabId(5), 4, rect(0.0)), (TabId(6), 5, rect(100.0))],
            exclusions: vec![Rect {
                min: point(190.0, 0.0),
                max: point(200.0, 40.0),
            }],
            chrome: Rect {
                min: point(0.0, 0.0),
                max: point(300.0, 40.0),
            },
            order: (1..=8).map(TabId).collect(),
        };
        let before = windows.clone();
        let mut gesture = Gesture::default();
        gesture.begin(drag);
        for _ in 0..12 {
            gesture.hover(point(185.0, 20.0), std::slice::from_ref(&strip));
        }
        assert_eq!(gesture.reveal(WindowId::ROOT), Some(6));
        assert_eq!(
            destination(drag, point(10.0, 20.0), std::slice::from_ref(&strip)).position,
            3,
            "hidden dragged chip is excluded from insertion positions"
        );
        assert!(
            !destination(drag, point(195.0, 20.0), std::slice::from_ref(&strip)).eligible,
            "a close target is not a reorder target"
        );
        assert!(
            gesture
                .release(point(185.0, 20.0), &[strip], true)
                .is_none()
        );
        assert_eq!(gesture.reveal(WindowId::ROOT), None);
        assert_eq!(windows, before);
    }
    #[test]
    fn desktop_hit_testing_uses_each_monitor_scale_including_zoom() {
        let source = Point::physical(point(0.0, 0.0), point(-1160.0, 120.0), 1.5);
        let target = Point::physical(point(-1536.0, 83.2), point(144.0, 60.8), 1.25);
        assert_eq!(source.x, target.x);
        assert!((source.y - target.y).abs() < 0.001);
        assert_eq!(source, point(-1740.0, 180.0));
    }
}
