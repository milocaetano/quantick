//! The aim, seen from the panes it was not made in.
//!
//! One desk serves every pane of a split, so the price a held key aims at in
//! one pane is a level every other pane can show. The pane the hand is on
//! paints the whole aim (line, label, chip and bracket). Every other pane
//! paints an echo: a faint dashed line at the aimed price on its *own* axis,
//! so the trader reads the level against tick, renko and minute bars at
//! once. The echo is a picture and nothing else. It has no label, no chip
//! and no click, because the click belongs to the pane the hand is on.

use quantick_engine::Side;
use rust_decimal::prelude::ToPrimitive;

use super::Desk;
use super::cmd::{CmdPreview, HeldKeys};
use super::geometry::{Bounds, PriceAxis};

/// What one pane paints of this frame's aim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AimPaint<'a> {
    /// The pane the aim was made in: line, label, chip and bracket.
    Full(&'a CmdPreview),
    /// Any other pane: a dashed line at `y`, the aimed price mapped
    /// through that pane's own axis.
    Echo { side: Side, y: f32 },
}

impl Desk {
    /// What a pane paints of the aim this frame, or `None` for nothing.
    ///
    /// `band` is the pane's interactive plot, `hand_here` whether its
    /// pointer feeds paper input, `keys` the frame's held modifiers and
    /// `axis` the pane's own price axis.
    ///
    /// The pane the aim was made in owns it whole: the one holding the hand,
    /// or under the capture hook (where no pane holds a hand) the band that
    /// holds the parked pointer. A pane holding the hand never echoes: its
    /// own aim is the only one it can have.
    ///
    /// Panes are painted one after another, and only the pane under the hand
    /// recomputes the aim, so a pane painted *before* it reads last frame's
    /// aim. The held keys are this frame's, so the echo drops the very frame
    /// the key is let go rather than one frame late. A price that moved is a
    /// frame behind until the next frame, which an input event always asks
    /// for.
    #[must_use]
    pub fn aim_paint(
        &self,
        band: Bounds,
        hand_here: bool,
        keys: HeldKeys,
        axis: &dyn PriceAxis,
    ) -> Option<AimPaint<'_>> {
        let preview = self.gesture.cmd_preview.as_ref()?;
        if hand_here || preview.forced {
            if band.contains(preview.pointer) {
                return Some(AimPaint::Full(preview));
            }
            if hand_here {
                return None;
            }
        }
        if !preview.forced && self.cmd_trading.aimed_side(keys) != Some(preview.side) {
            return None;
        }
        let y = axis.y(preview.price.to_f64().unwrap_or_default());
        (band.top() <= y && y <= band.bottom()).then_some(AimPaint::Echo {
            side: preview.side,
            y,
        })
    }
}

#[cfg(test)]
mod tests {
    use quantick_engine::{Side, Trade};
    use rust_decimal::Decimal;

    use super::AimPaint;
    use crate::PaperAccount;
    use crate::desk::Desk;
    use crate::desk::cmd::{CmdPreviewForce, HeldKeys};
    use crate::desk::geometry::{Bounds, Point, PriceAxis};
    use crate::desk::gesture::ChartFrame;
    use crate::scratch::ScratchDir;

    /// A price axis over pixels `0..400` from `high` at the top to `low`
    /// at the bottom: two panes of a split, each with its own range.
    struct Axis {
        low: f64,
        high: f64,
    }

    impl PriceAxis for Axis {
        fn y(&self, price: f64) -> f32 {
            ((self.high - price) / (self.high - self.low) * 400.0) as f32
        }

        fn price_at(&self, y: f32) -> f64 {
            self.high - f64::from(y) / 400.0 * (self.high - self.low)
        }

        fn is_inverted(&self) -> bool {
            false
        }
    }

    /// The pane the hand is on: 80..120 over the left half.
    const AIMED: Axis = Axis {
        low: 80.0,
        high: 120.0,
    };
    /// The other pane: 90..110 over the right half, so 95 sits lower.
    const OTHER: Axis = Axis {
        low: 90.0,
        high: 110.0,
    };

    fn aimed_band() -> Bounds {
        Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(400.0, 400.0))
    }

    fn other_band() -> Bounds {
        Bounds::from_min_max(Point::new(410.0, 0.0), Point::new(810.0, 400.0))
    }

    const SHIFT: HeldKeys = HeldKeys {
        shift: true,
        command: false,
        alt: false,
    };

    fn marked_account(dir: &ScratchDir) -> PaperAccount {
        let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
        account.set_symbol("ECHOX");
        account.seed(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        account
    }

    /// One input frame on the aimed pane, the pointer at `pointer`.
    fn frame(pointer: Option<Point>, keys: HeldKeys) -> ChartFrame {
        ChartFrame {
            chart: aimed_band(),
            pointer,
            primary_pressed: false,
            primary_down: false,
            primary_released: false,
            keys,
            canvas_claimed: false,
            scroll_y: 0.0,
            middle_pressed: false,
            layer_visible: true,
        }
    }

    /// Shift over the aimed pane at y 250 aims a buy at 95.
    fn aiming_desk(account: &PaperAccount) -> Desk {
        let mut desk = Desk::default();
        desk.handle_input(
            account,
            &frame(Some(Point::new(200.0, 250.0)), SHIFT),
            Some(&AIMED),
        );
        assert_eq!(
            desk.gesture.cmd_preview.map(|preview| preview.price),
            Some(Decimal::from(95)),
            "Shift over the chart aims at 95"
        );
        desk
    }

    /// The pane under the hand paints the whole aim; every other pane
    /// paints the echo at the aimed price on its own axis.
    #[test]
    fn the_other_pane_echoes_the_aim_at_its_own_y() {
        let dir = ScratchDir::new("echo-on");
        let account = marked_account(&dir);
        let desk = aiming_desk(&account);

        assert!(matches!(
            desk.aim_paint(aimed_band(), true, SHIFT, &AIMED),
            Some(AimPaint::Full(preview)) if preview.side == Side::Buy
        ));
        assert_eq!(
            desk.aim_paint(other_band(), false, SHIFT, &OTHER),
            Some(AimPaint::Echo {
                side: Side::Buy,
                y: OTHER.y(95.0),
            }),
            "95 on the other pane's own axis, not the aimed pane's pixel"
        );
        assert_ne!(OTHER.y(95.0), AIMED.y(95.0), "the panes disagree on y");
    }

    /// No aim, no echo; and a price off the other pane's range paints
    /// nothing there rather than pinning a line to its edge.
    #[test]
    fn no_aim_or_an_off_range_price_paints_no_echo() {
        let dir = ScratchDir::new("echo-off");
        let account = marked_account(&dir);
        let mut desk = Desk::default();
        desk.handle_input(
            &account,
            &frame(Some(Point::new(200.0, 250.0)), HeldKeys::default()),
            Some(&AIMED),
        );
        assert_eq!(desk.gesture.cmd_preview, None, "no key, no aim");
        assert_eq!(desk.aim_paint(other_band(), false, SHIFT, &OTHER), None);

        let desk = aiming_desk(&account);
        let far = Axis {
            low: 200.0,
            high: 210.0,
        };
        assert_eq!(
            desk.aim_paint(other_band(), false, SHIFT, &far),
            None,
            "95 is off a 200..210 pane"
        );
    }

    /// A pane painted before the one under the hand still holds last
    /// frame's aim. The keys are this frame's: let go of Shift and the echo
    /// is gone that same frame, and the hand's own pane never echoes.
    #[test]
    fn the_echo_drops_the_frame_the_key_is_released() {
        let dir = ScratchDir::new("echo-release");
        let account = marked_account(&dir);
        let desk = aiming_desk(&account);

        assert_eq!(
            desk.aim_paint(other_band(), false, HeldKeys::default(), &OTHER),
            None,
            "the key is up: the aim ends now, not a frame later"
        );
        let both = HeldKeys {
            shift: true,
            alt: true,
            command: false,
        };
        assert_eq!(
            desk.aim_paint(other_band(), false, both, &OTHER),
            None,
            "both keys aim nothing, so nothing echoes"
        );
        assert_eq!(
            desk.aim_paint(other_band(), true, SHIFT, &OTHER),
            None,
            "a pane holding the hand paints only its own aim"
        );
    }

    /// The capture hook has no hand and no key: the band holding the
    /// parked pointer paints the whole aim, and every other band echoes it.
    #[test]
    fn a_forced_aim_paints_whole_where_parked_and_echoes_elsewhere() {
        let dir = ScratchDir::new("echo-forced");
        let account = marked_account(&dir);
        let mut desk = Desk::default();
        desk.gesture.cmd_preview_force = CmdPreviewForce::parse("buy");
        desk.handle_input(
            &account,
            &frame(Some(Point::new(200.0, 250.0)), HeldKeys::default()),
            Some(&AIMED),
        );
        let preview = desk.gesture.cmd_preview.expect("the hook forces an aim");
        assert!(preview.forced);

        assert!(matches!(
            desk.aim_paint(aimed_band(), false, HeldKeys::default(), &AIMED),
            Some(AimPaint::Full(_))
        ));
        assert_eq!(
            desk.aim_paint(other_band(), false, HeldKeys::default(), &OTHER),
            Some(AimPaint::Echo {
                side: Side::Buy,
                y: OTHER.y(95.0),
            })
        );
    }
}
